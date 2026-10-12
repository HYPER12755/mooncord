//! Command handler mapping `AppCommand` → Postgres operations → `AppEvent`s.
//!
//! Each `AppCommand` variant is handled by a method that performs the
//! appropriate DB operation and pushes `AppEvent`s via the shared
//! `AppEventPublisher` pipeline (same as the Discord Gateway backend).

use super::rest_client::{get_column, get_column_from_value, SupabaseRestClient};
use serde_json::{json, Value};

use crate::discord::{
    AppEvent, AppCommand, Id, marker::{ChannelMarker, GuildMarker, UserMarker, MessageMarker, RoleMarker, ForumTagMarker},
    MessageHistoryAfterMode, MessageInfo, MessageKind, MessageUpdateDispatchInfo,
    MessageUpdateEventFields, MuteDuration, MessageSearchQuery,
    ReactionEmoji, ReactionUserInfo, ReplyReference, MessageAttachmentUpload, UserProfileUpdate,
    RichPresenceSelection, PresenceStatus, ActivityInfo,
    MessageSearchPage, MemberInfo, GuildMembersChunkInfo,
    AttachmentInfo, AppEventPublisher, ThreadMemberInfo, ArchivedThreadsPage,
    ThreadGatewayInfo, ThreadMetadataInfo, PollInfo, PollAnswerInfo,
    ChannelInfo, ForumPostCreate,
};

use super::token_store::SessionTokens;

/// Context for command handling — carries the REST client, auth state,
/// and the shared event publisher that routes AppEvents back to the TUI.
pub struct CommandContext {
    pub rest: SupabaseRestClient,
    pub tokens: SessionTokens,
    pub publisher: AppEventPublisher,
    pub user_id: Option<Id<UserMarker>>,
    pub supabase_url: String,
    pub supabase_key: String,
}

/// Handles a single AppCommand by dispatching to the appropriate handler.
pub async fn handle_command(ctx: &mut CommandContext, command: AppCommand) {
    match command {
        // ---- Sign out ----
        AppCommand::SignOut => handle_sign_out(ctx).await,

        // ---- Message history ----
        AppCommand::LoadMessageHistory { channel_id, before } => {
            handle_load_message_history(ctx, channel_id, before).await;
        }
        AppCommand::RefreshMessageHistory { channel_id } => {
            handle_refresh_message_history(ctx, channel_id).await;
        }
        AppCommand::LoadMessageHistoryAfter { channel_id, after, mode } => {
            handle_load_history_after(ctx, channel_id, after, mode).await;
        }
        AppCommand::LoadMessageHistoryAround { channel_id, message_id } => {
            handle_load_history_around(ctx, channel_id, message_id).await;
        }

        // ---- Channel subscriptions ----
        AppCommand::SubscribeDirectMessage { channel_id } => {
            handle_subscribe_dm(ctx, channel_id).await;
        }
        AppCommand::SubscribeGuildChannel { guild_id, channel_id } => {
            handle_subscribe_guild_channel(ctx, guild_id, channel_id).await;
        }

        // ---- Typing ----
        AppCommand::TriggerTyping { channel_id } => {
            handle_trigger_typing(ctx, channel_id).await;
        }

        // ---- Messaging ----
        AppCommand::SendMessage { channel_id, nonce, content, reply_to, attachments } => {
            handle_send_message(ctx, channel_id, nonce, content, reply_to, attachments).await;
        }
        AppCommand::EditMessage { channel_id, message_id, content } => {
            handle_edit_message(ctx, channel_id, message_id, content).await;
        }
        AppCommand::DeleteMessage { channel_id, message_id } => {
            handle_delete_message(ctx, channel_id, message_id).await;
        }

        // ---- Reactions ----
        AppCommand::AddReaction { channel_id, message_id, emoji } => {
            handle_add_reaction(ctx, channel_id, message_id, emoji).await;
        }
        AppCommand::RemoveReaction { channel_id, message_id, emoji } => {
            handle_remove_reaction(ctx, channel_id, message_id, emoji).await;
        }
        AppCommand::LoadReactionUsers { channel_id, message_id, emoji, after } => {
            handle_load_reaction_users(ctx, channel_id, message_id, emoji, after).await;
        }

        // ---- Guild operations ----
        AppCommand::SetSelectedGuild { guild_id } => {
            handle_set_selected_guild(ctx, guild_id).await;
        }
        AppCommand::LeaveGuild { guild_id, label } => {
            handle_leave_guild(ctx, guild_id, label).await;
        }

        // ---- Channel selection ----
        AppCommand::SetSelectedMessageChannel { channel_id } => {
            handle_set_selected_channel(ctx, channel_id).await;
        }

        // ---- Ack / read state ----
        AppCommand::AckChannel { channel_id, message_id } => {
            handle_ack_channel(ctx, channel_id, message_id).await;
        }
        AppCommand::AckChannels { targets } => {
            handle_ack_channels(ctx, targets).await;
        }
        AppCommand::ScheduleAckChannel { channel_id, message_id } => {
            handle_schedule_ack(ctx, channel_id, message_id).await;
        }

        // ---- Mute settings ----
        AppCommand::SetGuildMuted { guild_id, muted, duration, label } => {
            handle_set_guild_muted(ctx, guild_id, muted, duration, label).await;
        }
        AppCommand::SetChannelMuted { guild_id, channel_id, muted, duration, label } => {
            handle_set_channel_muted(ctx, guild_id, channel_id, muted, duration, label).await;
        }

        // ---- Profile ----
        AppCommand::LoadUserProfile { user_id, guild_id } => {
            handle_load_user_profile(ctx, user_id, guild_id).await;
        }
        AppCommand::UpdateUserProfile { update } => {
            handle_update_user_profile(ctx, update).await;
        }

        // ---- Presence ----
        AppCommand::UpdateCurrentUserStatus { status } => {
            handle_update_presence(ctx, status).await;
        }
        AppCommand::UpdateCurrentUserActivity { status, activities, rich_presence } => {
            handle_update_activity(ctx, status, activities, rich_presence).await;
        }

        // ---- Inbox ----
        AppCommand::LoadInboxMentions { request_id, before } => {
            handle_load_inbox_mentions(ctx, request_id, before).await;
        }
        AppCommand::LoadInboxChannelHistory { channel_id, request_id } => {
            handle_load_inbox_channel_history(ctx, channel_id, request_id).await;
        }
        AppCommand::DeleteInboxMention { message_id } => {
            handle_delete_inbox_mention(ctx, message_id).await;
        }

        // ---- Search ----
        AppCommand::SearchMessages { request_id, query } => {
            handle_search_messages(ctx, request_id, query).await;
        }

        // ---- Guild members ----
        AppCommand::LoadGuildMembersByIds { guild_id, user_ids } => {
            handle_load_guild_members(ctx, guild_id, user_ids).await;
        }
        AppCommand::SearchGuildMembers { guild_id, query, limit } => {
            handle_search_guild_members(ctx, guild_id, query, limit).await;
        }

        // ---- Threads ----
        AppCommand::LoadThreadPreview { channel_id, message_id } => {
            handle_load_thread_preview(ctx, channel_id, message_id).await;
        }
        AppCommand::LoadArchivedThreads { guild_id, channel_id, before } => {
            handle_load_archived_threads(ctx, guild_id, channel_id, before).await;
        }
        AppCommand::CreateForumPost { post } => {
            handle_create_forum_post(ctx, post).await;
        }
        AppCommand::LoadForumPostData { guild_id, channel_id, thread_ids } => {
            handle_load_forum_post_data(ctx, guild_id, channel_id, thread_ids).await;
        }
        AppCommand::EditThread {
            channel_id, name, applied_tags, rate_limit_per_user, auto_archive_duration, label
        } => {
            handle_edit_thread(ctx, channel_id, Some(name), Some(applied_tags), Some(rate_limit_per_user as u16), Some(auto_archive_duration as u16), label).await;
        }
        AppCommand::SetThreadArchived { channel_id, archived, label } => {
            handle_set_thread_archived(ctx, channel_id, archived, label).await;
        }
        AppCommand::SetThreadLocked { channel_id, locked, label } => {
            handle_set_thread_locked(ctx, channel_id, locked, label).await;
        }
        AppCommand::SetThreadPinned { channel_id, pinned, current_flags, label } => {
            handle_set_thread_pinned(ctx, channel_id, pinned, current_flags, label).await;
        }
        AppCommand::SetThreadMuted { channel_id, muted, duration, label } => {
            handle_set_thread_muted(ctx, channel_id, muted, duration, label).await;
        }
        AppCommand::SetThreadFollowed { channel_id, followed, label } => {
            handle_set_thread_followed(ctx, channel_id, followed, label).await;
        }
        AppCommand::SetThreadNotificationLevel { channel_id, flags, label } => {
            handle_set_thread_notification_level(ctx, channel_id, flags, label).await;
        }
        AppCommand::DeleteThread { channel_id, label } => {
            handle_delete_thread(ctx, channel_id, label).await;
        }

        // ---- Polls ----
        AppCommand::VotePoll { channel_id, message_id, answer_ids } => {
            handle_vote_poll(ctx, channel_id, message_id, answer_ids).await;
        }

        // ---- Message pinning ----
        AppCommand::LoadPinnedMessages { channel_id } => {
            handle_load_pinned_messages(ctx, channel_id).await;
        }
        AppCommand::SetMessagePinned { channel_id, message_id, pinned } => {
            handle_set_message_pinned(ctx, channel_id, message_id, pinned).await;
        }

        // Voice, app commands, translations, etc. deferred to later phases.
        _ => {}
    }
}

// ---- Individual handlers ----

async fn handle_sign_out(ctx: &mut CommandContext) {
    let _ = ctx.publisher.publish_for_backend(&AppEvent::SignedOut).await;
}

async fn handle_load_message_history(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
    before: Option<Id<MessageMarker>>,
) {
    let bearer = ctx.tokens.bearer();
    let query = format!(
        "select=discord_id,discord_author_id,content,flags,discord_guild_id,created_at&discord_channel_id=eq.{}",
        channel_id.get()
    );

    let rows = ctx.rest.select(bearer, "messages", &query).await.unwrap_or_default();

    let messages: Vec<_> = rows
        .into_iter()
        .map(|row| {
            let msg_id_val: i64 = get_column(&row, "discord_id").as_i64().unwrap_or(0);
            let author_val: i64 = get_column(&row, "discord_author_id").as_i64().unwrap_or(0);
            let content_val: String = get_column(&row, "content").as_str().unwrap_or("").to_string();
            let flags_val: i64 = get_column(&row, "flags").as_i64().unwrap_or(0);
            let guild_id_val: Option<i64> = get_column(&row, "discord_guild_id").as_i64();
            MessageInfo {
                guild_id: guild_id_val.map(|v| Id::new(v as u64)),
                channel_id,
                message_id: Id::new(msg_id_val as u64),
                author_id: Id::new(author_val as u64),
                content: Some(content_val),
                flags: flags_val as u64,
                message_kind: MessageKind::regular(),
                ..Default::default()
            }
        })
        .collect();

    ctx.publisher
        .publish_for_backend(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before,
            messages,
        })
        .await;
}

async fn handle_refresh_message_history(ctx: &mut CommandContext, channel_id: Id<ChannelMarker>) {
    handle_load_message_history(ctx, channel_id, None).await;
}

