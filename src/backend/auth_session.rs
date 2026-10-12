//! Supabase authentication session.
//!
//! Replaces [`DiscordAuthSession`](crate::discord::DiscordAuthSession).
//! Handles sign-in with email/password, magic link (email OTP), OTP, and
//! OAuth ID tokens (GitHub, Google, etc.). All auth goes through the Supabase
//! Auth REST API (`/auth/v1`).

use crate::error::AppError;
use crate::Result;
use serde::{Deserialize, Serialize};

/// Supabase authentication session.
#[derive(Clone)]
pub struct SupabaseAuthSession {
    pub supabase_url: String,
    pub anon_key: String,
    pub http: reqwest::Client,
}

/// A verified user identity from Supabase Auth.
#[derive(Clone, Debug)]
pub struct AuthenticatedUser {
    pub id: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

// Supabase Auth API response types

#[derive(Deserialize)]
struct SupabaseUser {
    id: String,
    email: Option<String>,
    user_metadata: Option<serde_json::Map<String, serde_json::Value>>,
    app_metadata: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Deserialize)]
struct SupabaseSession {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
    user: SupabaseUser,
}

#[derive(Deserialize)]
struct SupabaseError {
    #[allow(dead_code)]
    msg: Option<String>,
    code: Option<String>,
}

#[derive(Serialize)]
struct SignInWithPasswordRequest {
    email: String,
    password: String,
}

#[derive(Serialize)]
struct SignInWithIdTokenRequest {
    provider: String,
    token: String,
}

#[derive(Serialize)]
struct RefreshTokenRequest {
    refresh_token: String,
}

/// Result of a sign-in attempt.
#[derive(Clone, Debug)]
pub struct SignInResult {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub user: AuthenticatedUser,
    pub new_user: bool,
}

/// Events emitted during the auth flow.
#[derive(Clone, Debug)]
pub enum SupabaseAuthEvent {
    Status(String),
    UserAuthenticated(SignInResult),
    MfaRequired {
        ticket: String,
        factors: Vec<MfaFactor>,
    },
    OauthUrl(String),
    Failed(String),
}

/// An MFA factor available for the user.
#[derive(Clone, Debug)]
pub enum MfaFactor {
    Totp,
    WebAuthn,
}

/// OAuth providers supported.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OAuthProvider {
    Github,
    Google,
    Gitlab,
    Bitbucket,
}

impl OAuthProvider {
    pub fn name(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Google => "google",
            Self::Gitlab => "gitlab",
            Self::Bitbucket => "bitbucket",
        }
    }
}

impl SupabaseAuthSession {
    /// Load configuration from the environment.
    pub fn new() -> Result<Self> {
        let supabase_url = std::env::var("SUPABASE_URL")
            .map_err(|_| AppError::ConfigLoad("SUPABASE_URL not set".to_string()))?;
        let anon_key = std::env::var("SUPABASE_ANON_KEY")
            .map_err(|_| AppError::ConfigLoad("SUPABASE_ANON_KEY not set".to_string()))?;

        Ok(Self {
            supabase_url,
            anon_key,
            http: reqwest::Client::new(),
        })
    }

    fn auth_endpoint(&self) -> String {
        format!("{}/auth/v1", self.supabase_url)
    }

    /// Get the current authenticated user. Token is optional; if not
    /// provided the anon key is used (which won't return a user).
    pub async fn get_current_user(&self, token: Option<&str>) -> Result<AuthenticatedUser> {
        let url = format!("{}/auth/v1/user", self.supabase_url);
        let auth_token = token.unwrap_or(&self.anon_key);
        let resp = self.http
            .get(&url)
            .header("apikey", self.anon_key.clone())
            .bearer_auth(auth_token)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(AppError::SupabaseAuth("failed to fetch user from Supabase".to_string()));
        }

        let user: SupabaseUser = resp.json().await?;
        let display_name = user.user_metadata.as_ref()
            .and_then(|m| m.get("full_name").or_else(|| m.get("name")))
            .and_then(|v| v.as_str().map(|s| s.to_string()));
        let avatar_url = user.user_metadata.as_ref()
            .and_then(|m| m.get("avatar_url").or_else(|| m.get("picture")))
            .and_then(|v| v.as_str().map(|s| s.to_string()));

