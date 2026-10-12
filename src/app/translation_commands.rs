use std::sync::Arc;

use crate::{
    backend::BackendClient,
    discord::{AppEvent, TranslationTarget},
    logging,
    translation::TranslationService,
};

pub(super) async fn translate(
    client: Arc<dyn BackendClient>,
    translation: TranslationService,
    request_id: u64,
    target: TranslationTarget,
    target_language: String,
    content: String,
) {
    let event = match translation.translate(&content, &target_language).await {
        Ok(result) => AppEvent::TranslationCompleted {
            request_id,
            target,
            translated_text: result.text,
        },
        Err(error) => {
            logging::error("translation", error.to_string());
            AppEvent::TranslationFailed {
                request_id,
                target,
                message: error.to_string(),
            }
        }
    };
    client.publish_event(event).await;
}
