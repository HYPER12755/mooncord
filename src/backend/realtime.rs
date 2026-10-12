//! Supabase Realtime WebSocket client.
//!
//! Connects to `wss://<ref>.supabase.co/realtime/v1/websocket` with the
//! user's JWT and subscribes to:
//! - `postgres_changes` on public channels (guild text channel messages,
//!   presences, typing — RLS applies automatically)
//! - Private Broadcast channels for DMs (since postgres_changes does NOT
//!   enforce RLS on delivered rows, see §5)
//!
//! Inbound Realtime messages are decoded and converted into `AppEvent`s
//! that are published through the shared `AppEventPublisher` pipeline.

use crate::discord::{AppEventPublisher, Id, marker::{UserMarker, ChannelMarker, GuildMarker, MessageMarker}, AppEvent, MessageInfo, MessageUpdateDispatchInfo, MessageUpdateEventFields};

use serde::Deserialize;
use tokio_tungstenite::connect_async_tls_with_config;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::Utf8Bytes;
use futures::StreamExt;
use futures::SinkExt;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use super::token_store::SessionTokens;

/// An inbound Supabase Realtime WebSocket message.
///
/// Supabase Realtime uses a Socket.io v2-compatible wire format. Each
/// frame is a JSON object with `topic`, `event`, `payload`, and optionally
/// `ref`.
#[derive(Deserialize, Debug)]
struct RealtimeFrame {
    #[allow(dead_code)]
    topic: String,
    event: String,
    #[allow(dead_code)]
    r#type: Option<String>,
    payload: serde_json::Value,
}

