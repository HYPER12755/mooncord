mod command_loop;
mod credentials;
mod shutdown;

use std::sync::Arc;

use crate::{
    Result, config,
    backend::{BackendClient, SupabaseAuthSession, SupabaseClient, SessionTokens},
    discord::AppEvent,
    logging, tui, version_check,
};

use self::{credentials::resolve_token, shutdown::shutdown_backend};

#[derive(Default)]
pub struct App;

impl App {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(self) -> Result<()> {
        let theme_warnings = match config::load_theme_options_with_warnings() {
            Ok((theme_options, mut warnings)) => {
                warnings.extend(tui::initialize_theme(&theme_options));
                warnings
            }
            Err(error) => {
                logging::error("config", format!("failed to load theme config: {error}"));
                let mut warnings = vec![format!("theme.toml could not be loaded: {error}")];
                warnings.extend(tui::initialize_theme(&config::ThemeOptions::default()));
                warnings
            }
        };

        loop {
            let auth_session = SupabaseAuthSession::new()?;
            let resolved_token = resolve_token(auth_session.clone()).await?;
            let tokens: SessionTokens = resolved_token.tokens;
            let token_warnings = resolved_token.warnings;

            let supabase_url = std::env::var("SUPABASE_URL")
                .unwrap_or_else(|_| panic!("SUPABASE_URL not set"));
            let supabase_key = std::env::var("SUPABASE_ANON_KEY")
                .unwrap_or_else(|_| panic!("SUPABASE_ANON_KEY not set"));

            // Initialize the REST client — all DB operations go through PostgREST (REST API)
            let (client_inner, backend_commands_tx) = SupabaseClient::new(
                supabase_url,
                supabase_key,
                tokens.clone(),
            );
            let client: Arc<dyn BackendClient> = Arc::new(client_inner);
            let effects = client.take_effects();
            let snapshots = client.subscribe_snapshots();
            let commands_tx = backend_commands_tx;
            let (app_options, app_warnings) = match config::load_options_with_warnings() {
                Ok(loaded) => loaded,
                Err(error) => {
                    logging::error("config", format!("failed to load config: {error}"));
                    (
                        config::AppOptions::default(),
                        vec![format!("config.toml could not be loaded: {error}")],
                    )
                }
            };
            let mut config_warnings = theme_warnings.clone();
            config_warnings.extend(app_warnings);
            let gateway_task = client.start();

            let version_client = client.clone();
            tokio::spawn(async move {
                match version_check::check_latest_version().await {
                    Ok(Some(latest_version)) => {
                        version_client
                            .publish_event(AppEvent::UpdateAvailable { latest_version })
                            .await;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        logging::debug("version", format!("latest version check failed: {error}"))
                    }
                }
            });

            let result = async {
                for warning in token_warnings {
                    logging::error("app", &warning);
                    client
                        .publish_event(AppEvent::GatewayError { message: warning })
                        .await;
                }

                tui::run_with_options(
                    effects,
                    snapshots,
                    commands_tx,
                    client.clone(),
                    app_options,
                    config_warnings,
                )
                .await
            }
            .await;

            shutdown_backend(&client, gateway_task).await;
            match result? {
                tui::DashboardExit::Quit => return Ok(()),
                // Sign-out of an env-token session quits: re-resolving would
                // read the same CONCORD_TOKEN and log straight back in.
                tui::DashboardExit::SignOut => {
                    if crate::token_store::env_token().is_some() {
                        return Ok(());
                    }
                }
            }
        }
    }
}
