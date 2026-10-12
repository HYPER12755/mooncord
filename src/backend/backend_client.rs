//! Backend-agnostic client trait.
//!
//! Both Discord client and `SupabaseClient` implement this trait so the TUI
//! can be written against an abstract interface. Discord-specific operations
//! that have no Supabase equivalent default to no-ops or return `None`.

use std::time::Instant;

use tokio::sync::{mpsc, watch};

use crate::{
    discord::{
        AppEvent, AppCommand, DiscordSnapshot, SequencedAppEvent, SnapshotRevision,
        GuildMemberSearchSurface,
        gateway::GatewayCommand,
        ids::{Id, marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker}},
    },
    backend::AuthenticatedUser,
};

/// Trait abstracting the client methods the TUI and command loop need.
#[allow(clippy::too_many_methods)]
pub(crate) trait BackendClient: Send + Sync {
    // ---- core lifecycle ----

    /// Take all currently-buffered effect events.
    fn take_effects(&self) -> mpsc::Receiver<SequencedAppEvent>;

    /// Subscribe to snapshot updates.
    fn subscribe_snapshots(&self) -> watch::Receiver<SnapshotRevision>;

    /// Current in-memory snapshot (Discord only; Supabase returns default).
    fn current_discord_snapshot(&self) -> DiscordSnapshot;

    /// The current user's ID, if available.
    fn current_user_id(&self) -> Option<Id<UserMarker>>;

    /// The current user's RPC identity (bot token + application ID).
    fn current_user_rpc_identity(&self) -> Option<(String, String)> {
        None
    }

    /// Publish an application event.
    fn publish_event(&self, event: AppEvent) -> std::pin::Pin<Box<dyn std::future::Future<Output=()> + Send + '_>>;

    /// Start the backend (gateway for Discord, poller for Supabase).
    fn start(&self) -> tokio::task::JoinHandle<()>;

    /// Shut down all backend resources.
    fn shutdown(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output=()> + Send + '_>> {
        Box::pin(async {})
    }

    // ---- member search deadline management ----

    fn set_guild_member_search_target(
        &self,
        _surface: GuildMemberSearchSurface,
        _guild_id: Option<Id<GuildMarker>>,
        _query: Option<&str>,
        _now: Instant,
    ) {
    }

    fn next_due_guild_member_search(
        &self,
        _surface: GuildMemberSearchSurface,
        _now: Instant,
    ) -> Option<(Id<GuildMarker>, String)> {
        None
    }

    fn guild_member_search_deadline(
        &self,
        _surface: GuildMemberSearchSurface,
    ) -> Option<Instant> {
        None
    }

    // ---- member list subscription ----

    fn set_member_list_subscription_target(
        &self,
        _target: Option<(
            Id<GuildMarker>,
            Id<ChannelMarker>,
            Option<Id<ChannelMarker>>,
            u32,
            u64,
            Vec<(u32, u32)>,
        )>,
        _now: Instant,
    ) {
    }

    fn next_due_member_list_subscription(
        &self,
        _now: Instant,
    ) -> Option<(Id<GuildMarker>, Id<ChannelMarker>, Option<Id<ChannelMarker>>, Vec<(u32, u32)>)> {
        None
    }

    fn member_list_subscription_deadline(&self) -> Option<Instant> {
        None
    }

    // ---- read-state deadline management ----

    fn next_read_ack_deadline(&self) -> Option<Instant> {
        None
    }

    fn due_read_ack_commands(&self, _now: Instant) -> Vec<AppCommand> {
        Vec::new()
    }

    fn mark_read_state_sync_failed(&self, _channel_id: Id<ChannelMarker>) {}

    // ---- presence sync ----

    fn next_presence_sync(&self) -> Option<Instant> {
        None
    }

    fn mark_presence_sync_failed(&self) {}

    // ---- thread preview tracking ----

    fn next_thread_preview_requests(
        &self,
        _missing: Vec<(Id<ChannelMarker>, Id<MessageMarker>)>,
    ) -> Vec<(Id<ChannelMarker>, Id<MessageMarker>)> {
        Vec::new()
    }

    fn missing_thread_preview_load_requests(&self) -> Vec<(Id<ChannelMarker>, Id<MessageMarker>)> {
        Vec::new()
    }

    fn remove_thread_preview_request(&self, _channel_id: Id<ChannelMarker>, _message_id: Id<MessageMarker>) {}