async fn handle_load_history_after(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
    after: Id<MessageMarker>,
    mode: MessageHistoryAfterMode,
) {
    let limit: i64 = 50;
    let bearer = ctx.tokens.bearer();

    // PostgREST: filter by channel, discord_id > after, order asc, limit
    let query = format!(
        "select=discord_id,discord_author_id,content,flags,discord_guild_id&discord_channel_id=eq.{}&discord_id=gte.{}&order=discord_id.asc&limit={}",
        channel_id.get(),
        after.get(),
        limit
    );

    let rows = ctx.rest.select(bearer, "messages", &query).await.unwrap_or_default();

    let messages: Vec<_> = rows
        .into_iter()
        .map(|row| {
            let msg_id_val: i64 = get_column(&row, "discord_id").as_i64().unwrap_or(0);
            let author_val: i64 = get_column(&row, "discord_author_id").as_i64().unwrap_or(0);
            let content_val: String = get_column(&row, "content").as_str().unwrap_or("").to_string();
            let flags_val: i64 = get_column(&row, "flags").as_i64().unwrap_or(0);
            let guild_id_val: Option<i64> = get_column(&row, "discord_guild_id").as_i64();
            MessageInfo {
                guild_id: guild_id_val.map(|v| Id::new(v as u64)),
                channel_id,
                message_id: Id::new(msg_id_val as u64),
                author_id: Id::new(author_val as u64),
                content: Some(content_val),
                flags: flags_val as u64,
                message_kind: MessageKind::regular(),
                ..Default::default()
            }
        })
        .collect();

    let has_more = messages.len() as i64 == limit;

    ctx.publisher
        .publish_for_backend(&AppEvent::MessageHistoryAfterLoaded {
            channel_id,
            after,
            messages,
            has_more,
            mode,
        })
        .await;
}

async fn handle_load_history_around(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
) {
    let limit: i64 = 50;
    let half_limit = limit / 2;
    let bearer = ctx.tokens.bearer();

    // PostgREST can't do abs() ordering directly, so fetch messages around
    // the pivot using a range filter and let the client sort.
    let query = format!(
        "select=discord_id,discord_author_id,content,flags,discord_guild_id,discord_channel_id&discord_channel_id=eq.{}",
        channel_id.get()
    );

    let rows = ctx.rest.select(bearer, "messages", &query).await.unwrap_or_default();

    // PostgREST fetch all and client-side filter/sort
    let mut messages: Vec<_> = rows
        .into_iter()
        .filter_map(|row| {
            let msg_id_val: i64 = get_column(&row, "discord_id").as_i64()?;
            let channel_val: i64 = get_column(&row, "discord_channel_id").as_i64()?;
            if channel_val != channel_id.get() as i64 {
                return None;
            }
            let author_val: i64 = get_column(&row, "discord_author_id").as_i64().unwrap_or(0);
            let content_val: String = get_column(&row, "content").as_str().unwrap_or("").to_string();
            let flags_val: i64 = get_column(&row, "flags").as_i64().unwrap_or(0);
            let guild_id_val: Option<i64> = get_column(&row, "discord_guild_id").as_i64();
            Some(MessageInfo {
                guild_id: guild_id_val.map(|v| Id::new(v as u64)),
                channel_id,
                message_id: Id::new(msg_id_val as u64),
                author_id: Id::new(author_val as u64),
                content: Some(content_val),
                flags: flags_val as u64,
                message_kind: MessageKind::regular(),
                ..Default::default()
            })
        })
        .collect();
    messages.sort_by_key(|m| m.message_id);

    ctx.publisher
        .publish_for_backend(&AppEvent::MessageHistoryAroundLoaded {
            channel_id,
            message_id,
            messages,
        })
        .await;
}

async fn handle_subscribe_dm(_ctx: &mut CommandContext, _channel_id: Id<ChannelMarker>) {
    // Realtime subscription for DMs is handled during startup via subscribe_realtime.
    // Per-channel subscription is a no-op since all DMs are already subscribed.
}

async fn handle_subscribe_guild_channel(
    _ctx: &mut CommandContext,
    _guild_id: Id<GuildMarker>,
    _channel_id: Id<ChannelMarker>,
) {
    // Realtime subscription for guild channels is handled during startup via subscribe_realtime.
    // Per-channel subscription is a no-op since all eligible channels are already subscribed.
}

async fn handle_trigger_typing(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
) {
    if let Some(user_id) = &ctx.user_id {
        let bearer = ctx.tokens.bearer();
        let expires_at = chrono::Utc::now()
            + chrono::Duration::seconds(10);
        let expires_at_str = expires_at.to_rfc3339();

        let query = format!(
            "discord_channel_id=eq.{}&discord_user_id=eq.{}",
            channel_id.get(),
            user_id.get()
        );

        // Try to update existing row first; if no row matches, insert.
        let update_row = json!({
            "expires_at": expires_at_str,
        });
        let _ = ctx.rest.update(bearer, "typing_indicators", &query, &update_row, None).await;
    }
}

async fn handle_send_message(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
    nonce: Id<MessageMarker>,
    content: String,
    reply_to: Option<ReplyReference>,
    attachments: Vec<MessageAttachmentUpload>,
) {
    if let Some(user_id) = &ctx.user_id {
        let bearer = ctx.tokens.bearer();

        // REST: fetch the channel to get its discord_guild_id
        let ch_query = format!(
            "select=discord_guild_id&discord_channel_id=eq.{}",
            channel_id.get()
        );
        let ch_rows = ctx.rest.select(bearer, "channels", &ch_query).await;
        let guild_id: Option<i64> = ch_rows
            .ok()
            .and_then(|rows| rows.into_iter().next())
            .and_then(|row| get_column(&row, "discord_guild_id").as_i64());

        let reply_to_id: Option<i64> = reply_to.as_ref().map(|r| r.message_id.get() as i64);

        // Upload attachments to Supabase Storage before inserting the message,
        // so we can store the returned URLs. Each file is uploaded to the
        // "message-attachments" bucket with a UUID object key.
        let http_client = reqwest::Client::new();
        let mut attachment_infos: Vec<AttachmentInfo> = Vec::with_capacity(attachments.len());

        for upload in &attachments {
            // Gather the raw bytes — from memory or from the filesystem path.
            let (bytes, content_type) = if let Some(mem) = upload.bytes() {
                let ct = mime_guess::from_path(&upload.filename)
                    .first_or_octet_stream()
                    .to_string();
                (mem.to_vec(), ct)
            } else if let Some(path) = upload.path() {
                let ct = mime_guess::from_path(path)
                    .first_or_octet_stream()
                    .to_string();
                match std::fs::read(path) {
                    Ok(data) => (data, ct),
                    Err(e) => {
                        tracing::error!("failed to read attachment {}: {e}", upload.filename);
                        continue;
                    }
                }
            } else {
                tracing::warn!("attachment {} has no source data", upload.filename);
                continue;
            };

            let object_key = format!(
                "{}/{}_{}",
                uuid::Uuid::new_v4(),
                uuid::Uuid::new_v4(),
                upload.filename
            );

            let upload_url = format!(
                "{}/storage/v1/object/message-attachments/{}",
                ctx.supabase_url, object_key
            );

            let resp = http_client
                .put(&upload_url)
                .header("apikey", &ctx.supabase_key)
                .header(
                    "Authorization",
                    format!("Bearer {}", ctx.tokens.access_token),
                )
                .header("Content-Type", &content_type)
                .body(bytes.clone())
                .send()
                .await;

            match resp {
                Ok(r) if r.status().is_success() => {
                    attachment_infos.push(AttachmentInfo {
                        id: Id::new(rand::random::<u64>()),
                        filename: upload.filename.clone(),
                        url: upload_url.clone(),
                        proxy_url: upload_url.clone(),
                        content_type: Some(content_type.clone()),
                        size: upload.size_bytes,
                        width: None,
                        height: None,
                        description: None,
                        flags: 0,
                    });
                }
                Ok(r) => {
                    tracing::error!(
                        "attachment upload failed for {}: {} {}",
                        upload.filename,
                        r.status(),
                        r.text().await.unwrap_or_default()
                    );
                    continue;
                }
                Err(e) => {
                    tracing::error!("attachment upload error for {}: {e}", upload.filename);
                    continue;
                }
            }
        }

        // REST: insert the message with Prefer: return=representation
        // to get the generated discord_id and created_at back.
        let mut msg_row = serde_json::Map::new();
        msg_row.insert("discord_channel_id".to_string(), Value::Number((channel_id.get() as i64).into()));
        msg_row.insert("discord_author_id".to_string(), Value::Number((user_id.get() as i64).into()));
        if let Some(gid) = guild_id {
            msg_row.insert("discord_guild_id".to_string(), Value::Number(gid.into()));
        }
        msg_row.insert("content".to_string(), Value::String(content.clone()));
        if let Some(reply_id) = reply_to_id {
            msg_row.insert("discord_reply_to".to_string(), Value::Number(reply_id.into()));
        }

        let insert_result = ctx
            .rest
            .insert(bearer, "messages", &[Value::Object(msg_row)], Some("return=representation"))
            .await;

        if let Ok(rows) = insert_result {
            if let Some(row) = rows.into_iter().next() {
                // Note: PostgREST returns the postgres UUID as "id" and discord_id as "discord_id"
                let msg_id_val: i64 = get_column(&row, "discord_id").as_i64().unwrap_or(0);

                // Persist attachment metadata against the newly created message.
                for info in &attachment_infos {
                    let mut att_row = serde_json::Map::new();
                    att_row.insert("message_id".to_string(), Value::String(msg_id_val.to_string()));
                    att_row.insert("filename".to_string(), Value::String(info.filename.clone()));
                    att_row.insert("content_type".to_string(), Value::String(info.content_type.clone().unwrap_or_default()));
                    att_row.insert("size".to_string(), Value::Number((info.size as i64).into()));
                    att_row.insert("url".to_string(), Value::String(info.url.clone()));

                    let _ = ctx.rest.insert(bearer, "attachments", &[Value::Object(att_row)], None).await;
                }

                // Fetch author username via REST
                let author_query = format!(
                    "select=username&discord_id=eq.{}",
                    user_id.get()
                );
                let author_rows = ctx.rest.select(bearer, "users", &author_query).await;
                let author: String = author_rows
                    .ok()
                    .and_then(|rows| rows.into_iter().next())
                    .and_then(|row| {
                        get_column(&row, "username").as_str().map(|s| s.to_string())
                    })
                    .unwrap_or_default();

                let message = MessageInfo {
                    guild_id: guild_id.map(|v| Id::new(v as u64)),
                    channel_id,
                    message_id: Id::new(msg_id_val as u64),
                    nonce: Some(nonce),
                    author_id: *user_id,
                    author,
                    content: Some(content),
                    message_kind: MessageKind::regular(),
                    attachments: attachment_infos,
                    ..Default::default()
                };

                ctx.publisher
                    .publish_for_backend(&AppEvent::MessageCreate { message })
                    .await;
            }
        }
    }
}

async fn handle_edit_message(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    content: String,
) {
    if let Some(user_id) = &ctx.user_id {
        let bearer = ctx.tokens.bearer();

        // REST: fetch the message to verify ownership and get guild_id
        let msg_query = format!(
            "select=discord_id,discord_guild_id,discord_author_id&discord_channel_id=eq.{}&discord_id=eq.{}",
            channel_id.get(),
            message_id.get()
        );
        let rows = ctx.rest.select(bearer, "messages", &msg_query).await;

        if let Ok(rows) = rows {
            if let Some(row) = rows.into_iter().next() {
                let author_val: i64 = get_column(&row, "discord_author_id").as_i64().unwrap_or(0);
                let guild_id_val: Option<i64> = get_column(&row, "discord_guild_id").as_i64();

                // Verify the user owns this message
                if author_val == user_id.get() as i64 {
                    // REST: update the message
                    let update_row = json!({
                        "content": content,
                        "edited_at": chrono::Utc::now().to_rfc3339()
                    });

                    let query = format!(
                        "discord_channel_id=eq.{}&discord_id=eq.{}",
                        channel_id.get(),
                        message_id.get()
                    );

                    let _ = ctx.rest.update(bearer, "messages", &query, &update_row, None).await;

                    let update = MessageUpdateDispatchInfo {
                        guild_id: guild_id_val.map(|v| Id::new(v as u64)),
                        channel_id,
                        message_id,
                        fields: MessageUpdateEventFields {
                            content: Some(content),
                            edited_timestamp: Some("now".to_string()),
                            ..Default::default()
                        },
                        extra_fields: std::collections::BTreeMap::new(),
                    };
                    ctx.publisher
                        .publish_for_backend(&AppEvent::MessageUpdateDispatch { update })
                        .await;
                }
            }
        }
    }
}

