//! Supabase-backed client implementing the same `AppCommand` → `AppEvent` contract
//! as the Discord client did previously.
//!
//! Emits the same `AppEvent`s and `SnapshotRevision`s the TUI consumes,
//! so the frontend layer requires zero changes.

use crate::discord::{
    AppEvent, AppCommand, AppEventPublisher, ApplicationCommandCache, ChannelInfo,
    DiscordSnapshot, DiscordState, GuildBoostTier, Id, MessageAttachmentUpload,
    RequestState, RequestLifecycle, SequencedAppEvent,
    SnapshotRevision, ReplyReference, marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
};

use base64::Engine;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::{Mutex as SyncMutex, RwLock as SyncRwLock};
use tokio::sync::mpsc;
use tokio::sync::watch;
use tokio::sync::Mutex as AsyncMutex;

use super::rest_client::{get_column, SupabaseRestClient};
use super::token_store::SessionTokens;

/// The Supabase backend client.
///
/// Exposes the same interface as the Discord client did previously:
/// - `take_effects()` → `mpsc::Receiver<SequencedAppEvent>`
/// - `subscribe_snapshots()` → `watch::Receiver<SnapshotRevision>`
/// - `start()` → spawns the command-processing loop
pub struct SupabaseClient {
    supabase_url: String,
    supabase_key: String,
    rest_client: SupabaseRestClient,
    tokens: SessionTokens,
    pub(crate) user_id: Option<Id<UserMarker>>,

    // Event output — matches Discord client's effects channel
    #[allow(dead_code)]
    pub(crate) effects_tx: mpsc::Sender<SequencedAppEvent>,
    effects_rx: AsyncMutex<Option<mpsc::Receiver<SequencedAppEvent>>>,

    // Snapshot output
    snapshots_tx: watch::Sender<SnapshotRevision>,

    // Command input
    commands_rx: AsyncMutex<Option<mpsc::Receiver<AppCommand>>>,
    commands_tx: mpsc::Sender<AppCommand>,

    // Shared state + publisher (reuses Discord client's machinery)
    publisher: AppEventPublisher,
}

impl SupabaseClient {
    pub fn new(
        supabase_url: String,
        supabase_key: String,
        tokens: SessionTokens,
    ) -> (Self, mpsc::Sender<AppCommand>) {
        let (effects_tx, effects_rx) = mpsc::channel(4096);
        let (snapshots_tx, _) = watch::channel(SnapshotRevision::default());
        let (commands_tx, commands_rx) = mpsc::channel(4096);
        let (voice_events_tx, _voice_events_rx) = mpsc::unbounded_channel();

        let state = Arc::new(SyncRwLock::new(DiscordState::default()));
        let revision = Arc::new(SyncRwLock::new(SnapshotRevision::default()));
        let publish_lock = Arc::new(AsyncMutex::new(()));
        let application_command_requests: Arc<SyncMutex<HashMap<Option<Id<GuildMarker>>, RequestState>>> =
            Arc::new(SyncMutex::new(HashMap::new()));
        let application_commands: Arc<SyncMutex<ApplicationCommandCache>> =
            Arc::new(SyncMutex::new(HashMap::new()));
        let request_lifecycle = Arc::new(SyncMutex::new(RequestLifecycle::default()));

        let publisher = AppEventPublisher::new_for_backend(
            effects_tx.clone(),
            snapshots_tx.clone(),
            state,
            revision,
            publish_lock,
            application_command_requests,
            application_commands,
            request_lifecycle,
            voice_events_tx,
        );

        let rest_client = SupabaseRestClient::new(&supabase_url, &supabase_key);

        let client = Self {
            supabase_url,
            supabase_key,
            rest_client,
            tokens,
            user_id: None,

            effects_tx,
            effects_rx: AsyncMutex::new(Some(effects_rx)),
            snapshots_tx,

            commands_rx: AsyncMutex::new(Some(commands_rx)),
            commands_tx: commands_tx.clone(),
            publisher,
        };

        (client, commands_tx)
    }

    /// Take the effects (SequencedAppEvent) stream. Can only be called once.
    pub async fn take_effects(&self) -> mpsc::Receiver<SequencedAppEvent> {
        self.effects_rx
            .lock()
            .await
            .take()
            .expect("effects receiver can only be taken once")
    }

