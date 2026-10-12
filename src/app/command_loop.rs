use std::sync::Arc;

use crate::{
    discord::AppEvent,
    error::AppError,
    logging,
    backend::BackendClient,
};

pub(super) fn log_app_error(context: &str, error: &AppError) {
    logging::error(
        "app",
        format!("{context}: {}; detail={}", error, error.log_detail()),
    );
}

pub(super) async fn publish_app_error(
    client: &Arc<dyn BackendClient>,
    context: &str,
    error: &AppError,
) {
    log_app_error(context, error);
    let event = match error {
        AppError::CaptchaRequired { action } => AppEvent::CaptchaRequired {
            action: action.clone(),
        },
        _ => AppEvent::GatewayError {
            message: format!("{context}: {error}"),
        },
    };
    client.publish_event(event).await;
}
