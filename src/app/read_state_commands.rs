use crate::backend::BackendClient;
use std::sync::Arc;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, MessageMarker},
};

use super::command_loop::log_app_error;

pub(super) async fn ack_channel(
    client: Arc<dyn BackendClient>,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
) {
    client.clear_read_ack(channel_id);
    client
        .publish_optimistic_read_ack(channel_id, message_id)
        .await;
    // A failure here only loses cross-client sync because the backend
    // has already published the local read state update.
    if let Err(error) = client.ack_channel(channel_id, message_id).await {
        log_app_error("ack channel failed", &error);
    }
}

pub(super) async fn schedule_ack_channel(
    client: Arc<dyn BackendClient>,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
) {
    client
        .publish_optimistic_read_ack(channel_id, message_id)
        .await;
    client.schedule_read_ack(channel_id, message_id, std::time::Instant::now());
}

pub(super) async fn ack_channels(
    client: Arc<dyn BackendClient>,
    targets: Vec<(Id<ChannelMarker>, Id<MessageMarker>)>,
) {
    client.clear_read_acks(targets.iter().map(|(channel_id, _)| *channel_id));
    client.publish_optimistic_read_acks(&targets).await;
    // A failure here only loses cross-client sync because the backend
    // has already published the local read state updates.
    if let Err(error) = client.ack_channels(&targets).await {
        log_app_error("ack channels failed", &error);
    }
}