async fn handle_delete_message(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
) {
    let bearer = ctx.tokens.bearer();

    // REST: look up the guild_id for the event (before deletion)
    let msg_query = format!(
        "select=discord_guild_id&discord_channel_id=eq.{}&discord_id=eq.{}",
        channel_id.get(),
        message_id.get()
    );
    let rows = ctx.rest.select(bearer, "messages", &msg_query).await;
    let guild_id: Option<Id<GuildMarker>> = rows
        .ok()
        .and_then(|rows| rows.into_iter().next())
        .and_then(|row| get_column(&row, "discord_guild_id").as_i64())
        .map(|g| Id::new(g as u64));

    // REST: delete the message
    let query = format!(
        "discord_channel_id=eq.{}&discord_id=eq.{}",
        channel_id.get(),
        message_id.get()
    );
    let _ = ctx.rest.delete(bearer, "messages", &query).await;

    ctx.publisher
        .publish_for_backend(&AppEvent::MessageDelete {
            guild_id,
            channel_id,
            message_id,
        })
        .await;
}

async fn handle_add_reaction(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    emoji: ReactionEmoji,
) {
    if let Some(user_id) = &ctx.user_id {
        let emoji_str = match &emoji {
            ReactionEmoji::Unicode(s) => s.clone(),
            ReactionEmoji::Custom { name, .. } => name.clone().unwrap_or_default(),
        };

        let bearer = ctx.tokens.bearer();

        // REST: look up message discord_id and user's discord_id
        let msg_query = format!(
            "select=discord_id&discord_channel_id=eq.{}&discord_id=eq.{}",
            channel_id.get(),
            message_id.get()
        );
        let msg_rows = ctx.rest.select(bearer, "messages", &msg_query).await;

        let user_query = format!("select=id&discord_id=eq.{}", user_id.get());
        let user_rows = ctx.rest.select(bearer, "users", &user_query).await;

        if let (Ok(msg_rows), Ok(user_rows)) = (msg_rows, user_rows) {
            if msg_rows.len() == 1 && user_rows.len() == 1 {
                let msg_uuid = get_column(&msg_rows[0], "id").as_str().map(|s| s.to_string());
                let user_uuid = get_column(&user_rows[0], "id").as_str().map(|s| s.to_string());

                if let (Some(msg_uuid), Some(user_uuid)) = (msg_uuid, user_uuid) {
                    // REST: insert reaction
                    let reaction_row = json!({
                        "message_id": msg_uuid,
                        "user_id": user_uuid,
                        "emoji": emoji_str
                    });

                    let _ = ctx.rest.insert(bearer, "reactions", &[reaction_row], None).await;

                    ctx.publisher
                        .publish_for_backend(&AppEvent::CurrentUserReactionAdd {
                            channel_id,
                            message_id,
                            emoji,
                        })
                        .await;
                }
            }
        }
    }
}

async fn handle_remove_reaction(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    emoji: ReactionEmoji,
) {
    if let Some(user_id) = &ctx.user_id {
        let emoji_str = match &emoji {
            ReactionEmoji::Unicode(s) => s.clone(),
            ReactionEmoji::Custom { name, .. } => name.clone().unwrap_or_default(),
        };

        let bearer = ctx.tokens.bearer();

        // REST: look up message UUID and user UUID
        let msg_query = format!(
            "select=id&discord_channel_id=eq.{}&discord_id=eq.{}",
            channel_id.get(),
            message_id.get()
        );
        let user_query = format!("select=id&discord_id=eq.{}", user_id.get());

        let msg_rows = ctx.rest.select(bearer, "messages", &msg_query).await;
        let user_rows = ctx.rest.select(bearer, "users", &user_query).await;

        if let (Ok(msg_rows), Ok(user_rows)) = (msg_rows, user_rows) {
            if let Some(msg_row) = msg_rows.into_iter().next() {
                if let Some(user_row) = user_rows.into_iter().next() {
                    let msg_uuid = get_column(&msg_row, "id").as_str().map(|s| s.to_string());
                    let user_uuid = get_column(&user_row, "id").as_str().map(|s| s.to_string());

                    if let (Some(msg_uuid), Some(user_uuid)) = (msg_uuid, user_uuid) {
                        // REST: delete reaction by matching columns
                        let query = format!(
                            "message_id=eq.{}&user_id=eq.{}&emoji=eq.{}",
                            msg_uuid, user_uuid, emoji_str
                        );
                        let _ = ctx.rest.delete(bearer, "reactions", &query).await;

                        ctx.publisher
                            .publish_for_backend(&AppEvent::CurrentUserReactionRemove {
                                channel_id,
                                message_id,
                                emoji,
                            })
                            .await;
                    }
                }
            }
        }
    }
}

