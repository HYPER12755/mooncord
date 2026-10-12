mod clipboard;
mod commands;
mod fuzzy;
#[cfg(feature = "voice-playback")]
mod global_push_to_talk;
mod input;
mod keybindings;
mod login;
mod media;
mod message;
mod runtime;
mod selection;
mod state;
mod terminal;
mod text;
mod text_cursor;
mod text_input;
mod theme;
mod ui;

use tokio::sync::{mpsc, watch};

use crate::{
    AppError, Result,
    config::{AppOptions, KeymapOptions, ThemeOptions},
    discord::{AppCommand, SequencedAppEvent, SnapshotRevision},
};

pub use runtime::DashboardExit;

pub fn validate_keymap_options(keymap_options: &KeymapOptions) -> Result<()> {
    keybindings::KeyBindings::try_from_options(keymap_options)
        .map(|_| ())
        .map_err(AppError::InvalidKeymapConfig)
}

/// Resolves `theme_options` against the built-in defaults and returns any
/// per-field warnings, without applying the result. Theme values never fail
/// startup outright (an unparseable color just falls back), so this is a
/// report, not a pass/fail check like [`validate_keymap_options`].
pub fn theme_options_warnings(theme_options: &ThemeOptions) -> Vec<String> {
    let mut warnings = Vec::new();
    theme::Theme::from_options(theme_options, &mut warnings);
    warnings
}

pub fn initialize_theme(theme_options: &ThemeOptions) -> Vec<String> {
    let mut warnings = Vec::new();
    let resolved = theme::Theme::from_options(theme_options, &mut warnings);
    theme::init(resolved);
    warnings
}

pub async fn prompt_login_with_supabase(
    notice: Option<String>,
    post_login_warnings: Vec<String>,
    auth_session: crate::backend::SupabaseAuthSession,
) -> Result<crate::backend::SessionTokens> {
    login::prompt_login_with_supabase(notice, post_login_warnings, auth_session).await
}

pub async fn run(
    effects: mpsc::Receiver<SequencedAppEvent>,
    snapshots: watch::Receiver<SnapshotRevision>,
    commands: mpsc::Sender<AppCommand>,
    client: std::sync::Arc<dyn crate::backend::BackendClient>,
    mut config_warnings: Vec<String>,
) -> Result<DashboardExit> {
    let options = match crate::config::load_options_with_warnings() {
        Ok((options, warnings)) => {
            config_warnings.extend(warnings);
            options
        }
        Err(error) => {
            crate::logging::error("config", format!("failed to load config: {error}"));
            AppOptions::default()
        }
    };
    run_with_options(
        effects,
        snapshots,
        commands,
        client,
        options,
        config_warnings,
    )
    .await
}

pub(crate) async fn run_with_options(
    mut effects: mpsc::Receiver<SequencedAppEvent>,
    mut snapshots: watch::Receiver<SnapshotRevision>,
    commands: mpsc::Sender<AppCommand>,
    client: std::sync::Arc<dyn crate::backend::BackendClient>,
    options: AppOptions,
    config_warnings: Vec<String>,
) -> Result<DashboardExit> {
    let mut terminal = ratatui::init();
    let _restore_guard = terminal::TerminalRestoreGuard::new();

    runtime::run_dashboard(
        &mut terminal,
        &mut effects,
        &mut snapshots,
        commands,
        client,
        options,
        config_warnings,
    )
    .await
}
