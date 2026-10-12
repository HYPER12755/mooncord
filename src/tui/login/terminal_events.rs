use crossterm::event::{Event as TerminalEvent, KeyEventKind};

use super::state::{LoginScreen, FormField, LoginState};

/// Quick syntactic check: a JWT must have 3 dot-separated base64url segments.
fn is_valid_jwt(token: &str) -> bool {
    token.split('.').count() == 3 && !token.trim().is_empty()
}
use crate::tui::keybindings::{
    LoginBusyAction, LoginGlobalAction, LoginModeSelectAction,
    LoginPasswordInputAction, LoginTextInputAction,
};

pub(super) enum LoginAction {
    /// Submit a raw token string (from token input).
    SubmitToken(String),
    /// Submit email + password for password login.
    SubmitPassword { email: String, password: String },
    /// Sign up a new account with email + password.
    SubmitSignup { email: String, password: String },
    /// Start GitHub OAuth — returns immediately, the caller opens the URL.
    StartGithubOauth,
    /// Start Google OAuth.
    StartGoogleOauth,
    Cancel,
    Quit,
}

pub(super) fn handle_terminal(state: &mut LoginState, event: TerminalEvent) -> Option<LoginAction> {
    let key = match event {
        TerminalEvent::Key(key) => key,
        TerminalEvent::Paste(text) => return handle_paste(state, &text),
        _ => return None,
    };
    if key.kind != KeyEventKind::Press {
        return None;
    }
    match state.key_bindings.login_global_action(key) {
        Some(LoginGlobalAction::Cancel) => return Some(LoginAction::Cancel),
        Some(LoginGlobalAction::Quit) => return Some(LoginAction::Quit),
        None => {}
    }

    match state.screen {
        LoginScreen::ModeSelect => match state.key_bindings.login_mode_select_action(key) {
            Some(LoginModeSelectAction::StartToken) => {
                state.screen = LoginScreen::TokenInput;
                state.error = None;
                state.token_input.clear();
                None
            }
            Some(LoginModeSelectAction::StartPassword) => {
                state.screen = LoginScreen::EmailPassword;
                state.error = None;
                state.reset_form();
                state.active_field = FormField::Email;
                None
            }
            Some(LoginModeSelectAction::StartGithub) => {
                state.screen = LoginScreen::Busy;
                state.error = None;
                state.in_progress = true;
                state.status = "Starting GitHub OAuth...".to_string();
                Some(LoginAction::StartGithubOauth)
            }
            Some(LoginModeSelectAction::StartGoogle) => {
                state.screen = LoginScreen::Busy;
                state.error = None;
                state.in_progress = true;
                state.status = "Starting Google OAuth...".to_string();
                Some(LoginAction::StartGoogleOauth)
            }
            Some(LoginModeSelectAction::Cancel) => Some(LoginAction::Cancel),
            None => None,
        },
        LoginScreen::TokenInput => match state.key_bindings.login_text_input_action(key) {
            LoginTextInputAction::Submit => {
                let token = state.token_input.trim();
                if token.is_empty() {
                    state.error = Some("Token cannot be empty".to_string());
                    None
                } else if !is_valid_jwt(token) {
                    state.error = Some("Token is not a valid JWT".to_string());
                    None
                } else {
                    Some(LoginAction::SubmitToken(token.to_string()))
                }
            }
            LoginTextInputAction::Back => {
                state.screen = LoginScreen::ModeSelect;
                state.token_input.clear();
                state.error = None;
                None
            }
            LoginTextInputAction::DeletePreviousChar => {
                state.token_input.pop();
                state.error = None;
                None
            }
            LoginTextInputAction::InsertChar(value) => {
                state.token_input.push(value);
                state.error = None;
                None
            }
            LoginTextInputAction::Ignore => None,
        },
        LoginScreen::EmailPassword => {
            if state.in_progress {
                return match state.key_bindings.login_busy_action(key) {
                    LoginBusyAction::Cancel => {
                        state.screen = LoginScreen::ModeSelect;
                        state.in_progress = false;
                        Some(LoginAction::Cancel)
                    }
                    LoginBusyAction::Ignore => None,
                };
            }
            match state.key_bindings.login_password_input_action(key) {
                LoginPasswordInputAction::Submit => {
                    let email = state.email.trim().to_string();
                    let password = state.password.clone();
                    if email.is_empty() || password.is_empty() {
                        state.error = Some("Email and password are required".to_string());
                        None
                    } else if state.signup_mode {
                        state.password.clear();
                        Some(LoginAction::SubmitSignup { email, password })
                    } else {
                        state.password.clear();
                        Some(LoginAction::SubmitPassword { email, password })
                    }
                }
                LoginPasswordInputAction::ToggleSignupMode => {
                    state.signup_mode = !state.signup_mode;
                    state.error = None;
                    None
                }
                LoginPasswordInputAction::SwitchField => {
                    state.active_field = match state.active_field {
                        FormField::Email => FormField::Password,
                        FormField::Password => FormField::Email,
                    };
                    state.error = None;
                    None
                }
                LoginPasswordInputAction::Back => {
                    state.screen = LoginScreen::ModeSelect;
                    state.error = None;
                    state.reset_sensitive();
                    None
                }
                LoginPasswordInputAction::DeletePreviousChar => {
                    active_form_input(state).pop();
                    state.error = None;
                    None
                }
                LoginPasswordInputAction::InsertChar(value) => {
                    active_form_input(state).push(value);
                    state.error = None;
                    None
                }
                LoginPasswordInputAction::Ignore => None,
            }
        }
        LoginScreen::Busy => match state.key_bindings.login_busy_action(key) {
            LoginBusyAction::Cancel => {
                state.screen = LoginScreen::ModeSelect;
                state.in_progress = false;
                state.status.clear();
                Some(LoginAction::Cancel)
            }
            LoginBusyAction::Ignore => None,
        },
        LoginScreen::OauthUrl => match state.key_bindings.login_busy_action(key) {
            LoginBusyAction::Cancel => {
                state.screen = LoginScreen::ModeSelect;
                state.oauth_url = None;
                Some(LoginAction::Cancel)
            }
            LoginBusyAction::Ignore => None,
        },
        LoginScreen::MfaSelect | LoginScreen::MfaCode => {
            // TODO: handle MFA selection/input
            None
        }
    }
}

fn handle_paste(state: &mut LoginState, text: &str) -> Option<LoginAction> {
    match state.screen {
        LoginScreen::TokenInput => {
            append_paste(&mut state.token_input, text);
            state.error = None;
        }
        LoginScreen::EmailPassword => {
            if state.in_progress {
                return None;
            }
            append_paste(active_form_input(state), text);
            state.error = None;
        }
        LoginScreen::ModeSelect
        | LoginScreen::MfaSelect
        | LoginScreen::MfaCode
        | LoginScreen::Busy
        | LoginScreen::OauthUrl => {}
    }
    None
}

fn active_form_input(state: &mut LoginState) -> &mut String {
    match state.active_field {
        FormField::Email => &mut state.email,
        FormField::Password => &mut state.password,
    }
}

fn append_paste(target: &mut String, text: &str) {
    target.extend(text.chars().filter(|value| !matches!(value, '\r' | '\n')));
}