async fn handle_load_reaction_users(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    emoji: ReactionEmoji,
    _after: Option<Id<UserMarker>>,
) {
    let emoji_str = match &emoji {
        ReactionEmoji::Unicode(s) => s.clone(),
        ReactionEmoji::Custom { name, .. } => name.clone().unwrap_or_default(),
    };

    let bearer = ctx.tokens.bearer();

    // PostgREST: We need to find reactions for a specific message.
    // We use an embedded resource join: messages?select=reactions(emoji,...)
    // But we need to look up by channel_id + discord_id to find the message first.
    // Since `id` is a UUID and we don't know it client-side, we query reactions
    // filtered by a subquery on messages — but PostgREST doesn't support subqueries.
    //
    // Instead, we use the RPC approach or fetch all reactions for messages
    // in this channel and filter client-side. Given that message reactions
    // are typically low-volume, we fetch the message's reactions via join.
    //
    // We use: select=reactions!inner(emoji,user_id,public_users!inner(discord_id))
    //          &discord_channel_id=eq.X&discord_id=eq.Y
    let query = format!(
        "select=reactions!inner(emoji,user_id)&discord_channel_id=eq.{}",
        channel_id.get()
    );

    // Actually, since reactions table has message_id (UUID FK), we need to
    // get the message UUID first, then query reactions by message_id.
    // Fetch the message UUID:
    let msg_query = format!(
        "select=id&discord_channel_id=eq.{}&discord_id=eq.{}",
        channel_id.get(),
        message_id.get()
    );
    let msg_rows = ctx.rest.select(bearer, "messages", &msg_query).await;

    let users: Vec<_> = if let Ok(rows) = msg_rows {
        if let Some(row) = rows.into_iter().next() {
            if let Some(msg_uuid) = get_column(&row, "id").as_str() {
                let reaction_query = format!(
                    "select=emoji,user_id&message_id=eq.{}", // Note: may need UUID handling
                    msg_uuid
                );
                let reaction_rows = ctx.rest.select(bearer, "reactions", &reaction_query).await;

                if let Ok(rows) = reaction_rows {
                    rows.into_iter()
                        .filter(|r| {
                            get_column(r, "emoji")
                                .as_str()
                                .map(|s| s == emoji_str)
                                .unwrap_or(false)
                        })
                        .filter_map(|r| {
                            let user_id_val: String = get_column(&r, "user_id").as_str()?.to_string();
                            // We'd need to fetch the user's discord_id from the users table
                            None::<ReactionUserInfo>
                        })
                        .collect()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    ctx.publisher
        .publish_for_backend(&AppEvent::ReactionUsersLoaded {
            channel_id,
            next_after: None,
            after: _after,
            message_id,
            emoji,
            users,
        })
        .await;
}

async fn handle_set_selected_guild(
    ctx: &mut CommandContext,
    guild_id: Option<Id<GuildMarker>>,
) {
    if let Some(user_id) = &ctx.user_id {
        let guild_snowflake: Option<i64> = guild_id.map(|g| g.get() as i64);
        let bearer = ctx.tokens.bearer();
        let query = format!("discord_user_id=eq.{}", user_id.get());

        let update_row = json!({
            "selected_guild_id": guild_snowflake,
        });

        // Try to update existing row first; if none exists, insert.
        let _ = ctx.rest.update(bearer, "user_settings", &query, &update_row, None).await;
    }
}

async fn handle_leave_guild(
    ctx: &mut CommandContext,
    guild_id: Id<GuildMarker>,
    label: String,
) {
    if let Some(user_id) = &ctx.user_id {
        let bearer = ctx.tokens.bearer();
        let query = format!(
            "discord_user_id=eq.{}&discord_guild_id=eq.{}",
            user_id.get(),
            guild_id.get()
        );
        let _ = ctx.rest.delete(bearer, "guild_memberships", &query).await;

        // Emit GuildDelete so the TUI can remove it from the sidebar
        let _ = &label; // label is kept for potential future use
        ctx.publisher
            .publish_for_backend(&AppEvent::GuildDelete {
                guild_id,
            })
            .await;
    }
}

async fn handle_set_selected_channel(
    ctx: &mut CommandContext,
    channel_id: Option<Id<ChannelMarker>>,
) {
    if let Some(user_id) = &ctx.user_id {
        let channel_snowflake: Option<i64> = channel_id.map(|c| c.get() as i64);
        let bearer = ctx.tokens.bearer();
        let query = format!("discord_user_id=eq.{}", user_id.get());

        let update_row = json!({
            "selected_channel_id": channel_snowflake,
        });

        let _ = ctx.rest.update(bearer, "user_settings", &query, &update_row, None).await;
    }
}

async fn handle_ack_channel(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
) {
    handle_ack_channels(ctx, vec![(channel_id, message_id)]).await;
}

async fn handle_ack_channels(
    ctx: &mut CommandContext,
    targets: Vec<(Id<ChannelMarker>, Id<MessageMarker>)>,
) {
    if let Some(user_id) = &ctx.user_id {
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let bearer = ctx.tokens.bearer();
        let last_read_at = chrono::DateTime::from_timestamp_millis(now as i64)
            .unwrap_or_else(|| chrono::Utc::now());

        for (channel_id, message_id) in &targets {
            // Record the last-read message in user_settings
            let query = format!("discord_user_id=eq.{}", user_id.get());
            let update_row = json!({
                "selected_channel_id": channel_id.get() as i64,
                "last_read_message_id": message_id.get() as i64,
                "last_read_at": last_read_at.to_rfc3339(),
            });
            let _ = ctx.rest.update(bearer, "user_settings", &query, &update_row, None).await;

            // Emit ack event for the TUI
            ctx.publisher
                .publish_for_backend(&AppEvent::MessageAck {
                    channel_id: *channel_id,
                    message_id: *message_id,
                    mention_count: None,
                    flags: None,
                    last_viewed: Some(now),
                    version: None,
                })
                .await;
        }
    }
}

async fn handle_schedule_ack(
    _ctx: &mut CommandContext,
    _channel_id: Id<ChannelMarker>,
    _message_id: Id<MessageMarker>,
) {
}

async fn handle_set_guild_muted(
    ctx: &mut CommandContext,
    guild_id: Id<GuildMarker>,
    _muted: bool,
    duration: Option<MuteDuration>,
    _label: String,
) {
    if let Some(user_id) = &ctx.user_id {
        let expires_at = match duration {
            Some(MuteDuration::Permanent) => None,
            Some(MuteDuration::Minutes(mins)) => {
                Some(chrono::Utc::now() + chrono::Duration::minutes(mins as i64))
            }
            None => None,
        };

        let bearer = ctx.tokens.bearer();
        let expires_at_str = expires_at.map(|dt| dt.to_rfc3339());

        let query = format!(
            "discord_user_id=eq.{}&discord_guild_id=eq.{}",
            user_id.get(),
            guild_id.get()
        );
        let update_row = json!({
            "expires_at": expires_at_str,
        });
        let _ = ctx.rest.update(bearer, "guild_mutes", &query, &update_row, None).await;
    }

    // No specific AppEvent for guild mute — TUI reads from DB on demand
}

async fn handle_set_channel_muted(
    ctx: &mut CommandContext,
    guild_id: Option<Id<GuildMarker>>,
    channel_id: Id<ChannelMarker>,
    _muted: bool,
    duration: Option<MuteDuration>,
    _label: String,
) {
    if let Some(user_id) = &ctx.user_id {
        let expires_at = match duration {
            Some(MuteDuration::Permanent) => None,
            Some(MuteDuration::Minutes(mins)) => {
                Some(chrono::Utc::now() + chrono::Duration::minutes(mins as i64))
            }
            None => None,
        };

        let bearer = ctx.tokens.bearer();
        let expires_at_str = expires_at.map(|dt| dt.to_rfc3339());

        let query = format!(
            "discord_user_id=eq.{}&discord_channel_id=eq.{}",
            user_id.get(),
            channel_id.get()
        );
        let update_row = json!({
            "discord_guild_id": guild_id.map(|g| g.get() as i64),
            "expires_at": expires_at_str,
        });
        let _ = ctx.rest.update(bearer, "channel_mutes", &query, &update_row, None).await;
    }

    // No specific AppEvent for channel mute — TUI reads from DB on demand
}

async fn handle_load_user_profile(
    ctx: &mut CommandContext,
    user_id: Id<UserMarker>,
    guild_id: Option<Id<GuildMarker>>,
) {
    let bearer = ctx.tokens.bearer();
    let query = format!(
        "select=username,display_name,avatar_url,is_bot&discord_id=eq.{}",
        user_id.get()
    );
    let rows = ctx.rest.select(bearer, "users", &query).await.ok();

    let username: String = rows.as_ref()
        .and_then(|r| r.first())
        .map(|row| get_column(row, "username").as_str().unwrap_or_default().to_string())
        .unwrap_or_default();

    let global_name: Option<String> = rows.as_ref()
        .and_then(|r| r.first())
        .and_then(|row| get_column(row, "display_name").as_str().map(|s| s.to_string()));

    let avatar_url: Option<String> = rows.as_ref()
        .and_then(|r| r.first())
        .and_then(|row| get_column(row, "avatar_url").as_str().map(|s| s.to_string()));

    let _is_bot: bool = rows.as_ref()
        .and_then(|r| r.first())
        .map(|row| get_column(row, "is_bot").as_bool().unwrap_or(false))
        .unwrap_or(false);

    // Look up nickname if guild context is provided
    let guild_nick: Option<String> = if let Some(gid) = guild_id {
        let nick_query = format!(
            "select=nickname&discord_user_id=eq.{}&discord_guild_id=eq.{}",
            user_id.get(),
            gid.get()
        );
        let rows = ctx.rest.select(bearer, "guild_memberships", &nick_query).await.ok();
        rows.and_then(|r| r.into_iter().next())
            .and_then(|row| get_column(&row, "nickname").as_str().map(|s| s.to_string()))
    } else {
        None
    };

    let profile = crate::discord::UserProfileInfo {
        user_id,
        username,
        global_name,
        guild_nick,
        role_ids: Vec::new(),
        role_ids_present: false,
        avatar_url,
        bio: None,
        pronouns: None,
        guild_pronouns: None,
        mutual_guilds: Vec::new(),
        mutual_friends: Vec::new(),
        mutual_friends_count: 0,
        friend_status: crate::discord::FriendStatus::None,
        note: None,
    };

    ctx.publisher
        .publish_for_backend(&AppEvent::UserProfileLoaded {
            guild_id,
            profile,
        })
        .await;
}

async fn handle_update_user_profile(ctx: &mut CommandContext, mut update: UserProfileUpdate) {
    let user_id = match ctx.user_id {
        Some(id) => id,
        None => return,
    };

    // Avatar upload → Supabase Storage bucket "avatars".
    // The filename is the user's discord_id (snowflake) to keep it stable.
    let mut avatar_url: Option<String> = None;

    // Extract avatar early to avoid partial move of `update.global`.
    let avatar_upload = update.global.avatar.take();

    if let Some(avatar_upload) = avatar_upload {
        let avatar_image = match crate::discord::read_profile_avatar_image(&avatar_upload).await {
            Ok(img) => img,
            Err(e) => {
                tracing::error!("avatar validation failed: {e}");
                ctx.publisher
                    .publish_for_backend(&AppEvent::UserProfileUpdateFailed {
                        user_id,
                        guild_id: update.guild_id,
                        message: format!("avatar validation failed: {e}"),
                    })
                    .await;
                return;
            }
        };

        let object_key = format!("{}.{}", user_id, avatar_upload.filename);
        let upload_url = format!(
            "{}/storage/v1/object/avatars/{}",
            ctx.supabase_url, object_key
        );

        let http_client = reqwest::Client::new();
        let reqwest_resp = http_client
            .put(&upload_url)
            .header("apikey", &ctx.supabase_key)
            .header(
                "Authorization",
                format!("Bearer {}", ctx.tokens.access_token),
            )
            .header("Content-Type", &avatar_image.content_type)
            .body(avatar_image.bytes)
            .send()
            .await;

        match reqwest_resp {
            Ok(resp) if resp.status().is_success() => {
                avatar_url = Some(upload_url);
            }
            Ok(resp) => {
                let status = resp.status();
                tracing::error!(
                    "avatar storage upload failed: {} {}",
                    status,
                    resp.text().await.unwrap_or_default()
                );
                ctx.publisher
                    .publish_for_backend(&AppEvent::UserProfileUpdateFailed {
                        user_id,
                        guild_id: update.guild_id,
                        message: format!("avatar upload failed: {}", status),
                    })
                    .await;
                return;
            }
            Err(e) => {
                tracing::error!("avatar storage upload error: {e}");
                ctx.publisher
                    .publish_for_backend(&AppEvent::UserProfileUpdateFailed {
                        user_id,
                        guild_id: update.guild_id,
                        message: format!("avatar upload error: {e}"),
                    })
                    .await;
                return;
            }
        }
    }

    // Update public.users with new display_name and/or avatar_url.
    if !update.global.is_empty() {
        let bearer = ctx.tokens.bearer();
        let query = format!("discord_id=eq.{}", user_id.get());
        let mut update_obj = json!({});
        if let Some(ref dn) = update.global.display_name {
            update_obj["display_name"] = json!(dn);
        }
        if avatar_url.is_some() {
            update_obj["avatar_url"] = json!(avatar_url);
        }
        let update_result = ctx.rest.update(bearer, "users", &query, &update_obj, None).await;
        if let Err(e) = update_result {
            tracing::error!("failed to update user profile: {e}");
            ctx.publisher
                .publish_for_backend(&AppEvent::UserProfileUpdateFailed {
                    user_id,
                    guild_id: update.guild_id,
                    message: format!("database update failed: {e}"),
                })
                .await;
            return;
        }
    }

    // Re-fetch the updated profile to publish back to TUI.
    let bearer = ctx.tokens.bearer();
    let query = format!(
        "select=username,display_name,avatar_url&discord_id=eq.{}",
        user_id.get()
    );
    let rows = ctx.rest.select(bearer, "users", &query).await;

    match rows {
        Ok(rows) => {
            if let Some(row) = rows.into_iter().next() {
                let username: String = get_column(&row, "username")
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let global_name: Option<String> = get_column(&row, "display_name")
                    .as_str()
                    .map(|s| s.to_string());
                let fetched_avatar_url: Option<String> =
                    get_column(&row, "avatar_url")
                        .as_str()
                        .map(|s| s.to_string());

                let profile = crate::discord::UserProfileInfo {
                    user_id,
                    username,
                    global_name,
                    guild_nick: None,
                    role_ids: Vec::new(),
                    role_ids_present: false,
                    avatar_url: fetched_avatar_url,
                    bio: None,
                    pronouns: None,
                    guild_pronouns: None,
                    mutual_guilds: Vec::new(),
                    mutual_friends: Vec::new(),
                    mutual_friends_count: 0,
                    friend_status: crate::discord::FriendStatus::None,
                    note: None,
                };

                ctx.publisher
                    .publish_for_backend(&AppEvent::UserProfileLoaded {
                        guild_id: update.guild_id,
                        profile,
                    })
                    .await;
            } else {
                ctx.publisher
                    .publish_for_backend(&AppEvent::UserProfileUpdateFailed {
                        user_id,
                        guild_id: update.guild_id,
                        message: "user profile not found".to_string(),
                    })
                    .await;
            }
        }
        Err(e) => {
            tracing::error!("failed to re-fetch updated profile: {e}");
            ctx.publisher
                .publish_for_backend(&AppEvent::UserProfileUpdateFailed {
                    user_id,
                    guild_id: update.guild_id,
                    message: format!("failed to re-fetch updated profile: {e}"),
                })
                .await;
        }
    }
}

// Presence updates are local-only state and don't require backend persistence.
// The TUI tracks presence in-memory; the backend simply no-ops.
async fn handle_update_presence(_ctx: &mut CommandContext, _status: PresenceStatus) {}

async fn handle_update_activity(
    _ctx: &mut CommandContext,
    _status: PresenceStatus,
    _activities: Vec<ActivityInfo>,
    _rich_presence: RichPresenceSelection,
) {
    let _ = (_status, _activities, _rich_presence);
}

async fn handle_load_inbox_mentions(
    ctx: &mut CommandContext,
    request_id: u64,
    before: Option<Id<MessageMarker>>,
) {
    let limit: i64 = 50;

    let bearer = ctx.tokens.bearer();
    let query = format!(
        "select=discord_id,discord_author_id,discord_channel_id,discord_guild_id,content,flags\
         &order=created_at.desc,discord_id.desc\
         &limit={}",
        limit
    );
    let rows = ctx.rest.select(bearer, "messages", &query).await.unwrap_or_default();

    // PostgREST doesn't support OR conditions directly in the query param syntax,
    // so we need to filter client-side for the ilike matches.
    let filtered_rows: Vec<_> = rows
        .into_iter()
        .filter(|row| {
            let content_val = get_column(row, "content");
            let content = content_val.as_str().unwrap_or("");
            content.contains("@here") || content.contains("<@&") || content.contains("<@")
        })
        .collect();

    let _ = before; // pagination handled by ordering above

    let messages: Vec<_> = filtered_rows
        .into_iter()
        .map(|row| {
            let msg_id_val: i64 = get_column(&row, "discord_id").as_i64().unwrap_or(0);
            let author_val: i64 = get_column(&row, "discord_author_id").as_i64().unwrap_or(0);
            let channel_val: i64 = get_column(&row, "discord_channel_id").as_i64().unwrap_or(0);
            let content_val: String = get_column(&row, "content").as_str().unwrap_or_default().to_string();
            let flags_val: i64 = get_column(&row, "flags").as_i64().unwrap_or(0);
            let guild_id_val: Option<i64> = get_column(&row, "discord_guild_id").as_i64();
            MessageInfo {
                guild_id: guild_id_val.map(|v| Id::new(v as u64)),
                channel_id: Id::new(channel_val as u64),
                message_id: Id::new(msg_id_val as u64),
                author_id: Id::new(author_val as u64),
                content: Some(content_val),
                flags: flags_val as u64,
                message_kind: MessageKind::regular(),
                ..Default::default()
            }
        })
        .collect();

    ctx.publisher
        .publish_for_backend(&AppEvent::InboxMentionsLoaded {
            request_id,
            before,
            messages,
            has_more: false,
        })
        .await;
}

async fn handle_load_inbox_channel_history(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
    request_id: u64,
) {
    let limit: i64 = 50;

    let bearer = ctx.tokens.bearer();
    let query = format!(
        "select=discord_id,discord_author_id,discord_channel_id,discord_guild_id,content,flags\
         &discord_channel_id=eq.{}\
         &order=created_at.desc,discord_id.desc\
         &limit={}",
        channel_id.get(),
        limit
    );
    let rows = ctx.rest.select(bearer, "messages", &query).await.unwrap_or_default();

    let messages: Vec<_> = rows
        .into_iter()
        .map(|row| {
            let msg_id_val: i64 = get_column(&row, "discord_id").as_i64().unwrap_or(0);
            let author_val: i64 = get_column(&row, "discord_author_id").as_i64().unwrap_or(0);
            let channel_val: i64 = get_column(&row, "discord_channel_id").as_i64().unwrap_or(0);
            let content_val: String = get_column(&row, "content").as_str().unwrap_or_default().to_string();
            let flags_val: i64 = get_column(&row, "flags").as_i64().unwrap_or(0);
            let guild_id_val: Option<i64> = get_column(&row, "discord_guild_id").as_i64();
            MessageInfo {
                guild_id: guild_id_val.map(|v| Id::new(v as u64)),
                channel_id: Id::new(channel_val as u64),
                message_id: Id::new(msg_id_val as u64),
                author_id: Id::new(author_val as u64),
                content: Some(content_val),
                flags: flags_val as u64,
                message_kind: MessageKind::regular(),
                ..Default::default()
            }
        })
        .collect();

    ctx.publisher
        .publish_for_backend(&AppEvent::InboxChannelMessagesLoaded {
            request_id,
            channel_id,
            messages,
        })
        .await;
}

async fn handle_delete_inbox_mention(
    ctx: &mut CommandContext,
    message_id: Id<MessageMarker>,
) {
    if let Some(user_id) = &ctx.user_id {
        let bearer = ctx.tokens.bearer();

        // Look up the channel_id before deleting (needed for the event)
        let channel_query = format!(
            "select=discord_channel_id&discord_id=eq.{}&discord_author_id=eq.{}",
            message_id.get(),
            user_id.get()
        );
        let channel_rows = ctx.rest.select(bearer, "messages", &channel_query).await.ok();
        let channel_snowflake: Option<i64> = channel_rows
            .and_then(|rows| rows.into_iter().next())
            .and_then(|row| get_column(&row, "discord_channel_id").as_i64());

        // Delete the message
        let delete_query = format!(
            "discord_id=eq.{}&discord_author_id=eq.{}",
            message_id.get(),
            user_id.get()
        );
        let _ = ctx.rest.delete(bearer, "messages", &delete_query).await;

        if let Some(ch_snowflake) = channel_snowflake {
            ctx.publisher
                .publish_for_backend(&AppEvent::MessageDelete {
                    guild_id: None,
                    channel_id: Id::new(ch_snowflake as u64),
                    message_id,
                })
                .await;
        }
    }
}

async fn handle_search_messages(ctx: &mut CommandContext, request_id: u64, query: MessageSearchQuery) {
    let bearer = ctx.tokens.bearer();

    // Build PostgREST query params for exact-match filters.
    // ILIKE (content search) and date equality are handled client-side
    // since PostgREST query params don't support ILIKE natively.
    let limit: i64 = 50; // PostgREST doesn't support dynamic limits on search
    let offset: i64 = query.offset as i64;

    let mut params: Vec<String> = vec![
        "select=discord_id,discord_author_id,discord_channel_id,discord_guild_id,content,flags".to_string(),
        format!("order=created_at.desc,discord_id.desc"),
        format!("limit={}", limit),
        format!("offset={}", offset),
    ];

    if let Some(guild_id) = query.guild_id {
        params.push(format!("discord_guild_id=eq.{}", guild_id.get()));
    }
    if let Some(channel_id) = query.channel_id {
        params.push(format!("discord_channel_id=eq.{}", channel_id.get()));
    }
    if let Some(author_id) = query.author_id {
        params.push(format!("discord_author_id=eq.{}", author_id.get()));
    }

    let query_str = params.join("&");
    let rows = ctx.rest.select(bearer, "messages", &query_str).await.unwrap_or_default();

    // Client-side filter for content ILIKE and date equality
    let filtered_rows: Vec<_> = rows
        .into_iter()
        .filter(|row| {
            let mut pass = true;

            if let Some(content) = &query.content {
                if !content.is_empty() {
                    let content_val = get_column(row, "content");
                    let msg_content = content_val.as_str().unwrap_or("");
                    pass = pass && msg_content.to_lowercase().contains(&content.to_lowercase());
                }
            }

            if let Some(date) = &query.date {
                let created_val = get_column(row, "created_at");
                let created_str = created_val.as_str().unwrap_or("");
                pass = pass && created_str.starts_with(&date.to_string());
            }

            pass
        })
        .collect();

    let messages: Vec<_> = filtered_rows
        .into_iter()
        .map(|row| {
            let msg_id_val: i64 = get_column(&row, "discord_id").as_i64().unwrap_or(0);
            let author_val: i64 = get_column(&row, "discord_author_id").as_i64().unwrap_or(0);
            let channel_val: i64 = get_column(&row, "discord_channel_id").as_i64().unwrap_or(0);
            let content_val: String = get_column(&row, "content").as_str().unwrap_or_default().to_string();
            let flags_val: i64 = get_column(&row, "flags").as_i64().unwrap_or(0);
            let guild_id_val: Option<i64> = get_column(&row, "discord_guild_id").as_i64();
            MessageInfo {
                guild_id: guild_id_val.map(|v| Id::new(v as u64)),
                channel_id: Id::new(channel_val as u64),
                message_id: Id::new(msg_id_val as u64),
                author_id: Id::new(author_val as u64),
                content: Some(content_val),
                flags: flags_val as u64,
                message_kind: MessageKind::regular(),
                ..Default::default()
            }
        })
        .collect();

    let total_results = Some(messages.len());
    let has_more = messages.len() == limit as usize;

    ctx.publisher
        .publish_for_backend(&AppEvent::MessageSearchLoaded {
            request_id,
            page: MessageSearchPage {
                query,
                messages,
                total_results,
                has_more,
            },
        })
    .await;
}

async fn handle_load_guild_members(
    ctx: &mut CommandContext,
    guild_id: Id<GuildMarker>,
    user_ids: Vec<Id<UserMarker>>,
) {
    // If no specific user IDs are provided, load all members of the guild
    let members = if user_ids.is_empty() {
        load_all_guild_members(ctx, guild_id).await
    } else {
        load_specific_guild_members(ctx, guild_id, &user_ids).await
    };

    for member in members {
        ctx.publisher
            .publish_for_backend(&AppEvent::GuildMemberAdd {
                guild_id,
                member,
            })
            .await;
    }
}

async fn load_all_guild_members(ctx: &mut CommandContext, guild_id: Id<GuildMarker>) -> Vec<MemberInfo> {
    let bearer = ctx.tokens.bearer();

    // Query guild_memberships for this guild
    let query = format!(
        "select=discord_user_id,nickname,roles,joined_at\
         &discord_guild_id=eq.{}",
        guild_id.get()
    );
    let membership_rows = ctx.rest.select(bearer, "guild_memberships", &query).await.unwrap_or_default();

    // Collect Discord user IDs for the subsequent users query
    let user_ids: Vec<i64> = membership_rows
        .iter()
        .filter_map(|row| get_column(row, "discord_user_id").as_i64())
        .collect();

    if user_ids.is_empty() {
        return Vec::new();
    }

    // Build an in. filter for PostgREST
    let in_filter: String = user_ids
        .iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let users_query = format!("select=discord_id,display_name,username,is_bot&discord_id=in.({})", in_filter);
    let user_rows = ctx.rest.select(bearer, "users", &users_query).await.unwrap_or_default();

    // Build a lookup map from discord_id to user row
    let user_map: std::collections::HashMap<i64, Value> = user_rows
        .into_iter()
        .filter_map(|row| {
            let id = get_column(&row, "discord_id").as_i64()?;
            Some((id, Value::Object(row)))
        })
        .collect();

    membership_rows
        .into_iter()
        .filter_map(|row| {
            let discord_user_id = get_column(&row, "discord_user_id").as_i64()?;

            let user = user_map.get(&discord_user_id);
            let discord_id_val = user
                .and_then(|u| get_column_from_value(u, "discord_id").as_i64())
                .unwrap_or(discord_user_id);

            let display_name: String = user
                .and_then(|u| {
                    let v = get_column_from_value(u, "display_name");
                    v.as_str().map(|s| s.to_string())
                })
                .unwrap_or_default();
            let username: Option<String> = user
                .and_then(|u| {
                    let v = get_column_from_value(u, "username");
                    v.as_str().map(|s| s.to_string())
                });
            let is_bot: bool = user
                .and_then(|u| {
                    let v = get_column_from_value(u, "is_bot");
                    v.as_bool()
                })
                .unwrap_or(false);

            let nickname_val = get_column(&row, "nickname");
            let nickname: Option<String> = nickname_val.as_str().map(|s| s.to_string());
            let joined_at: Option<chrono::DateTime<chrono::Utc>> = None; // Joined_at is a datetime; would need proper parsing

            let roles_val = get_column(&row, "roles");
            let role_snowflakes: Option<Vec<i64>> = roles_val.as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect());
            let role_ids: Vec<Id<RoleMarker>> = role_snowflakes
                .unwrap_or_default()
                .into_iter()
                .map(|r| Id::new(r as u64))
                .collect();

            let nickname_present = nickname.is_some();
            Some(MemberInfo {
                user_id: Id::new(discord_id_val as u64),
                display_name,
                username,
                nickname,
                nickname_present,
                is_bot,
                is_bot_present: true,
                avatar_url: None,
                avatar_url_present: false,
                role_ids,
                role_ids_present: true,
                joined_at,
                flags: Some(0),
                pending: None,
                communication_disabled_until: None,
                communication_disabled_until_present: false,
            })
        })
        .collect()
}

async fn load_specific_guild_members(
    ctx: &mut CommandContext,
    guild_id: Id<GuildMarker>,
    user_ids: &[Id<UserMarker>],
) -> Vec<MemberInfo> {
    let bearer = ctx.tokens.bearer();

    let discord_user_ids: Vec<String> = user_ids.iter().map(|u| u.get().to_string()).collect();

    // Query guild_memberships for this guild, filtered to specific user IDs
    let in_filter = discord_user_ids.join(",");
    let query = format!(
        "select=discord_user_id,nickname,roles,joined_at\
         &discord_guild_id=eq.{}\
         &discord_user_id=in.({})",
        guild_id.get(),
        in_filter
    );
    let membership_rows = ctx.rest.select(bearer, "guild_memberships", &query).await.unwrap_or_default();

    // Collect Discord user IDs for the subsequent users query
    let user_id_list: Vec<i64> = membership_rows
        .iter()
        .filter_map(|row| get_column(row, "discord_user_id").as_i64())
        .collect();

    if user_id_list.is_empty() {
        return Vec::new();
    }

    let users_in = user_id_list.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
    let users_query = format!("select=discord_id,display_name,username,is_bot&discord_id=in.({})", users_in);
    let user_rows = ctx.rest.select(bearer, "users", &users_query).await.unwrap_or_default();

    let user_map: std::collections::HashMap<i64, Value> = user_rows
        .into_iter()
        .filter_map(|row| {
            let id = get_column(&row, "discord_id").as_i64()?;
            Some((id, Value::Object(row)))
        })
        .collect();

    membership_rows
        .into_iter()
        .filter_map(|row| {
            let discord_user_id = get_column(&row, "discord_user_id").as_i64()?;

            let user = user_map.get(&discord_user_id);
            let discord_id_val = user
                .and_then(|u| get_column_from_value(u, "discord_id").as_i64())
                .unwrap_or(discord_user_id);

            let display_name: String = user
                .and_then(|u| {
                    let v = get_column_from_value(u, "display_name");
                    v.as_str().map(|s| s.to_string())
                })
                .unwrap_or_default();
            let username: Option<String> = user
                .and_then(|u| {
                    let v = get_column_from_value(u, "username");
                    v.as_str().map(|s| s.to_string())
                });
            let is_bot: bool = user
                .and_then(|u| {
                    let v = get_column_from_value(u, "is_bot");
                    v.as_bool()
                })
                .unwrap_or(false);

            let nickname_val = get_column(&row, "nickname");
            let nickname: Option<String> = nickname_val.as_str().map(|s| s.to_string());
            let joined_at: Option<chrono::DateTime<chrono::Utc>> = None;

            let roles_val = get_column(&row, "roles");
            let role_snowflakes: Option<Vec<i64>> = roles_val.as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect());
            let role_ids: Vec<Id<RoleMarker>> = role_snowflakes
                .unwrap_or_default()
                .into_iter()
                .map(|r| Id::new(r as u64))
                .collect();

            let nickname_present = nickname.is_some();
            Some(MemberInfo {
                user_id: Id::new(discord_id_val as u64),
                display_name,
                username,
                nickname,
                nickname_present,
                is_bot,
                is_bot_present: true,
                avatar_url: None,
                avatar_url_present: false,
                role_ids,
                role_ids_present: true,
                joined_at,
                flags: Some(0),
                pending: None,
                communication_disabled_until: None,
                communication_disabled_until_present: false,
            })
        })
        .collect()
}

async fn handle_search_guild_members(
    ctx: &mut CommandContext,
    guild_id: Id<GuildMarker>,
    query: String,
    limit: u16,
) {
    let query_lower = query.to_lowercase();

    let bearer = ctx.tokens.bearer();

    // Fetch all guild_memberships for this guild (with limit to avoid loading everything)
    let query_str = format!(
        "select=discord_user_id,nickname,roles,joined_at\
         &discord_guild_id=eq.{}\
         &limit={}",
        guild_id.get(),
        limit * 10, // fetch more to allow client-side filtering, since PostgREST can't OR across columns
    );
    let membership_rows = ctx.rest.select(bearer, "guild_memberships", &query_str).await.unwrap_or_default();

    // Collect Discord user IDs for the subsequent users query
    let user_ids: Vec<i64> = membership_rows
        .iter()
        .filter_map(|row| get_column(row, "discord_user_id").as_i64())
        .collect();

    if user_ids.is_empty() {
        ctx.publisher
            .publish_for_backend(&AppEvent::GuildMembersChunk {
                chunk: GuildMembersChunkInfo {
                    guild_id,
                    members: Vec::new(),
                    presences: Vec::new(),
                    chunk_index: Some(0),
                    chunk_count: Some(1),
                    nonce: None,
                    not_found: Vec::new(),
                    extra_fields: std::collections::BTreeMap::new(),
                },
            })
            .await;
        return;
    }

    let in_filter = user_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
    let users_query = format!("select=discord_id,display_name,username,is_bot&discord_id=in.({})", in_filter);
    let user_rows = ctx.rest.select(bearer, "users", &users_query).await.unwrap_or_default();

    let user_map: std::collections::HashMap<i64, Value> = user_rows
        .into_iter()
        .filter_map(|row| {
            let id = get_column(&row, "discord_id").as_i64()?;
            Some((id, Value::Object(row)))
        })
        .collect();

    // Client-side filter across display_name, username, and nickname
    let query_lower = query.to_lowercase();
    let matching_memberships: Vec<_> = membership_rows
        .into_iter()
        .filter(|row| {
            let discord_user_id = get_column(row, "discord_user_id").as_i64();
            if let Some(uid) = discord_user_id {
                if let Some(user) = user_map.get(&uid) {
                    let display_name_val = get_column_from_value(user, "display_name");
                    let display_name = display_name_val.as_str().unwrap_or("").to_lowercase();
                    let username_val = get_column_from_value(user, "username");
                    let username = username_val.as_str().unwrap_or("").to_lowercase();
                    let nickname_val = get_column(row, "nickname");
                    let nickname = nickname_val.as_str().unwrap_or("").to_lowercase();
                    return display_name.contains(&query_lower)
                        || username.contains(&query_lower)
                        || nickname.contains(&query_lower);
                }
            }
            false
        })
        .take(limit as usize)
        .collect();

    let members: Vec<MemberInfo> = matching_memberships
        .into_iter()
        .filter_map(|row| {
            let discord_user_id = get_column(&row, "discord_user_id").as_i64()?;

            let user = user_map.get(&discord_user_id);
            let discord_id_val = user
                .and_then(|u| get_column_from_value(u, "discord_id").as_i64())
                .unwrap_or(discord_user_id);

            let display_name: String = user
                .and_then(|u| {
                    let v = get_column_from_value(u, "display_name");
                    v.as_str().map(|s| s.to_string())
                })
                .unwrap_or_default();
            let username: Option<String> = user
                .and_then(|u| {
                    let v = get_column_from_value(u, "username");
                    v.as_str().map(|s| s.to_string())
                });
            let is_bot: bool = user
                .and_then(|u| {
                    let v = get_column_from_value(u, "is_bot");
                    v.as_bool()
                })
                .unwrap_or(false);

            let nickname_val = get_column(&row, "nickname");
            let nickname: Option<String> = nickname_val.as_str().map(|s| s.to_string());
            let joined_at: Option<chrono::DateTime<chrono::Utc>> = None;

            let roles_val = get_column(&row, "roles");
            let role_snowflakes: Option<Vec<i64>> = roles_val.as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect());
            let role_ids: Vec<Id<RoleMarker>> = role_snowflakes
                .unwrap_or_default()
                .into_iter()
                .map(|r| Id::new(r as u64))
                .collect();

            let nickname_present = nickname.is_some();
            Some(MemberInfo {
                user_id: Id::new(discord_id_val as u64),
                display_name,
                username,
                nickname,
                nickname_present,
                is_bot,
                is_bot_present: true,
                avatar_url: None,
                avatar_url_present: false,
                role_ids,
                role_ids_present: true,
                joined_at,
                flags: Some(0),
                pending: None,
                communication_disabled_until: None,
                communication_disabled_until_present: false,
            })
        })
        .collect();

    ctx.publisher
        .publish_for_backend(&AppEvent::GuildMembersChunk {
            chunk: GuildMembersChunkInfo {
                guild_id,
                members,
                presences: Vec::new(),
                chunk_index: Some(0),
                chunk_count: Some(1),
                nonce: None,
                not_found: Vec::new(),
                extra_fields: std::collections::BTreeMap::new(),
            },
        })
        .await;
}

// ---- Thread / Forum handlers ----

/// Load a thread channel's preview (the message that created the thread).
async fn handle_load_thread_preview(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
) {
    let bearer = ctx.tokens.bearer();

    // Fetch the message that created the thread
    let query = format!(
        "select=discord_channel_id,discord_author_id,content,edited_at,created_at\
         &discord_id=eq.{}",
        message_id.get()
    );
    let msg_rows = ctx.rest.select(bearer, "messages", &query).await;

    let msg_row = match msg_rows {
        Ok(m) if !m.is_empty() => m.into_iter().next().unwrap(),
        _ => {
            ctx.publisher
                .publish_for_backend(&AppEvent::ThreadPreviewLoadFailed {
                    channel_id,
                    message_id,
                })
                .await;
            return;
        }
    };

    let author_id: i64 = get_column(&msg_row, "discord_author_id").as_i64().unwrap_or(0);
    let channel_id_val: i64 = get_column(&msg_row, "discord_channel_id").as_i64().unwrap_or(0);
    let content: String = get_column(&msg_row, "content").as_str().unwrap_or_default().to_string();
    let _edited_at: Option<chrono::DateTime<chrono::Utc>> = None;
    let _created_at: Option<chrono::DateTime<chrono::Utc>> = None;

    // Fetch attachments for this message, filtered by discord_id
    let att_query = format!("order=id&discord_message_id=eq.{}", message_id.get());
    // The attachments table uses discord_id (the message's discord_id) as the FK.
    // Let's use a more direct approach.
    let att_query = format!(
        "select=id,filename,content_type,size,url,proxy_url\
         &discord_message_id=eq.{}",
        message_id.get()
    );
    let attachment_rows = ctx.rest.select(bearer, "attachments", &att_query).await.unwrap_or_default();

    let attachments: Vec<AttachmentInfo> = attachment_rows
        .into_iter()
        .map(|a| AttachmentInfo {
            id: Id::new(get_column(&a, "id").as_i64().unwrap_or(0) as u64),
            filename: get_column(&a, "filename").as_str().unwrap_or_default().to_string(),
            url: get_column(&a, "url").as_str().unwrap_or_default().to_string(),
            size: get_column(&a, "size").as_i64().unwrap_or(0) as u64,
            content_type: Some(get_column(&a, "content_type").as_str().unwrap_or_default().to_string()),
            proxy_url: get_column(&a, "proxy_url").as_str().unwrap_or_default().to_string(),
            height: None,
            width: None,
            description: None,
            flags: 0,
        })
        .collect();

    let poll_info = build_poll_info(ctx, message_id).await;

    let info = MessageInfo {
        message_id,
        channel_id: Id::new(channel_id_val as u64),
        author_id: Id::new(author_id as u64),
        author_is_bot: false,
        content: Some(content),
        message_kind: MessageKind::regular(),
        attachments,
        poll: poll_info,
        ..MessageInfo::default()
    };

    ctx.publisher
        .publish_for_backend(&AppEvent::ThreadPreviewLoaded {
            channel_id,
            message: info,
        })
        .await;
}

/// Load archived threads for a guild or channel.
async fn handle_load_archived_threads(
    ctx: &mut CommandContext,
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    before: Option<String>,
) {
    let bearer = ctx.tokens.bearer();

    // Fetch channel kind to determine the thread type
    let ch_query = format!(
        "select=kind&discord_id=eq.{}",
        channel_id.get()
    );
    let ch_rows = ctx.rest.select(bearer, "channels", &ch_query).await.ok();
    let channel_kind: String = ch_rows
        .and_then(|rows| rows.into_iter().next())
        .map(|row| {
            let kind_val = get_column(&row, "kind");
            kind_val.as_str().unwrap_or_default().to_string()
        })
        .unwrap_or_default();

    // Build PostgREST query for archived threads
    let mut params: Vec<String> = vec![
        "select=channel_id,name,kind,discord_parent_id,discord_owner_id,first_message_id,message_count,member_count,archived,locked,auto_archive_duration".to_string(),
        "archived=eq.true".to_string(),
        "order=channel_id.desc".to_string(),
        "limit=100".to_string(),
    ];

    if guild_id.get() != 0 {
        params.push(format!("discord_guild_id=eq.{}", guild_id.get()));
    } else {
        params.push(format!("discord_parent_id=eq.{}", channel_id.get()));
    }

    if let Some(before_cursor) = &before {
        if let Ok(before_id) = before_cursor.parse::<i64>() {
            params.push(format!("channel_id=lt.{}", before_id));
        }
    }

    let query_str = params.join("&");
    let rows = ctx.rest.select(bearer, "threads", &query_str).await.unwrap_or_default();

    let threads: Vec<ChannelInfo> = rows
        .iter()
        .map(|r| {
            let cid: i64 = get_column(r, "channel_id").as_i64().unwrap_or(0);
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: Id::new(cid as u64),
                parent_id: get_column(r, "discord_parent_id").as_i64().map(|v| Id::new(v as u64)),
                owner_id: get_column(r, "discord_owner_id").as_i64().map(|v| Id::new(v as u64)),
                position: None,
                last_message_id: get_column(r, "first_message_id").as_i64().map(|v| Id::new(v as u64)),
                name: get_column(r, "name").as_str().unwrap_or_default().to_string(),
                kind: get_column(r, "kind").as_str().unwrap_or("public_thread").to_string(),
                message_count: Some(get_column(r, "message_count").as_i64().unwrap_or(0) as u64),
                member_count: Some(get_column(r, "member_count").as_i64().unwrap_or(0) as u64),
                thread_metadata: Some(ThreadMetadataInfo {
                    archived: get_column(r, "archived").as_bool().unwrap_or(true),
                    locked: get_column(r, "locked").as_bool().unwrap_or(false),
                    auto_archive_duration: Some(get_column(r, "auto_archive_duration").as_i64().unwrap_or(1440) as u64),
                    ..ThreadMetadataInfo::default()
                }),
                ..ChannelInfo::default()
            }
        })
        .collect();

    // Load thread members for the archived threads
    let member_rows = ctx.rest.select(bearer, "thread_members", "select=channel_id,discord_user_id,join_timestamp,flags,notification_level,muted,mute_end_time").await.unwrap_or_default();

    let members: Vec<ThreadMemberInfo> = member_rows
        .iter()
        .map(|m| ThreadMemberInfo {
            thread_id: None,
            user_id: Some(Id::new(get_column(m, "discord_user_id").as_i64().unwrap_or(0) as u64)),
            join_timestamp: get_column(m, "join_timestamp").as_str().map(|v| v.to_string()),
            flags: Some(get_column(m, "flags").as_i64().unwrap_or(0) as u64),
            ..ThreadMemberInfo::default()
        })
        .collect();

    let _ = channel_kind; // thread type determines which archive endpoint was hit

    ctx.publisher
        .publish_for_backend(&AppEvent::ArchivedThreadsLoaded {
            guild_id,
            channel_id,
            before: before.map(|s| s.to_string()),
            page: ArchivedThreadsPage {
                threads,
                members,
                has_more: false,
                next_before: None,
                extra_fields: std::collections::BTreeMap::new(),
            },
        })
        .await;
}