    fn mark_thread_preview_request_failed(
        &self,
        _channel_id: Id<ChannelMarker>,
        _message_id: Id<MessageMarker>,
    ) {
    }

    // ---- pinned / forum / archived deadline tracking ----

    fn next_pinned_message_request(&self, _channel_id: Option<Id<ChannelMarker>>) -> Option<Id<ChannelMarker>> {
        None
    }

    fn mark_pinned_message_request_failed(&self, _channel_id: Id<ChannelMarker>) {}

    fn next_forum_post_data_request(
        &self,
        _target: Option<crate::discord::ForumPostDataRequestTarget>,
    ) -> Option<crate::discord::ForumPostDataRequestTarget> {
        None
    }

    fn mark_forum_post_data_request_failed(
        &self,
        _channel_id: Id<ChannelMarker>,
        _thread_ids: &[Id<ChannelMarker>],
    ) {
    }

    fn next_archived_thread_request(
        &self,
        _target: Option<crate::discord::ArchivedThreadRequestTarget>,
    ) -> Option<crate::discord::ArchivedThreadRequestTarget> {
        None
    }

    fn mark_archived_thread_request_send_failed(
        &self,
        _channel_id: Id<ChannelMarker>,
        _cursor: &crate::discord::ArchivedThreadPageCursor,
    ) {
    }

    // ---- member hydration ----