    /// Subscribe to snapshot revision updates.
    pub fn subscribe_snapshots(&self) -> watch::Receiver<SnapshotRevision> {
        self.snapshots_tx.subscribe()
    }

    /// Returns the current user ID if authenticated.
    pub async fn current_user_id(&self) -> Option<Id<UserMarker>> {
        self.user_id
    }

    /// Emit a single AppEvent through the shared pipeline.
    /// This applies the event to DiscordState, advances the snapshot revision,
    /// and sends a SequencedAppEvent to the TUI.
    pub async fn publish_event(&self, event: AppEvent) {
        self.publisher.publish_for_backend(&event).await;
    }

    /// Start the backend: process commands, subscribe to Realtime, emit events.
    pub async fn start(&self) -> tokio::task::JoinHandle<()> {
        let rest_client = self.rest_client.clone();
        let tokens = self.tokens.clone();
        let publisher = self.publisher.clone();
        let supabase_url = self.supabase_url.clone();
        let supabase_key = self.supabase_key.clone();

        let mut commands_rx = self
            .commands_rx
            .lock()
            .await
            .take()
            .expect("command receiver can only be taken once");

        tokio::spawn(async move {
            // 1. Bootstrap: load initial snapshot via REST + emit Ready event
            let user_id = fetch_user_id(&rest_client, &tokens).await;

            if let Some(uid) = user_id {
                // Load guilds, channels, members, etc. via REST
                // and emit the equivalent AppEvents that DiscordState::apply_event
                // understands (Ready, GuildCreate, ChannelUpsert, etc.)
                let events = build_ready_events(&rest_client, uid, &tokens).await;
                for event in events {
                    publisher.publish_for_backend(&event).await;
                }

                // 2. Subscribe to Realtime for incremental updates
                subscribe_realtime(
                    &rest_client,
                    &supabase_url,
                    &tokens,
                    &publisher,
                    uid,
                ).await;
            }

            // 3. Process incoming commands
            while let Some(command) = commands_rx.recv().await {
                let mut ctx = crate::backend::commands::CommandContext {
                    rest: rest_client.clone(),
                    tokens: tokens.clone(),
                    publisher: publisher.clone(),
                    user_id,
                    supabase_url: supabase_url.clone(),
                    supabase_key: supabase_key.clone(),
                };
                crate::backend::commands::handle_command(&mut ctx, command).await;
            }
        })
    }
}

impl crate::backend::BackendClient for SupabaseClient {
    fn take_effects(&self) -> mpsc::Receiver<SequencedAppEvent> {
        self.effects_rx
            .blocking_lock()
            .take()
            .expect("effects receiver can only be taken once")
    }

    fn subscribe_snapshots(&self) -> watch::Receiver<SnapshotRevision> {
        self.snapshots_tx.subscribe()
    }

    fn current_discord_snapshot(&self) -> DiscordSnapshot {
        DiscordSnapshot::default()
    }

    fn current_user_id(&self) -> Option<Id<UserMarker>> {
        self.user_id
    }

    fn current_user_rpc_identity(&self) -> Option<(String, String)> {
        None
    }