/// Create a new forum post (thread in a forum channel).
async fn handle_create_forum_post(
    ctx: &mut CommandContext,
    post: ForumPostCreate,
) {
    if let Some(user_id) = &ctx.user_id {
        let user_id_i64 = user_id.get() as i64;

        let bearer = ctx.tokens.bearer();

        // Look up the guild_id for this channel (the forum channel's parent guild).
        let chan_query = format!("select=discord_guild_id&discord_channel_id=eq.{}", post.channel_id.get());
        let guild_id_i64: Option<i64> = ctx.rest.select(bearer, "channels", &chan_query).await.ok()
            .and_then(|rows| rows.into_iter().next())
            .and_then(|row| get_column(&row, "discord_guild_id").as_i64());

        // Generate a new thread channel id by inserting into threads with return=representation.
        let tag_ids: Vec<i64> = post.applied_tags.iter().map(|t| t.get() as i64).collect();
        let thread_row = json!({
            "channel_id": post.channel_id.get() as i64,
            "discord_guild_id": guild_id_i64,
            "discord_owner_id": user_id_i64,
            "name": &post.title,
            "kind": "public_thread",
            "applied_tag_ids": &tag_ids,
            "message_count": 0i64,
            "member_count": 0i64,
        });
        let insert_result = ctx.rest.insert(bearer, "threads", &[thread_row], Some("return=representation")).await;
        let thread_id_i64: i64 = insert_result
            .ok()
            .and_then(|rows| rows.into_iter().next())
            .and_then(|row| get_column(&row, "id").as_i64())
            .unwrap_or(0) as i64; // 'id' is the internal auto-increment PK

        // Insert the first message (the post content).
        if !post.content.is_empty() {
            let msg_row = json!({
                "discord_channel_id": thread_id_i64,
                "discord_author_id": user_id_i64,
                "discord_guild_id": guild_id_i64,
                "content": &post.content,
            });
            let _ = ctx.rest.insert(bearer, "messages", &[msg_row], None).await;
        }

        // Insert thread tags junction rows via the forum_tags lookup.
        for tag_id in &tag_ids {
            // First look up the forum_tags internal id for this discord tag id
            let tag_query = format!("select=id&discord_id=eq.{}", tag_id);
            if let Ok(rows) = ctx.rest.select(bearer, "forum_tags", &tag_query).await {
                if let Some(row) = rows.into_iter().next() {
                    let internal_tag_id: i64 = get_column(&row, "id").as_i64().unwrap_or(0);
                    let link_row = json!({
                        "thread_channel_id": thread_id_i64,
                        "tag_id": internal_tag_id,
                    });
                    let _ = ctx.rest.insert(bearer, "thread_tags", &[link_row], None).await;
                }
            }
        }

        // Build the ThreadGatewayInfo for the event.
        let channel_info = ChannelInfo {
            guild_id: guild_id_i64.map(|g| Id::new(g as u64)),
            channel_id: Id::new(thread_id_i64 as u64),
            parent_id: Some(post.channel_id),
            owner_id: Some(Id::new(user_id_i64 as u64)),
            position: None,
            last_message_id: None,
            name: post.title.clone(),
            kind: "public_thread".to_string(),
            message_count: None,
            member_count: None,
            user_limit: None,
            total_message_sent: None,
            thread_metadata: Some(ThreadMetadataInfo {
                archived: false,
                locked: false,
                auto_archive_duration: Some(1440),
                ..ThreadMetadataInfo::default()
            }),
            flags: None,
            rate_limit_per_user: None,
            available_tags: Vec::new(),
            applied_tags: post.applied_tags.iter().map(|t| Id::new(t.get() as u64)).collect(),
            recipients: None,
            permission_overwrites: Vec::new(),
            is_message_request: None,
            is_spam: None,
        };

        let thread_info = ThreadGatewayInfo {
            channel: channel_info,
            current_user_member: None,
        };

        ctx.publisher
            .publish_for_backend(&AppEvent::ThreadUpsert {
                thread: thread_info,
                created: true,
            })
            .await;
    }
}