    fn next_member_hydration_requests(
        &self,
        _missing: Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)>,
        _now: Instant,
    ) -> Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)> {
        Vec::new()
    }

    // ---- message history ----

    fn next_message_history_request(
        &self,
        _channel_id: Option<Id<ChannelMarker>>,
        _force_reload: bool,
    ) -> Option<Id<ChannelMarker>> {
        None
    }

    fn mark_message_history_request_failed(&self, _channel_id: Id<ChannelMarker>) {}

    // ---- typing indicator ----

    fn next_due_typing_indicator(&self, _now: Instant) -> Option<(Id<ChannelMarker>, Id<GuildMarker>, bool)> {
        None
    }

    fn mark_typing_indicator_failed(&self) {}

    fn set_typing_indicator(&self, _channel_id: Id<ChannelMarker>, _guild_id: Id<GuildMarker>) {}

    // ---- push-to-talk ----

    fn set_push_to_talk_enabled(&self, _enabled: bool) {}

    fn set_push_to_talk_pressed(&self, _pressed: bool) {}

    // ---- guild/channel subscription ----

    fn request_guild_members_by_ids(&self, _guild_id: Id<GuildMarker>, _user_ids: &[Id<UserMarker>]) {}

    fn search_guild_members(&self, _guild_id: Id<GuildMarker>, _query: &str) {}

    fn subscribe_direct_message(&self, _channel_id: Id<ChannelMarker>) {}

    fn subscribe_guild_channel(&self, _guild_id: Id<GuildMarker>, _channel_id: Id<ChannelMarker>) {}

    // ---- token authentication ----

    fn validate_token_authentication(&self, _token: &str) -> crate::Result<AuthenticatedUser> {
        Err(crate::error::AppError::NotLoggedIn)
    }

    // ---- guild/channel subscription ----

    fn update_rest_page_referer(&self, _channel_id: Option<Id<ChannelMarker>>) {}

    fn current_user_activities(&self) -> Vec<crate::discord::ActivityInfo> {
        Vec::new()
    }

    fn update_user_profile(&self, _update: &crate::discord::UserProfileUpdate) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn load_user_profile(&self, _user_id: Id<UserMarker>) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<crate::discord::UserProfileInfo>> + Send + '_>> {
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn load_user_note(&self, _user_id: Id<UserMarker>) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<Option<String>>> + Send + '_>> {
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn next_user_note_request(&self, _now: Instant) -> Vec<Id<UserMarker>> {
        Vec::new()
    }

    fn next_user_profile_request(&self, _now: Instant) -> Vec<(Id<UserMarker>, Instant)> {
        Vec::new()
    }

    fn mark_user_note_request_failed(&self, _user_id: Id<UserMarker>) {}

    // ---- message operations ----

    fn ack_channel(&self, _channel_id: Id<ChannelMarker>) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn ack_channels(&self, _channel_ids: &[Id<ChannelMarker>]) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn clear_read_ack(&self, _channel_id: Id<ChannelMarker>) {}

    fn clear_read_acks(&self, _channel_id: Id<ChannelMarker>) {}

    fn publish_optimistic_read_acks(&self, _channel_id: Id<ChannelMarker>, _last_read_message_id: Id<MessageMarker>) {}

    fn schedule_read_ack(
        &self,
        _channel_id: Id<ChannelMarker>,
        _last_read_message_id: Id<MessageMarker>,
    ) -> Option<Instant> {
        None
    }

    fn delete_recent_mention(&self, _message_id: Id<MessageMarker>) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    // ---- message send/edit ----

    fn send_message(
        &self,
        channel_id: Id<ChannelMarker>,
        nonce: Id<MessageMarker>,
        content: &str,
        reply_to: Option<&crate::discord::ReplyReference>,
        attachments: &[crate::discord::MessageAttachmentUpload],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<crate::discord::MessageInfo>> + Send + '_>> {
        let _ = (channel_id, nonce, content, reply_to, attachments);
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn send_tts_message(
        &self,
        channel_id: Id<ChannelMarker>,
        nonce: Id<MessageMarker>,
        content: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<crate::discord::MessageInfo>> + Send + '_>> {
        let _ = (channel_id, nonce, content);
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn edit_message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        edit: &crate::discord::MessageEditRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<crate::discord::MessageInfo>> + Send + '_>> {
        let _ = (channel_id, message_id, edit);
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn remove_message_embeds(&self, _channel_id: Id<ChannelMarker>, _message_id: Id<MessageMarker>) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn delete_message(&self, _channel_id: Id<ChannelMarker>, _message_id: Id<MessageMarker>) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn leave_guild(&self, _guild_id: Id<GuildMarker>) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn trigger_typing(&self, _channel_id: Id<ChannelMarker>) -> crate::Result<()> {
        Err(crate::error::AppError::NotImplemented)
    }

    fn create_forum_post(&self, _post: &crate::discord::ForumPostCreate) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<crate::discord::CreatedForumPost>> + Send + '_>> {
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn message_slow_mode(&self, _channel_id: Id<ChannelMarker>) -> Option<std::time::Duration> {
        None
    }

    fn load_pinned_messages(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<Vec<crate::discord::MessageInfo>>> + Send + '_>> {
        let _ = channel_id;
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn set_message_pinned(&self, _channel_id: Id<ChannelMarker>, _message_id: Id<MessageMarker>, _pinned: bool) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn vote_poll(&self, _channel_id: Id<ChannelMarker>, _message_id: Id<MessageMarker>, _answer_index: std::num::NonZero<u8>, _publish: bool) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    // ---- reactions ----

    fn add_reaction(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &crate::discord::ReactionEmoji,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        let _ = (channel_id, message_id, emoji);
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    // ---- thread management ----

    fn set_thread_archived(
        &self,
        channel_id: Id<ChannelMarker>,
        archived: bool,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        let _ = (channel_id, archived);
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn set_thread_locked(
        &self,
        channel_id: Id<ChannelMarker>,
        locked: bool,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        let _ = (channel_id, locked);
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn delete_thread(&self, _thread_id: Id<ChannelMarker>) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn follow_thread(&self, _thread_id: Id<ChannelMarker>) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn unfollow_thread(&self, _thread_id: Id<ChannelMarker>) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    // ---- message history ----

    fn begin_message_history_after_request(
        &self,
        channel_id: Id<ChannelMarker>,
        last_seen_message_id: Id<MessageMarker>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        let _ = (channel_id, last_seen_message_id);
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn begin_older_message_history_request(
        &self,
        channel_id: Id<ChannelMarker>,
        oldest_message_id: Id<MessageMarker>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        let _ = (channel_id, oldest_message_id);
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn search_messages(
        &self,
        channel_id: Id<ChannelMarker>,
        query: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        let _ = (channel_id, query);
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn load_recent_mentions(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    // ---- application commands ----

    fn load_application_commands(
        &self,
        guild_id: Option<Id<GuildMarker>>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        let _ = guild_id;
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn run_application_command(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        application_command: &crate::discord::ApplicationCommandInvocation,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        let _ = (guild_id, application_command);
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn request_application_command_autocomplete(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        application_command: &crate::discord::ApplicationCommandAutocompleteInvocation,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        let _ = (guild_id, application_command);
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn application_display_name(
        &self,
        application_id: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output=Option<String>> + Send + '_>> {
        let _ = application_id;
        Box::pin(async { None })
    }

    // ---- presence ----

    fn update_presence_activity(
        &self,
        _activity: crate::discord::ActivityInfo,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output=()> + Send + '_>> {
        Box::pin(async {})
    }

    fn update_presence_status(&self, _status: crate::discord::PresenceStatus) -> std::pin::Pin<Box<dyn std::future::Future<Output=()> + Send + '_>> {
        Box::pin(async {})
    }

    fn rich_presence_dirty(&self) -> Option<std::sync::Arc<tokio::sync::Notify>> {
        None
    }

    fn rich_presence_selection(&self) -> Option<crate::discord::RichPresenceSelection> {
        None
    }

    fn set_rich_presence_selection(&self, _selection: crate::discord::RichPresenceSelection) {}

    fn notify_rich_presence_dirty(&self) {}

    // ---- voice ----

    fn replace_voice_participant_playback_settings(&self, _user_id: Id<UserMarker>) {}

    fn update_voice_participant_playback_settings(
        &self,
        _user_id: Id<UserMarker>,
        _settings: &crate::discord::VoiceParticipantPlaybackSettings,
    ) {
    }

    fn update_voice_state(&self, _voice_state: crate::discord::VoiceStateInfo) {}

    fn request_voice_join(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        mute: bool,
        deaf: bool,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        let _ = (guild_id, channel_id, mute, deaf);
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn request_stream_watch(&self, _guild_id: Id<GuildMarker>) {}

    fn request_stream_start(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn request_stream_stop(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output=crate::Result<()>> + Send + '_>> {
        Box::pin(async { Err(crate::error::AppError::NotImplemented) })
    }

    fn update_voice_capture_permission(&self, _allowed: bool) {}

    fn update_voice_audio_sources(&self, _sources: crate::discord::VoiceAudioSources) {}

    // ---- gateway commands (test only) ----

    fn take_gateway_commands_for_test(&self) -> mpsc::UnboundedReceiver<GatewayCommand> {
        unimplemented!()
    }
}

// Blanket impls for common reference types
impl<T: BackendClient + ?Sized> BackendClient for &T {
    fn take_effects(&self) -> mpsc::Receiver<SequencedAppEvent> { (**self).take_effects() }
    fn subscribe_snapshots(&self) -> watch::Receiver<SnapshotRevision> { (**self).subscribe_snapshots() }
    fn current_discord_snapshot(&self) -> DiscordSnapshot { (**self).current_discord_snapshot() }
    fn current_user_id(&self) -> Option<Id<UserMarker>> { (**self).current_user_id() }
    fn current_user_rpc_identity(&self) -> Option<(String, String)> { (**self).current_user_rpc_identity() }
    fn publish_event(&self, event: AppEvent) -> std::pin::Pin<Box<dyn std::future::Future<Output=()> + Send + '_>> { (**self).publish_event(event) }
    fn start(&self) -> tokio::task::JoinHandle<()> { (**self).start() }
    fn shutdown(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output=()> + Send + '_>> { (**self).shutdown() }
    fn set_guild_member_search_target(&self, surface: GuildMemberSearchSurface, guild_id: Option<Id<GuildMarker>>, query: Option<&str>, now: Instant) { (**self).set_guild_member_search_target(surface, guild_id, query, now) }
    fn next_due_guild_member_search(&self, surface: GuildMemberSearchSurface, now: Instant) -> Option<(Id<GuildMarker>, String)> { (**self).next_due_guild_member_search(surface, now) }
    fn guild_member_search_deadline(&self, surface: GuildMemberSearchSurface) -> Option<Instant> { (**self).guild_member_search_deadline(surface) }
    fn set_member_list_subscription_target(&self, target: Option<(Id<GuildMarker>, Id<ChannelMarker>, Option<Id<ChannelMarker>>, u32, u64, Vec<(u32, u32)>)>, now: Instant) { (**self).set_member_list_subscription_target(target, now) }
    fn next_due_member_list_subscription(&self, now: Instant) -> Option<(Id<GuildMarker>, Id<ChannelMarker>, Option<Id<ChannelMarker>>, Vec<(u32, u32)>)> { (**self).next_due_member_list_subscription(now) }
    fn member_list_subscription_deadline(&self) -> Option<Instant> { (**self).member_list_subscription_deadline() }
    fn next_read_ack_deadline(&self) -> Option<Instant> { (**self).next_read_ack_deadline() }
    fn due_read_ack_commands(&self, now: Instant) -> Vec<AppCommand> { (**self).due_read_ack_commands(now) }
    fn mark_read_state_sync_failed(&self, channel_id: Id<ChannelMarker>) { (**self).mark_read_state_sync_failed(channel_id) }
    fn next_presence_sync(&self) -> Option<Instant> { (**self).next_presence_sync() }
    fn mark_presence_sync_failed(&self) { (**self).mark_presence_sync_failed() }
    fn next_thread_preview_requests(&self, missing: Vec<(Id<ChannelMarker>, Id<MessageMarker>)>) -> Vec<(Id<ChannelMarker>, Id<MessageMarker>)> { (**self).next_thread_preview_requests(missing) }
    fn missing_thread_preview_load_requests(&self) -> Vec<(Id<ChannelMarker>, Id<MessageMarker>)> { (**self).missing_thread_preview_load_requests() }
    fn remove_thread_preview_request(&self, channel_id: Id<ChannelMarker>, message_id: Id<MessageMarker>) { (**self).remove_thread_preview_request(channel_id, message_id) }
    fn mark_thread_preview_request_failed(&self, channel_id: Id<ChannelMarker>, message_id: Id<MessageMarker>) { (**self).mark_thread_preview_request_failed(channel_id, message_id) }
    fn next_pinned_message_request(&self, channel_id: Option<Id<ChannelMarker>>) -> Option<Id<ChannelMarker>> { (**self).next_pinned_message_request(channel_id) }
    fn mark_pinned_message_request_failed(&self, channel_id: Id<ChannelMarker>) { (**self).mark_pinned_message_request_failed(channel_id) }
    fn next_forum_post_data_request(&self, target: Option<crate::discord::ForumPostDataRequestTarget>) -> Option<crate::discord::ForumPostDataRequestTarget> { (**self).next_forum_post_data_request(target) }
    fn mark_forum_post_data_request_failed(&self, channel_id: Id<ChannelMarker>, thread_ids: &[Id<ChannelMarker>]) { (**self).mark_forum_post_data_request_failed(channel_id, thread_ids) }
    fn next_archived_thread_request(&self, target: Option<crate::discord::ArchivedThreadRequestTarget>) -> Option<crate::discord::ArchivedThreadRequestTarget> { (**self).next_archived_thread_request(target) }
    fn mark_archived_thread_request_send_failed(&self, channel_id: Id<ChannelMarker>, cursor: &crate::discord::ArchivedThreadPageCursor) { (**self).mark_archived_thread_request_send_failed(channel_id, cursor) }
    fn next_member_hydration_requests(&self, missing: Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)>, now: Instant) -> Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)> { (**self).next_member_hydration_requests(missing, now) }
    fn next_message_history_request(&self, channel_id: Option<Id<ChannelMarker>>, force_reload: bool) -> Option<Id<ChannelMarker>> { (**self).next_message_history_request(channel_id, force_reload) }
    fn mark_message_history_request_failed(&self, channel_id: Id<ChannelMarker>) { (**self).mark_message_history_request_failed(channel_id) }
    fn next_due_typing_indicator(&self, now: Instant) -> Option<(Id<ChannelMarker>, Id<GuildMarker>, bool)> { (**self).next_due_typing_indicator(now) }
    fn mark_typing_indicator_failed(&self) { (**self).mark_typing_indicator_failed() }
    fn set_typing_indicator(&self, channel_id: Id<ChannelMarker>, guild_id: Id<GuildMarker>) { (**self).set_typing_indicator(channel_id, guild_id) }
    fn set_push_to_talk_enabled(&self, enabled: bool) { (**self).set_push_to_talk_enabled(enabled) }
    fn set_push_to_talk_pressed(&self, pressed: bool) { (**self).set_push_to_talk_pressed(pressed) }
    fn request_guild_members_by_ids(&self, guild_id: Id<GuildMarker>, user_ids: &[Id<UserMarker>]) { (**self).request_guild_members_by_ids(guild_id, user_ids) }
    fn search_guild_members(&self, guild_id: Id<GuildMarker>, query: &str) { (**self).search_guild_members(guild_id, query) }
    fn subscribe_direct_message(&self, channel_id: Id<ChannelMarker>) { (**self).subscribe_direct_message(channel_id) }
    fn subscribe_guild_channel(&self, guild_id: Id<GuildMarker>, channel_id: Id<ChannelMarker>) { (**self).subscribe_guild_channel(guild_id, channel_id) }
    fn validate_token_authentication(&self, token: &str) -> crate::Result<AuthenticatedUser> { (**self).validate_token_authentication(token) }
    fn take_gateway_commands_for_test(&self) -> mpsc::UnboundedReceiver<GatewayCommand> { (**self).take_gateway_commands_for_test() }
}