    fn publish_event(&self, event: AppEvent) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        let publisher = &self.publisher;
        Box::pin(async move {
            publisher.publish_for_backend(&event).await;
        })
    }

    fn start(&self) -> tokio::task::JoinHandle<()> {
        tokio::runtime::Handle::current().block_on(self.start())
    }

    fn shutdown(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async {})
    }

    fn validate_token_authentication(&self, _token: &str) -> crate::Result<crate::backend::AuthenticatedUser> {
        Err(crate::error::AppError::NotLoggedIn)
    }

    fn send_message(
        &self,
        channel_id: Id<ChannelMarker>,
        nonce: Id<MessageMarker>,
        content: &str,
        reply_to: Option<&ReplyReference>,
        attachments: &[MessageAttachmentUpload],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::Result<crate::discord::MessageInfo>> + Send + '_>> {
        let reply_to = reply_to.cloned();
        let attachments = attachments.to_vec();
        let content = content.to_string();
        let tx = self.commands_tx.clone();
        Box::pin(async move {
            let _ = tx.send(AppCommand::SendMessage {
                channel_id,
                nonce,
                content,
                reply_to,
                attachments,
            }).await;
            Err(crate::error::AppError::NotImplemented)
        })
    }

    fn send_tts_message(
        &self,
        channel_id: Id<ChannelMarker>,
        nonce: Id<MessageMarker>,
        content: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::Result<crate::discord::MessageInfo>> + Send + '_>> {
        let content = content.to_string();
        let tx = self.commands_tx.clone();
        Box::pin(async move {
            let _ = tx.send(AppCommand::SendMessage {
                channel_id,
                nonce,
                content,
                reply_to: None,
                attachments: vec![],
            }).await;
            Err(crate::error::AppError::NotImplemented)
        })
    }

    fn edit_message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        edit: &crate::discord::MessageEditRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::Result<crate::discord::MessageInfo>> + Send + '_>> {
        let content = match edit {
            crate::discord::MessageEditRequest::Content(c) => c.to_string(),
            crate::discord::MessageEditRequest::Flags(_) => String::new(),
        };
        let tx = self.commands_tx.clone();
        Box::pin(async move {
            let _ = tx.send(AppCommand::EditMessage {
                channel_id,
                message_id,
                content,
            }).await;
            Err(crate::error::AppError::NotImplemented)
        })
    }

    fn delete_message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::Result<()>> + Send + '_>> {
        let tx = self.commands_tx.clone();
        Box::pin(async move {
            let _ = tx.send(AppCommand::DeleteMessage { channel_id, message_id }).await;
            Ok(())
        })
    }

    fn add_reaction(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &crate::discord::ReactionEmoji,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::Result<()>> + Send + '_>> {
        let emoji = emoji.clone();
        let tx = self.commands_tx.clone();
        Box::pin(async move {
            let _ = tx.send(AppCommand::AddReaction { channel_id, message_id, emoji }).await;
            Ok(())
        })
    }


    fn leave_guild(&self, guild_id: Id<GuildMarker>) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::Result<()>> + Send + '_>> {
        let tx = self.commands_tx.clone();
        Box::pin(async move {
            let _ = tx.send(AppCommand::LeaveGuild { guild_id, label: String::new() }).await;
            Ok(())
        })
    }

    fn trigger_typing(&self, channel_id: Id<ChannelMarker>) -> crate::Result<()> {
        let _ = self.commands_tx.try_send(AppCommand::TriggerTyping { channel_id });
        Ok(())
    }

    fn create_forum_post(
        &self,
        post: &crate::discord::ForumPostCreate,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::Result<crate::discord::CreatedForumPost>> + Send + '_>> {
        let post = post.clone();
        let tx = self.commands_tx.clone();
        Box::pin(async move {
            let _ = tx.send(AppCommand::CreateForumPost { post }).await;
            Err(crate::error::AppError::NotImplemented)
        })
    }

    fn set_thread_archived(
        &self,
        channel_id: Id<ChannelMarker>,
        archived: bool,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::Result<()>> + Send + '_>> {
        let tx = self.commands_tx.clone();
        Box::pin(async move {
            let _ = tx.send(AppCommand::SetThreadArchived { channel_id, archived, label: String::new() }).await;
            Ok(())
        })
    }

    fn set_thread_locked(
        &self,
        channel_id: Id<ChannelMarker>,
        locked: bool,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::Result<()>> + Send + '_>> {
        let tx = self.commands_tx.clone();
        Box::pin(async move {
            let _ = tx.send(AppCommand::SetThreadLocked { channel_id, locked, label: String::new() }).await;
            Ok(())
        })
    }

    fn delete_thread(&self, thread_id: Id<ChannelMarker>) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::Result<()>> + Send + '_>> {
        let tx = self.commands_tx.clone();
        Box::pin(async move {
            let _ = tx.send(AppCommand::DeleteThread { channel_id: thread_id, label: String::new() }).await;
            Ok(())
        })
    }

    fn set_message_pinned(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        pinned: bool,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::Result<()>> + Send + '_>> {
        let tx = self.commands_tx.clone();
        Box::pin(async move {
            let _ = tx.send(AppCommand::SetMessagePinned { channel_id, message_id, pinned }).await;
            Ok(())
        })
    }

    fn search_messages(
        &self,
        channel_id: Id<ChannelMarker>,
        query: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::Result<()>> + Send + '_>> {
        let search_query = crate::discord::MessageSearchQuery {
            guild_id: None,
            channel_id: Some(channel_id),
            author_id: None,
            mentions_user_id: None,
            content: Some(query.to_string()),
            has: Vec::new(),
            date: None,
            author_type: Vec::new(),
            pinned: None,
            offset: 0,
        };
        let tx = self.commands_tx.clone();
        Box::pin(async move {
            let _ = tx.send(AppCommand::SearchMessages { request_id: 0, query: search_query }).await;
            Ok(())
        })
    }

    fn load_pinned_messages(&self, channel_id: Id<ChannelMarker>) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::Result<Vec<crate::discord::MessageInfo>>> + Send + '_>> {
        let tx = self.commands_tx.clone();
        Box::pin(async move {
            let _ = tx.send(AppCommand::LoadPinnedMessages { channel_id }).await;
            Ok(Vec::new())
        })
    }
}