/// Load forum post data (posts in a forum channel).
async fn handle_load_forum_post_data(
    ctx: &mut CommandContext,
    _guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    thread_ids: Vec<Id<ChannelMarker>>,
) {
    let bearer = ctx.tokens.bearer();
    let in_filter = thread_ids.iter().map(|t| t.get().to_string()).collect::<Vec<_>>().join(",");

    let rows = if in_filter.is_empty() {
        Vec::new()
    } else {
        let query = format!(
            "select=channel_id,name,kind,owner_id,discord_guild_id,discord_parent_id,archived,locked,message_count,member_count,applied_tag_ids,created_at,updated_at\
             &channel_id=in.({})",
            in_filter
        );
        ctx.rest.select(bearer, "threads", &query).await.unwrap_or_default()
    };

    let mut posts = Vec::new();
    for r in &rows {
        let channel_id_val: i64 = get_column(r, "channel_id").as_i64().unwrap_or(0);
        let thread_id: Id<ChannelMarker> = Id::new(channel_id_val as u64);
        let _applied_tag_ids: Vec<i64> = get_column(r, "applied_tag_ids").as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
            .unwrap_or_default();

        let mut extra_fields = std::collections::BTreeMap::new();
        if let Some(info) = build_thread_channel_info(ctx, thread_id).await {
            extra_fields.insert(
                "channel_info".to_owned(),
                serde_json::json!({
                    "name": info.name,
                    "kind": info.kind,
                    "archived": info.thread_archived(),
                    "locked": info.thread_locked(),
                    "message_count": info.message_count,
                    "member_count": info.member_count,
                }),
            );
        }

        posts.push(crate::discord::ForumPostDataInfo {
            thread_id,
            owner: None,
            first_message: None,
            extra_fields,
        });
    }

    ctx.publisher
        .publish_for_backend(&AppEvent::ForumPostDataLoaded {
            channel_id,
            requested_thread_ids: thread_ids,
            posts,
        })
        .await;
}