        Ok(AuthenticatedUser {
            id: user.id,
            email: user.email,
            display_name,
            avatar_url,
        })
    }

    /// Sign in with email + password.
    pub async fn sign_in_with_password(&self, email: &str, password: &str) -> Result<SignInResult> {
        let request = SignInWithPasswordRequest {
            email: email.to_string(),
            password: password.to_string(),
        };

        let url = self.auth_endpoint();
        let resp = self.http.post(&format!("{url}/signin"))
            .header("apikey", self.anon_key.clone())
            .json(&request)
            .send().await?;

        self.handle_signin_response(resp).await
    }

    /// Sign in with an ID token from an OAuth provider (e.g. a Discord or
    /// GitHub access token obtained out-of-band).
    pub async fn sign_in_with_id_token(&self, provider: &str, token: &str) -> Result<SignInResult> {
        let request = SignInWithIdTokenRequest {
            provider: provider.to_string(),
            token: token.to_string(),
        };

        let url = self.auth_endpoint();
        let resp = self.http.post(&format!("{url}/token"))
            .header("apikey", self.anon_key.clone())
            .header("Accept", "application/json")
            .json(&request)
            .send().await?;

        self.handle_signin_response(resp).await
    }

    /// Sign up with email + password.
    pub async fn sign_up(&self, email: &str, password: &str) -> Result<SignInResult> {
        let request = serde_json::json!({
            "email": email,
            "password": password,
        });

        let url = self.auth_endpoint();
        let resp = self.http.post(&format!("{url}/signup"))
            .header("apikey", self.anon_key.clone())
            .json(&request)
            .send().await?;

        // Supabase may or may not return a session in the signup response.
        // If the project has email confirmation enabled, no session is returned.
        // In that case, fall back to sign-in to obtain tokens.
        if resp.status().is_success() {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            if body.get("access_token").is_some() {
                return self.handle_signin_response_from_value(body).await;
            }
            // No session — sign in to get one
            return self.sign_in_with_password(email, password).await;
        }

        // Signup failed
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let msg = body.get("msg")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                body.get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("signup failed")
            });
        Err(AppError::SupabaseAuth(msg.to_string()))
    }

    /// Refresh the access token using a refresh token.
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<SignInResult> {
        let request = RefreshTokenRequest {
            refresh_token: refresh_token.to_string(),
        };
        let url = self.auth_endpoint();
        let resp = self.http.post(&format!("{url}/refresh"))
            .header("apikey", self.anon_key.clone())
            .json(&request)
            .send().await?;

        self.handle_signin_response(resp).await
    }

    /// Returns the provider authorization URL for OAuth login.
    pub fn get_oauth_url(&self, provider: &str) -> String {
        format!(
            "{}/auth/v1/authorize?provider={}&redirect_to=concord://auth/callback",
            self.supabase_url, provider
        )
    }

    async fn handle_signin_response(&self, resp: reqwest::Response) -> Result<SignInResult> {
        let status = resp.status();
        let body: serde_json::Value = resp.json().await.unwrap_or_default();

        if !status.is_success() {
            let msg = body.get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| {
                    body.get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("authentication failed")
                });
            return Err(AppError::SupabaseAuth(msg.to_string()));
        }

        self.handle_signin_response_from_value(body).await
    }

    async fn handle_signin_response_from_value(&self, body: serde_json::Value) -> Result<SignInResult> {
        let session: SupabaseSession = serde_json::from_value(body.clone())
            .map_err(|_| AppError::SupabaseAuth("invalid session response from server".to_string()))?;

        let display_name = session.user.user_metadata.as_ref()
            .and_then(|m| m.get("full_name").or_else(|| m.get("name")))
            .and_then(|v| v.as_str().map(|s| s.to_string()));
        let avatar_url = session.user.user_metadata.as_ref()
            .and_then(|m| m.get("avatar_url").or_else(|| m.get("picture")))
            .and_then(|v| v.as_str().map(|s| s.to_string()));

        let new_user = body.get("new_user").and_then(|v| v.as_bool()).unwrap_or(false);

        Ok(SignInResult {
            access_token: session.access_token,
            refresh_token: session.refresh_token,
            expires_in: session.expires_in,
            user: AuthenticatedUser {
                id: session.user.id,
                email: session.user.email,
                display_name,
                avatar_url,
            },
            new_user,
        })
    }
}
