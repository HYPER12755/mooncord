//! Supabase-based login flow for Concord.
//!
//! This module implements the interactive login TUI, presenting the user
//! with a set of login methods: token, email/password, magic link, email OTP,
//! GitHub OAuth, and Google OAuth. On successful authentication it returns
//! the resulting [`SessionTokens`](crate::backend::token_store::SessionTokens).

use std::time::Duration;

use crossterm::event::{self, EventStream};
use futures::StreamExt;
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::backend::{SupabaseAuthSession, token_store::SessionTokens};
use crate::error::AppError;
use crate::logging;

mod render;
mod state;
mod terminal_events;

use render::render;
use state::LoginState;
use terminal_events::{handle_terminal, LoginAction};

/// Prompt the user interactively for a Supabase login method.
///
/// Returns the resulting [`SessionTokens`] on success, or an error.
pub(super) async fn prompt_login_with_supabase(
    notice: Option<String>,
    warnings: Vec<String>,
    auth_session: SupabaseAuthSession,
) -> crate::Result<SessionTokens> {
    let mut state = LoginState::default();
    state.notice = notice;
    for warning in &warnings {
        logging::error("app", warning);
    }

    // Set up terminal for login screen
    let original_hook = std::panic::take_hook();
    let terminal = match Terminal::new(CrosstermBackend::new(std::io::stdout())) {
        Ok(t) => t,
        Err(e) => return Err(AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e))),
    };
    let _guard = {
        use crossterm::terminal::{enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
        enable_raw_mode()?;
        let _ = crossterm::execute!(
            std::io::stdout(),
            EnterAlternateScreen,
            crossterm::event::EnableMouseCapture
        );
        // On panic or drop, restore the terminal
        struct RestoreGuard;
        impl Drop for RestoreGuard {
            fn drop(&mut self) {
                let _ = crossterm::execute!(
                    std::io::stdout(),
                    LeaveAlternateScreen,
                    crossterm::event::DisableMouseCapture
                );
                let _ = crossterm::terminal::disable_raw_mode();
            }
        }
        RestoreGuard
    };
    let _ = original_hook;

    let result = run_login_screen(&auth_session, &mut state, terminal).await;

    // Ensure terminal is restored
    let _ = _guard;

    result
}

async fn run_login_screen(
    auth_session: &SupabaseAuthSession,
    state: &mut LoginState,
    mut terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
) -> crate::Result<SessionTokens> {
    let mut terminal_events = EventStream::new();

    // Render loop
    loop {
        // Render the current screen
        terminal.draw(|frame| {
            render(frame, state);
        })?;

        // Poll for terminal events via EventStream (async, non-blocking)
        let maybe_event = terminal_events.next().await;

        if let Some(Ok(event)) = maybe_event {
            let action = handle_terminal(state, event);
            if let Some(action) = action {
                let result = dispatch_action(auth_session, state, action).await;
                match result {
                    Ok(Some(tokens)) => return Ok(tokens),
                    Ok(None) => continue,
                    Err(error) => {
                        state.screen = state::LoginScreen::ModeSelect;
                        state.in_progress = false;
                        state.error = Some(format!("{error}"));
                    }
                }
            }
        }
    }
}

async fn dispatch_action(
    auth_session: &SupabaseAuthSession,
    state: &mut LoginState,
    action: LoginAction,
) -> crate::Result<Option<SessionTokens>> {
    match action {
        LoginAction::SubmitToken(token) => {
            // Validate and parse the JWT
            let exp = extract_jwt_exp(&token)?;
            let tokens = SessionTokens::new(token.clone(), String::new(), exp);
            // Verify with the auth server
            match auth_session.get_current_user(Some(&token)).await {
                Ok(_) => Ok(Some(tokens)),
                Err(error) => {
                    state.error = Some(format!("Token verification failed: {error}"));
                    state.screen = state::LoginScreen::ModeSelect;
                    Ok(None)
                }
            }
        }
        LoginAction::SubmitPassword { email, password } => {
            state.in_progress = true;
            state.status = "Signing in...".to_string();
            state.screen = state::LoginScreen::Busy;

            match auth_session.sign_in_with_password(&email, &password).await {
                Ok(result) => {
                    let tokens = SessionTokens::new(
                        result.access_token,
                        result.refresh_token,
                        chrono::Utc::now().timestamp() as u64 + result.expires_in as u64,
                    );
                    return Ok(Some(tokens));
                }
                Err(error) => {
                    state.error = Some(format!("Password login failed: {error}"));
                    state.screen = state::LoginScreen::ModeSelect;
                    state.in_progress = false;
                }
            }
            Ok(None)
        }
        LoginAction::SubmitSignup { email, password } => {
            state.in_progress = true;
            state.status = "Creating account...".to_string();
            state.screen = state::LoginScreen::Busy;

            match auth_session.sign_up(&email, &password).await {
                Ok(result) => {
                    let tokens = SessionTokens::new(
                        result.access_token,
                        result.refresh_token,
                        chrono::Utc::now().timestamp() as u64 + result.expires_in as u64,
                    );
                    return Ok(Some(tokens));
                }
                Err(error) => {
                    state.error = Some(format!("Signup failed: {error}"));
                    state.screen = state::LoginScreen::ModeSelect;
                    state.in_progress = false;
                }
            }
            Ok(None)
        }
        LoginAction::StartGithubOauth => {
            let url = auth_session.get_oauth_url("github");
            state.oauth_url = Some(url.clone());
            state.screen = state::LoginScreen::OauthUrl;
            logging::debug("app", format!("GitHub OAuth URL: {url}"));
            Ok(None)
        }
        LoginAction::StartGoogleOauth => {
            let url = auth_session.get_oauth_url("google");
            state.oauth_url = Some(url.clone());
            state.screen = state::LoginScreen::OauthUrl;
            logging::debug("app", format!("Google OAuth URL: {url}"));
            Ok(None)
        }
        LoginAction::Cancel => {
            cancel_login(state);
            Err(AppError::LoginCancelled)
        }
        LoginAction::Quit => {
            std::process::exit(0);
        }
    }
}

fn cancel_login(state: &mut LoginState) {
    state.screen = state::LoginScreen::ModeSelect;
    state.in_progress = false;
    state.status.clear();
    state.oauth_url = None;
    state.error = None;
    state.reset_sensitive();
}

/// Extract the `exp` claim from a JWT payload.
fn extract_jwt_exp(token: &str) -> crate::Result<u64> {
    use base64::Engine;
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(AppError::DiscordTokenRejected);
    }
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|_| AppError::DiscordTokenRejected)?;
    let claims: serde_json::Value = serde_json::from_slice(&payload)
        .map_err(|_| AppError::DiscordTokenRejected)?;
    claims
        .get("exp")
        .and_then(|v| v.as_u64())
        .ok_or(AppError::DiscordTokenRejected)
}