/// JWT payload claims we care about for auth resolution.
#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct JwtClaims {
    sub: String,  // public.users.id (UUID)
    exp: u64,
}

/// Decode a base64url segment from a JWT (no padding).
fn b64url_decode(s: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s)
        .map_err(|e| e.to_string())
}

/// Validate the Supabase access JWT by checking its expiry and extracting
/// the `sub` claim (the auth user's UUID). Full RS256 signature verification
/// is deferred — Postgres RLS enforces authorization on every query, so an
/// invalid token simply yields no rows.
fn extract_jwt_sub(tokens: &SessionTokens) -> Result<String, String> {
    let parts: Vec<&str> = tokens.access_token.split('.').collect();
    if parts.len() != 3 {
        return Err("malformed JWT: expected 3 segments".into());
    }

    let claims_json = b64url_decode(parts[1])?;
    let claims: JwtClaims = serde_json::from_slice(&claims_json)
        .map_err(|e| format!("JWT claims parse error: {e}"))?;

    if tokens.is_expired() {
        return Err("JWT expired".into());
    }

    Ok(claims.sub)
}

/// Fetch the user's Discord snowflake ID via REST API using their JWT.
/// Phase 1: validate JWT + look up user row.
async fn fetch_user_id(client: &SupabaseRestClient, tokens: &SessionTokens) -> Option<Id<UserMarker>> {
    let auth_user_id = extract_jwt_sub(tokens).ok()?;
    let bearer = tokens.bearer();

    // REST: GET /users?auth_user_id=eq.<jwt_sub>
    // PostgREST uses the auth_user_id column which stores the UUID string.
    // The RLS policy on users ensures only the authenticated user's row is returned.
    let query = format!("select=discord_id&auth_user_id=eq.{}", auth_user_id);
    let rows = client
        .select(bearer, "users", &query)
        .await
        .ok()?;

    let row = rows.into_iter().next()?;
    let discord_id_val: i64 = get_column(&row, "discord_id")
        .as_i64()
        .unwrap_or(0);
    Some(Id::new(discord_id_val as u64))
}