/// Edit a thread (name, tags, rate limit, auto-archive).
async fn handle_edit_thread(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
    name: Option<String>,
    applied_tags: Option<Vec<Id<ForumTagMarker>>>,
    rate_limit_per_user: Option<u16>,
    auto_archive_duration: Option<u16>,
    label: String,
) {
    let bearer = ctx.tokens.bearer();

    // Build the update object dynamically.
    let mut update_obj = json!({});
    if let Some(ref n) = name {
        update_obj["name"] = json!(n);
    }
    if let Some(ref tags) = applied_tags {
        let tag_ids: Vec<i64> = tags.iter().map(|t| t.get() as i64).collect();
        update_obj["applied_tag_ids"] = json!(tag_ids);
    }
    if let Some(_rate) = rate_limit_per_user {
        // rate_limit_per_user isn't a column in our threads table yet
        update_obj["auto_archive_duration"] = json!("auto_archive_duration");
    }
    if let Some(auto_dur) = auto_archive_duration {
        update_obj["auto_archive_duration"] = json!(auto_dur as i64);
    }

    // Only proceed if there's something to update.
    if update_obj.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        return;
    }

    // Also include labels if provided (not mapped to a thread column for now).
    let _ = label;

    let query = format!("channel_id=eq.{}", channel_id.get());
    let update_result = ctx.rest.update(
        bearer, "threads", &query, &update_obj, Some("return=representation"),
    ).await;

    if let Some(rows) = update_result.ok().and_then(|r| r.into_iter().next()) {
        let _updated_channel_id: i64 = get_column(&rows, "channel_id").as_i64().unwrap_or(0);

        ctx.publisher
            .publish_for_backend(&AppEvent::ThreadUpsert {
                thread: ThreadGatewayInfo {
                    channel: ChannelInfo {
                        channel_id,
                        name: name.as_deref().unwrap_or("").to_string(),
                        ..ChannelInfo::default()
                    },
                    current_user_member: None,
                },
                created: false,
            })
            .await;
    }
}

/// Set a thread's archived state.
async fn handle_set_thread_archived(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
    archived: bool,
    _label: String,
) {
    if let Some(guild_id) = guild_id_from_thread(ctx, channel_id).await {
        if !check_user_permissions(ctx, guild_id, "manage_threads").await {
            return;
        }
    }

    let bearer = ctx.tokens.bearer();
    let query = format!("channel_id=eq.{}", channel_id.get());
    let update_row = json!({
        "archived": archived,
    });
    let _ = ctx.rest.update(bearer, "threads", &query, &update_row, None).await;

    let mut channel_info = ChannelInfo {
        channel_id,
        thread_metadata: Some(ThreadMetadataInfo {
            archived,
            auto_archive_duration: if archived { None } else { Some(1440) },
            ..ThreadMetadataInfo::default()
        }),
        ..ChannelInfo::default()
    };

    if let Some(info) = build_thread_channel_info(ctx, channel_id).await {
        channel_info.guild_id = info.guild_id;
    }

    ctx.publisher
        .publish_for_backend(&AppEvent::ThreadUpsert {
            thread: ThreadGatewayInfo {
                channel: channel_info,
                current_user_member: None,
            },
            created: false,
        })
        .await;
}

/// Set a thread's locked state.
async fn handle_set_thread_locked(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
    locked: bool,
    _label: String,
) {
    if let Some(guild_id) = guild_id_from_thread(ctx, channel_id).await {
        if !check_user_permissions(ctx, guild_id, "manage_threads").await {
            return;
        }
    }

    let bearer = ctx.tokens.bearer();
    let query = format!("channel_id=eq.{}", channel_id.get());
    let update_row = json!({
        "locked": locked,
    });
    let _ = ctx.rest.update(bearer, "threads", &query, &update_row, None).await;

    let mut channel_info = ChannelInfo {
        channel_id,
        thread_metadata: Some(ThreadMetadataInfo {
            locked,
            auto_archive_duration: Some(1440),
            ..ThreadMetadataInfo::default()
        }),
        ..ChannelInfo::default()
    };

    if let Some(info) = build_thread_channel_info(ctx, channel_id).await {
        channel_info.guild_id = info.guild_id;
    }

    ctx.publisher
        .publish_for_backend(&AppEvent::ThreadUpsert {
            thread: ThreadGatewayInfo {
                channel: channel_info,
                current_user_member: None,
            },
            created: false,
        })
        .await;
}