/// Realtime subscription type.
enum Subscription {
    /// postgres_changes on messages for a specific guild channel
    PostgresMessages { #[allow(dead_code)] guild_id: Id<GuildMarker>, channel_id: Id<ChannelMarker> },
    /// Private broadcast channel for DMs
    BroadcastDM { channel_id: Id<ChannelMarker> },
    /// postgres_changes on presences
    PostgresPresences { #[allow(dead_code)] guild_id: Option<Id<GuildMarker>> },
}

/// Realtime client — manages the WebSocket connection to Supabase Realtime.
pub struct RealtimeClient {
    ws_url: String,
    tokens: SessionTokens,
    publisher: AppEventPublisher,
    subscriptions: Arc<std::sync::Mutex<HashMap<String, Subscription>>>,
}

impl RealtimeClient {
    pub fn new(
        supabase_url: &str,
        tokens: SessionTokens,
        publisher: AppEventPublisher,
    ) -> Self {
        let ws_url = format!(
            "{}/realtime/v1/websocket",
            supabase_url.trim_end_matches('/')
        );

        Self {
            ws_url,
            tokens,
            publisher,
            subscriptions: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Connect and start processing Realtime messages.
    pub async fn run(&self) -> tokio::task::JoinHandle<()> {
        let ws_url = self.ws_url.clone();
        let access_token = self.tokens.access_token.clone();
        let publisher = self.publisher.clone();
        let subscriptions = Arc::clone(&self.subscriptions);

        tokio::spawn(async move {
            // Build the WebSocket URL with the auth token
            let url_str = format!("{}?access_token={}", ws_url, urlencoding(&access_token));

            loop {
                match connect_async_tls_with_config(&url_str, None, false, None).await {
                    Ok((ws_stream, _response)) => {
                        tracing::info!("Connected to Supabase Realtime");

                        let (mut write_half, mut read_half) = ws_stream.split();

                        // Join the Realtime socket
                        let join_msg = serde_json::json!({
                            "type": "join",
                            "payload": {
                                "access_token": access_token,
                            }
                        });
                        if let Err(e) = write_half.send(WsMessage::Text(
                            Utf8Bytes::from(join_msg.to_string())
                        )).await {
                            tracing::error!("Failed to join Realtime socket: {}", e);
                            return;
                        }

                        // Subscribe to all active subscriptions
                        for msg in build_subscription_messages(&subscriptions) {
                            let _ = write_half.send(WsMessage::Text(
                                Utf8Bytes::from(msg.to_string())
                            )).await;
                        }

                        // Process incoming messages
                        while let Some(msg) = read_half.next().await {
                            match msg {
                                Ok(WsMessage::Text(text)) => {
                                    handle_realtime_message(&text, &publisher).await;
                                }
                                Ok(WsMessage::Close(_)) => {
                                    tracing::warn!("Realtime WebSocket closed, reconnecting...");
                                    break;
                                }
                                Err(e) => {
                                    tracing::error!("Realtime WebSocket error: {}", e);
                                    break;
                                }
                                _ => {}
                            }
                        }

                        // Reconnect after a delay
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                    Err(e) => {
                        tracing::error!("Realtime connection failed: {}, retrying...", e);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        })
    }

    /// Subscribe to postgres_changes on messages for a guild channel.
    /// RLS applies because the subscription is authenticated as the user.
    pub fn subscribe_guild_messages(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
    ) {
        let key = format!("messages:{}", channel_id.get());
        let sub = Subscription::PostgresMessages { guild_id, channel_id };
        self.subscriptions.lock().unwrap().insert(key, sub);
    }

    /// Subscribe to a private Broadcast channel for DMs.
    /// DM message delivery goes through Broadcast to preserve RLS confidentiality.
    pub fn subscribe_dm(
        &self,
        channel_id: Id<ChannelMarker>,
    ) {
        let key = format!("dm:{}", channel_id.get());
        let sub = Subscription::BroadcastDM { channel_id };
        self.subscriptions.lock().unwrap().insert(key, sub);
    }

    /// Subscribe to presences in a guild.
    pub fn subscribe_presences(
        &self,
        guild_id: Option<Id<GuildMarker>>,
    ) {
        let key = format!("presences:{:?}", guild_id);
        let sub = Subscription::PostgresPresences { guild_id };
        self.subscriptions.lock().unwrap().insert(key, sub);
    }
}


/// Build the Realtime subscription messages for all active subscriptions.
/// Called on (re)connect to re-subscribe.
fn build_subscription_messages(
    subscriptions: &Arc<std::sync::Mutex<HashMap<String, Subscription>>>,
) -> Vec<serde_json::Value> {
    let subs = subscriptions.lock().unwrap();
    subs.values()
        .flat_map(|sub| match sub {
            Subscription::PostgresMessages { guild_id: _, channel_id } => {
                let ch = channel_id.get();
                vec![
                    serde_json::json!({
                        "type": "subscribe",
                        "event": "realtime",
                        "payload": {
                            "event": "INSERT",
                            "params": {
                                "schema": "public",
                                "table": "messages",
                                "filter": format!("discord_channel_id=eq.{}", ch),
                            }
                        }
                    }),
                    serde_json::json!({
                        "type": "subscribe",
                        "event": "realtime",
                        "payload": {
                            "event": "UPDATE",
                            "params": {
                                "schema": "public",
                                "table": "messages",
                                "filter": format!("discord_channel_id=eq.{}", ch),
                            }
                        }
                    }),
                    serde_json::json!({
                        "type": "subscribe",
                        "event": "realtime",
                        "payload": {
                            "event": "DELETE",
                            "params": {
                                "schema": "public",
                                "table": "messages",
                                "filter": format!("discord_channel_id=eq.{}", ch),
                            }
                        }
                    }),
                ]
            }
            Subscription::BroadcastDM { channel_id } => {
                let ch = channel_id.get();
                vec![
                    serde_json::json!({
                        "type": "subscribe",
                        "event": "broadcast",
                        "payload": {
                            "channel": format!("dm:{}", ch),
                        }
                    }),
                    serde_json::json!({
                        "type": "subscribe",
                        "event": "presence",
                        "payload": {
                            "channel": format!("dm:{}", ch),
                        }
                    }),
                ]
            }
            Subscription::PostgresPresences { .. } => Vec::new(),
        })
        .collect()
}

/// Handle a single Realtime message by converting it to an AppEvent.
async fn handle_realtime_message(text: &str, publisher: &AppEventPublisher) {
    let frame: RealtimeFrame = match serde_json::from_str(text) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("Failed to parse Realtime frame: {}", e);
            return;
        }
    };

    match frame.event.as_str() {
        "broadcast" => {
            handle_broadcast(&frame.payload, publisher).await;
        }
        "postgres_changes" => {
            handle_postgres_event(&frame.payload, publisher).await;
        }
        "presence" => {
            // Presence updates — future work
        }
        "response" | "ok" | "error" => {
            // Subscription ack / error — no AppEvent needed
        }
        "join" | "leave" => {
            // Socket lifecycle — no AppEvent needed
        }
        _ => {
            tracing::debug!("Unhandled Realtime event: {}", frame.event);
        }
    }
}

/// Convert a Broadcast event (DM messages) to an AppEvent.
///
/// DM messages arrive as:
/// ```json
/// { "event": "broadcast", "payload": { "channel_id": "...", "message_id": "...", "author_id": "...", "content": "..." } }
/// ```
async fn handle_broadcast(payload: &serde_json::Value, publisher: &AppEventPublisher) {
    if let Some(channel_val) = payload.get("channel_id") {
        if let Some(channel_id) = channel_val.as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .map(Id::<ChannelMarker>::new)
        {
            if let Some(message_id) = payload.get("message_id").and_then(|v| v.as_str())
                .and_then(|s| s.parse::<u64>().ok())
                .map(Id::<MessageMarker>::new)
            {
                let content = payload.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let author_id = payload.get("author_id").and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(Id::<UserMarker>::new);

                let message = build_message_info(
                    channel_id,
                    message_id,
                    content,
                    author_id,
                    payload.get("flags").and_then(|v| v.as_i64()).unwrap_or(0) as u64,
                );

                let event = AppEvent::MessageCreate { message };
                publisher.publish_for_backend(&event).await;
            }
        }
    }
}

/// Convert a postgres_changes event (guild messages) to an AppEvent.
///
/// Postgres change notifications arrive as:
/// ```json
/// {
///   "schema": "public",
///   "table": "messages",
///   "event": "INSERT",
///   "new": { "message_id": 123, "channel_id": 456, "content": "...", ... },
///   "old": { ... }
/// }
/// ```
async fn handle_postgres_event(payload: &serde_json::Value, publisher: &AppEventPublisher) {
    let table = payload.get("table").and_then(|v| v.as_str()).unwrap_or("");
    let change_type = payload.get("event").and_then(|v| v.as_str()).unwrap_or("INSERT");

    if table == "messages" {
        let row = match change_type {
            "DELETE" => &payload["old"],
            _ => &payload["new"],
        };

        if let (Some(channel_id_val), Some(message_id_val)) = (
            row.get("discord_channel_id").and_then(|v| v.as_i64()).map(|v| v as u64),
            row.get("message_id").and_then(|v| v.as_i64()).map(|v| v as u64),
        ) {
            let channel_id = Id::<ChannelMarker>::new(channel_id_val);
            let message_id = Id::<MessageMarker>::new(message_id_val);
            let content = row.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let author_id = row.get("discord_author_id").and_then(|v| v.as_i64())
                .map(|v| Id::<UserMarker>::new(v as u64));
            let flags = row.get("flags").and_then(|v| v.as_i64()).unwrap_or(0) as u64;

            match change_type {
                "INSERT" => {
                    let message = build_message_info(channel_id, message_id, content, author_id, flags);
                    publisher.publish_for_backend(&AppEvent::MessageCreate { message }).await;
                }
                "UPDATE" => {
                    let guild_id = row.get("discord_guild_id").and_then(|v| v.as_i64())
                        .map(|v| Id::<GuildMarker>::new(v as u64));
                    let update = MessageUpdateDispatchInfo {
                        guild_id,
                        channel_id,
                        message_id,
                        fields: MessageUpdateEventFields {
                            content: Some(content),
                            flags: Some(flags),
                            ..Default::default()
                        },
                        extra_fields: std::collections::BTreeMap::new(),
                    };
                    publisher.publish_for_backend(&AppEvent::MessageUpdateDispatch { update }).await;
                }
                "DELETE" => {
                    let guild_id = row.get("discord_guild_id").and_then(|v| v.as_i64())
                        .map(|v| Id::<GuildMarker>::new(v as u64));
                    publisher.publish_for_backend(&AppEvent::MessageDelete {
                        guild_id,
                        channel_id,
                        message_id,
                    }).await;
                }
                _ => {}
            }
        }
    } else if table == "presences" {
        // Presence updates — future work
        let _ = payload;
    }
}

/// Build a `MessageInfo` from Realtime data.
fn build_message_info(
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    content: String,
    author_id: Option<Id<UserMarker>>,
    flags: u64,
) -> MessageInfo {
    MessageInfo {
        channel_id,
        message_id,
        author_id: author_id.unwrap_or_else(|| Id::new(0)),
        author: String::new(),
        content: Some(content),
        flags,
        ..Default::default()
    }
}

/// URL-encode the access token for WebSocket query parameter.
fn urlencoding(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", b));
            }
        }
    }
    result
}