/// Load the initial snapshot via REST API and emit Ready + incremental events.
///
/// This replaces the Discord READY payload with an equivalent set of AppEvents
/// that the existing DiscordState::apply_event can consume.
async fn build_ready_events(
    client: &SupabaseRestClient,
    user_id: Id<UserMarker>,
    tokens: &SessionTokens,
) -> Vec<AppEvent> {
    let mut events = Vec::new();
    let bearer = tokens.bearer();

    // 1. Emit Ready — the TUI uses this to swap from the loading screen
    events.push(AppEvent::Ready {
        user: tokens.access_token.clone(),
        user_id: Some(user_id),
    });

    // 2. Load guilds the user belongs to via REST.
    //    We use PostgREST's embedded resource feature to join guild_memberships
    //    and fetch only the authenticated user's guilds.
    //    RLS on guild_memberships ensures we only see rows the user is a member of.
    let guild_query = "select=guild_id,name,member_count,features,channels!inner(channel_id,name,kind,position)";
    let guild_rows = client
        .select(bearer, "guilds", guild_query)
        .await
        .unwrap_or_default();

    for row in guild_rows {
        let guild_id_val: i64 = get_column(&row, "guild_id").as_i64().unwrap_or(0);
        let guild_id = Id::<crate::discord::marker::GuildMarker>::new(guild_id_val as u64);
        let name: String = get_column(&row, "name")
            .as_str()
            .unwrap_or("")
            .to_string();
        let member_count: Option<u64> = get_column(&row, "member_count").as_i64().map(|c| c as u64);
        let features: Option<Vec<String>> = get_column(&row, "features")
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            });

        // Extract channels from the embedded resource
        let channels = get_column(&row, "channels")
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_object().cloned())
                    .map(|ch| {
                        let channel_id_val: i64 =
                            get_column(&ch, "channel_id").as_i64().unwrap_or(0);
                        let ch_name: String =
                            get_column(&ch, "name").as_str().unwrap_or("").to_string();
                        let kind: String =
                            get_column(&ch, "kind").as_str().unwrap_or("").to_string();
                        let position: Option<i32> =
                            get_column(&ch, "position").as_i64().map(|p| p as i32);
                        ChannelInfo {
                            guild_id: Some(guild_id),
                            channel_id: Id::new(channel_id_val as u64),
                            parent_id: None,
                            owner_id: None,
                            position,
                            last_message_id: None,
                            name: ch_name,
                            kind,
                            message_count: None,
                            member_count: None,
                            user_limit: None,
                            total_message_sent: None,
                            thread_metadata: None,
                            flags: None,
                            rate_limit_per_user: None,
                            available_tags: Vec::new(),
                            applied_tags: Vec::new(),
                            recipients: None,
                            permission_overwrites: Vec::new(),
                            is_message_request: None,
                            is_spam: None,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let guild_create = AppEvent::GuildCreate {
            guild_id,
            name,
            member_count,
            owner_id: None, // Phase 2: resolve from guilds.owner_id
            boost_tier: GuildBoostTier::None,
            boost_count: 0,
            verification_level: None,
            mfa_level: None,
            features,
            onboarding: None,
            channels,
            thread_snapshot_complete: false,
            current_user_thread_members: Vec::new(),
            members: Vec::new(), // Phase 2: load guild_memberships
            presences: Vec::new(),
            roles: None,
            emojis: Vec::new(),
        };

        events.push(guild_create);
    }

    events
}

/// Subscribe to Supabase Realtime for incremental updates.
///
/// - Guild text channels: `postgres_changes` on `messages` (RLS applies)
/// - DM channels: Broadcast on private channels (RLS-safe)
/// - Presences: `postgres_changes` on `presences`
///
/// See docs/plan-supabase-backend.md §5 (RLS vs postgres_changes) for the security rationale.
async fn subscribe_realtime(
    client: &SupabaseRestClient,
    supabase_url: &str,
    tokens: &SessionTokens,
    publisher: &AppEventPublisher,
    _user_id: Id<UserMarker>,
) {
    let realtime = super::realtime::RealtimeClient::new(
        supabase_url,
        tokens.clone(),
        publisher.clone(),
    );

    let bearer = tokens.bearer();

    // Load the user's channels via REST. RLS on channels ensures we only
    // see rows the user can access. We use an embedded join to get the
    // guild's Discord snowflake ID (bigint) rather than the internal UUID PK.
    let channel_query = "select=channel_id,guild_id!inner(guild_id),kind";
    let channel_rows = client
        .select(bearer, "channels", channel_query)
        .await
        .unwrap_or_default();

    for row in channel_rows {
        let channel_id: i64 = get_column(&row, "channel_id").as_i64().unwrap_or(0);
        let kind: String = get_column(&row, "kind").as_str().unwrap_or("").to_string();
        let ch_id = Id::<ChannelMarker>::new(channel_id as u64);

        // Check if this is a guild channel (has a guild_id) or a DM channel
        if kind == "guild_text" {
            if let Some(guild_obj) = get_column(&row, "guild_id").as_object() {
                let guild_snowflake: i64 =
                    get_column(guild_obj, "guild_id").as_i64().unwrap_or(0);
                if guild_snowflake > 0 {
                    realtime.subscribe_guild_messages(
                        Id::<GuildMarker>::new(guild_snowflake as u64),
                        ch_id,
                    );
                }
            }
        } else if kind == "dm" {
            realtime.subscribe_dm(ch_id);
        }
    }

    // Spawn the Realtime client task
    let _ = realtime.run().await;
}