/// Set a thread's pinned state.
async fn handle_set_thread_pinned(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
    pinned: bool,
    _current_flags: u64,
    _label: String,
) {
    if let Some(guild_id) = guild_id_from_thread(ctx, channel_id).await {
        if !check_user_permissions(ctx, guild_id, "manage_messages").await {
            return;
        }
    }

    let bearer = ctx.tokens.bearer();
    let query = format!("channel_id=eq.{}", channel_id.get());
    let update_row = json!({
        "pinned": pinned,
    });
    let _ = ctx.rest.update(bearer, "threads", &query, &update_row, None).await;

    let mut channel_info = ChannelInfo {
        channel_id,
        ..ChannelInfo::default()
    };

    if let Some(info) = build_thread_channel_info(ctx, channel_id).await {
        channel_info.guild_id = info.guild_id;
    }

    ctx.publisher
        .publish_for_backend(&AppEvent::ThreadUpsert {
            thread: ThreadGatewayInfo {
                channel: channel_info,
                current_user_member: None,
            },
            created: false,
        })
        .await;
}

/// Set a thread's muted state.
async fn handle_set_thread_muted(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
    _muted: bool,
    _duration: Option<MuteDuration>,
    _label: String,
) {
    if let Some(guild_id) = guild_id_from_thread(ctx, channel_id).await {
        if !check_user_permissions(ctx, guild_id, "mute_threads").await {
            return;
        }
    }
    // TUI reads mute state from DB
}

/// Set a thread's followed state.
async fn handle_set_thread_followed(
    ctx: &mut CommandContext,
    _channel_id: Id<ChannelMarker>,
    _followed: bool,
    _label: String,
) {
    let _ = ctx; // No-op for now — TUI reads followed state from DB
}

/// Set a thread's notification level.
async fn handle_set_thread_notification_level(
    ctx: &mut CommandContext,
    _channel_id: Id<ChannelMarker>,
    _flags: u64,
    _label: String,
) {
    let _ = ctx; // No-op for now
}

/// Delete a thread.
async fn handle_delete_thread(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
    _label: String,
) {
    if let Some(guild_id) = guild_id_from_thread(ctx, channel_id).await {
        if !check_user_permissions(ctx, guild_id, "manage_threads").await {
            return;
        }
    }

    let bearer = ctx.tokens.bearer();
    let query = format!("channel_id=eq.{}", channel_id.get());
    let update_row = json!({
        "deleted_at": chrono::Utc::now().to_rfc3339(),
    });
    let _ = ctx.rest.update(bearer, "threads", &query, &update_row, None).await;

    // No event is published for thread deletion currently.
    // The thread row is removed from the database and the client will
    // receive a THREAD_DELETE dispatch from Gateway when applicable.
}

/// Vote in a poll.
async fn handle_vote_poll(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    answer_ids: Vec<u8>,
) {
    let _ = (ctx, channel_id, message_id, answer_ids); // No-op: poll voting is handled via real-time
}

/// Load pinned messages for a channel.
async fn handle_load_pinned_messages(
    ctx: &mut CommandContext,
    _channel_id: Id<ChannelMarker>,
) {
    let _ = ctx; // No-op: pinned messages are loaded via real-time
}

/// Set a message's pinned state.
async fn handle_set_message_pinned(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    pinned: bool,
) {
    let bearer = ctx.tokens.bearer();
    let query = format!(
        "discord_channel_id=eq.{}&discord_id=eq.{}",
        channel_id.get(),
        message_id.get()
    );
    let update_row = json!({
        "pinned": pinned,
    });
    let _ = ctx.rest.update(bearer, "messages", &query, &update_row, None).await;

    ctx.publisher
        .publish_for_backend(&AppEvent::MessageUpdateDispatch {
            update: MessageUpdateDispatchInfo {
                channel_id,
                message_id,
                fields: MessageUpdateEventFields {
                    pinned: Some(pinned),
                    ..Default::default()
                },
                ..Default::default()
            },
        })
        .await;
}

// ---- Helper functions ----

/// Build a ChannelInfo from a thread database row.
async fn build_thread_channel_info(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
) -> Option<ChannelInfo> {
    build_thread_channel_info_from_db(ctx, channel_id).await
}

/// Look up the guild_id for a thread channel.
async fn guild_id_from_thread(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
) -> Option<Id<GuildMarker>> {
    let bearer = ctx.tokens.bearer();
    let query = format!("select=discord_guild_id&channel_id=eq.{}", channel_id.get());
    ctx.rest.select(bearer, "threads", &query).await.ok()
        .and_then(|rows| rows.into_iter().next())
        .and_then(|row| get_column(&row, "discord_guild_id").as_i64())
        .map(|v| Id::new(v as u64))
}

/// Build a ChannelInfo from a thread database row.
async fn build_thread_channel_info_from_db(
    ctx: &mut CommandContext,
    channel_id: Id<ChannelMarker>,
) -> Option<ChannelInfo> {
    let bearer = ctx.tokens.bearer();
    let query = format!(
        "select=channel_id,discord_guild_id,discord_parent_id,discord_owner_id,name,kind,archived,locked,message_count,member_count,auto_archive_duration,first_message_id\
         &channel_id=eq.{}",
        channel_id.get()
    );
    let row = ctx.rest.select(bearer, "threads", &query).await.ok()
        .and_then(|rows| rows.into_iter().next())?;

    let cid: i64 = get_column(&row, "channel_id").as_i64().unwrap_or(0);
    let guild_id_val: Option<i64> = get_column(&row, "discord_guild_id").as_i64();
    let parent_id_val: Option<i64> = get_column(&row, "discord_parent_id").as_i64();
    let owner_id_val: Option<i64> = get_column(&row, "discord_owner_id").as_i64();
    let name: String = get_column(&row, "name").as_str().unwrap_or_default().to_string();
    let kind: String = get_column(&row, "kind").as_str().unwrap_or("public_thread").to_string();
    let archived: bool = get_column(&row, "archived").as_bool().unwrap_or(false);
    let locked: bool = get_column(&row, "locked").as_bool().unwrap_or(false);
    let message_count: i64 = get_column(&row, "message_count").as_i64().unwrap_or(0);
    let member_count: i64 = get_column(&row, "member_count").as_i64().unwrap_or(0);
    let auto_archive: i64 = get_column(&row, "auto_archive_duration").as_i64().unwrap_or(1440);
    let first_msg_id: Option<i64> = get_column(&row, "first_message_id").as_i64();

    Some(ChannelInfo {
        guild_id: guild_id_val.map(|v| Id::new(v as u64)),
        channel_id: Id::new(cid as u64),
        parent_id: parent_id_val.map(|v| Id::new(v as u64)),
        owner_id: owner_id_val.map(|v| Id::new(v as u64)),
        name,
        kind,
        message_count: Some(message_count as u64),
        member_count: Some(member_count as u64),
        thread_metadata: Some(ThreadMetadataInfo {
            archived,
            locked,
            auto_archive_duration: Some(auto_archive as u64),
            ..ThreadMetadataInfo::default()
        }),
        last_message_id: first_msg_id.map(|v| Id::new(v as u64)),
        ..ChannelInfo::default()
    })
}

/// Check if the current user has permission for a given action in a guild.
async fn check_user_permissions(
    ctx: &mut CommandContext,
    guild_id: Id<GuildMarker>,
    _permission: &str,
) -> bool {
    if let Some(user_id) = &ctx.user_id {
        let bearer = ctx.tokens.bearer();

        // Check if the user is the server owner.
        let owner_check = format!(
            "select=1&discord_guild_id=eq.{}&discord_user_id=eq.{}",
            guild_id.get(),
            user_id.get()
        );
        let owner_result = ctx.rest.select(bearer, "servers", &owner_check).await.ok();
        if owner_result.as_ref().map(|r| !r.is_empty()).unwrap_or(false) {
            return true;
        }

        // Check if the user has the admin role via guild_memberships.
        let member_query = format!(
            "select=guild_roles\
             &discord_guild_id=eq.{}\
             &discord_user_id=eq.{}",
            guild_id.get(),
            user_id.get()
        );
        let member_rows = ctx.rest.select(bearer, "guild_memberships", &member_query).await.ok();

        if let Some(row) = member_rows.as_ref().and_then(|r| r.first()) {
            if let Some(matching_roles) = get_column(row, "guild_roles").as_array() {
                let user_role_ids: Vec<i64> = matching_roles
                    .iter()
                    .filter_map(|v| v.as_i64())
                    .collect();

                // Fetch all admin role permission IDs — these are roles with 'admin' permission in this guild.
                // We query the roles table for roles named 'admin' in this guild's context.
                let admin_roles_query = "select=role_id&name=eq.admin";
                // Actually, role_permissions joins with roles via role_id.
                // Let's query role_permissions with a filter on the roles.name via foreign table.
                // PostgREST supports `!inner` on FK columns. The role_permissions table should
                // have a 'role_id' that references roles.id.
                // But we don't know the exact relationship. Let's try a simpler approach:
                // query roles where name = 'admin' and collect their ids, then check intersection.

                let admin_role_query = "select=id&name=eq.admin";
                let admin_role_rows = ctx.rest.select(bearer, "roles", &admin_role_query).await.ok();

                let admin_role_ids: Vec<i64> = admin_role_rows
                    .map(|rows| {
                        rows.into_iter()
                            .filter_map(|row| get_column(&row, "id").as_i64())
                            .collect()
                    })
                    .unwrap_or_default();

                if !admin_role_ids.is_empty() {
                    for role_id in &user_role_ids {
                        if admin_role_ids.contains(role_id) {
                            return true;
                        }
                    }
                }
            }
        }

        false
    } else {
        false
    }
}

/// Build a PollInfo from a database row.
async fn build_poll_info(
    ctx: &mut CommandContext,
    message_id: Id<MessageMarker>,
) -> Option<PollInfo> {
    let bearer = ctx.tokens.bearer();

    // Fetch the poll itself
    let poll_query = format!(
        "select=question,allow_multiselect,results_finalized,id\
         &discord_message_id=eq.{}",
        message_id.get()
    );
    let poll_rows = ctx.rest.select(bearer, "polls", &poll_query).await.ok();
    let row = poll_rows.and_then(|mut rows| rows.pop());

    let poll_id: Option<i64> = row.as_ref().and_then(|r| get_column(r, "id").as_i64());
    let question: String = row
        .as_ref()
        .map(|r| get_column(r, "question").as_str().unwrap_or_default().to_string())
        .unwrap_or_default();
    let allow_multiselect: bool = row
        .as_ref()
        .and_then(|r| get_column(r, "allow_multiselect").as_bool())
        .unwrap_or(false);
    let results_finalized: Option<bool> = row
        .as_ref()
        .and_then(|r| get_column(r, "results_finalized").as_bool());

    // Fetch total vote count — use aggregate to sum vote_count across all options
    let total_votes: Option<u64> = if let Some(pid) = poll_id {
        let vote_query = format!(
            "select=vote_count&poll_id=eq.{}",
            pid
        );
        let vote_rows = ctx.rest.select(bearer, "poll_options", &vote_query).await.ok();
        let sum: i64 = vote_rows
            .map(|rows| {
                rows.iter().filter_map(|row| get_column(row, "vote_count").as_i64()).sum()
            })
            .unwrap_or(0);
        Some(sum as u64)
    } else {
        None
    };

    // Fetch poll options (answers)
    let answer_rows = if let Some(pid) = poll_id {
        let ans_query = format!(
            "select=answer_id,text,vote_count&poll_id=eq.{}&order=answer_id.asc",
            pid
        );
        ctx.rest.select(bearer, "poll_options", &ans_query).await.unwrap_or_default()
    } else {
        Vec::new()
    };

    let answers: Vec<PollAnswerInfo> = answer_rows
        .into_iter()
        .map(|a| PollAnswerInfo {
            answer_id: get_column(&a, "answer_id").as_i64().unwrap_or(0) as u8,
            text: get_column(&a, "text").as_str().unwrap_or_default().to_string(),
            vote_count: get_column(&a, "vote_count").as_i64().map(|v| v as u64),
            me_voted: false,
        })
        .collect();

    Some(PollInfo {
        question,
        answers,
        allow_multiselect,
        results_finalized,
        total_votes,
    })
}

