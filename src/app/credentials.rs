use crate::{
    backend::{SupabaseAuthSession, SessionTokens},
    config,
    error::AppError,
    logging,
    Result,
    token_store,
    tui,
};

pub(super) struct ResolvedToken {
    pub(super) tokens: SessionTokens,
    pub(super) warnings: Vec<String>,
}

pub(super) async fn resolve_token(auth_session: SupabaseAuthSession) -> Result<ResolvedToken> {
    let mut login_notice = None;
    let mut post_login_warnings = Vec::new();

    if let Some(token) = token_store::env_token() {
        validate_supabase_token(&token)?;
        let tokens = SessionTokens::from_jwt_string(&token);
        return Ok(ResolvedToken {
            tokens,
            warnings: post_login_warnings,
        });
    }

    let credential_store = match config::load_options() {
        Ok(options) => options.credentials.store,
        Err(error) => {
            let warning = format!(
                "config could not be loaded for credential settings: {error}; using auto credential storage"
            );
            post_login_warnings.push(warning);
            login_notice =
                Some("Credential storage is unavailable; token may not be saved.".to_owned());
            config::CredentialStoreMode::Auto
        }
    };

    match load_token_from_store(credential_store).await {
        Ok(Some(tokens)) => {
            if let Err(error) = validate_supabase_token(&tokens.access_token) {
                match error {
                    AppError::SupabaseAuth(msg) => {
                        return Err(AppError::SupabaseAuth(
                            format!("stored token rejected: {msg}"),
                        ));
                    }
                    error => post_login_warnings.push(format!(
                        "Stored token could not be verified: {error}; re-prompting for login"
                    )),
                }
            } else {
                // Verify against the Supabase auth server
                match auth_session
                    .get_current_user(Some(&tokens.access_token))
                    .await
                {
                    Ok(_) => return Ok(ResolvedToken {
                        tokens,
                        warnings: post_login_warnings,
                    }),
                    Err(error) => post_login_warnings.push(format!(
                        "Stored token verification failed: {error}; re-prompting for login"
                    )),
                }
            }
        }
        Ok(None) => {}
        Err(error) => {
            let warning = format!(
                "credential store was unavailable: {error}; falling back to interactive login"
            );
            post_login_warnings.push(warning);
        }
    }

    let login_result = tui::prompt_login_with_supabase(
        login_notice,
        post_login_warnings.clone(),
        auth_session,
    )
    .await?;
    let tokens = login_result;
    let warnings = post_login_warnings.clone();

    match credential_store {
        config::CredentialStoreMode::Auto | config::CredentialStoreMode::Keychain => {
            if let Err(error) = save_token_to_store(&tokens.access_token, credential_store).await {
                logging::error("credentials", format!(
                    "could not save token to credential store: {error}"
                ));
            }
        }
        config::CredentialStoreMode::Plain => {
            if let Err(error) = save_token_to_store(&tokens.access_token, credential_store).await {
                logging::error("credentials", format!(
                    "could not save token to credential store: {error}"
                ));
            }
        }
    }

    Ok(ResolvedToken {
        tokens,
        warnings,
    })
}

/// Validate a Supabase JWT by checking structure and expiry.
fn validate_supabase_token(token: &str) -> Result<()> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(AppError::DiscordTokenRejected);
    }
    let payload = base64_url_decode(parts[1])?;
    let claims: serde_json::Value = serde_json::from_slice(&payload)
        .map_err(|_| AppError::DiscordTokenRejected)?;
    let exp = claims.get("exp").and_then(|v| v.as_u64()).unwrap_or(0);
    let now = chrono::Utc::now().timestamp() as u64;
    if exp < now {
        return Err(AppError::DiscordTokenRejected);
    }
    Ok(())
}

fn base64_url_decode(input: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(input)
        .map_err(|_| AppError::DiscordTokenRejected)
}

async fn load_token_from_store(
    store: config::CredentialStoreMode,
) -> Result<Option<SessionTokens>> {
    let token_string = token_store::load_token(store)?;
    match token_string {
        Some(token) => {
            let tokens = SessionTokens::from_jwt_string(&token);
            Ok(Some(tokens))
        }
        None => Ok(None),
    }
}

async fn save_token_to_store(
    token: &str,
    store: config::CredentialStoreMode,
) -> Result<()> {
    token_store::save_token(token, store)?;
    Ok(())
}
