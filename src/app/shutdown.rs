use std::sync::Arc;

use tokio::time::{Duration, sleep};

use crate::{backend::BackendClient, logging};

pub(super) async fn shutdown_backend(
    _client: &Arc<dyn BackendClient>,
    mut gateway_task: tokio::task::JoinHandle<()>,
) {
    // For the Supabase backend there is no gateway task to abort explicitly —
    // the background polled task from .start() owns the connection. Aborting
    // the task signals it to stop.
    gateway_task.abort();

    tokio::select! {
        result = &mut gateway_task => {
            if let Err(error) = result
                && !error.is_cancelled()
            {
                logging::error("app", format!("backend task ended unexpectedly: {error}"));
            }
        }
        () = sleep(Duration::from_secs(2)) => {
            gateway_task.abort();
            if let Err(error) = gateway_task.await
                && !error.is_cancelled()
            {
                logging::error("app", format!("backend task ended unexpectedly: {error}"));
            }
        }
    }
}
