//! Supabase-backed Concord backend.
//!
//! This module provides [`SupabaseClient`], a drop-in replacement for
//! [`DiscordClient`] that speaks to Supabase
//! Postgres + Realtime instead of the Discord Gateway. It maintains the
//! same `AppCommand` → `AppEvent`/`SequencedAppEvent` → snapshot contract,
//! so the TUI layer requires zero changes.
//!
//! ## Architecture
//!
//! ```text
//!     TUI
//!       │  mpsc::Sender<AppCommand>
//!       ▼
//!     AppState ──┐
//!                │  watch::Sender<SnapshotRevision>
//!                ▼
//!     SupabaseClient ──► Postgres (REST via PostgREST)
//!                       ──► Realtime (WebSocket)
//!                       ──► Broadcast (private channels for DMs)
//! ```
//!
//! ## Phases
//!
//! - Phase 0: Postgres schema + RLS (migrations/*.sql)
//! - Phase 1: Auth wiring (JWT validation + user lookup)
//! - Phase 2: Ready snapshot load + incremental Realtime subscriptions
//! - Phase 3: Voice via LiveKit (deferred)

mod auth_session;
mod backend_client;
mod client;
mod commands;
mod realtime;
mod rest_client;
pub mod token_store;

pub use auth_session::{MfaFactor, OAuthProvider, SignInResult, SupabaseAuthSession, AuthenticatedUser};
pub(crate) use backend_client::BackendClient;
pub use client::SupabaseClient;
pub use commands::CommandContext;
pub use realtime::RealtimeClient;
pub use rest_client::{get_column, get_column_from_value, Row, SupabaseRestClient};
pub use token_store::SessionTokens;
