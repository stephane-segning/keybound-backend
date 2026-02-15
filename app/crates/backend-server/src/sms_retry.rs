use crate::state::AppState;
use backend_model::db;
use backend_repository::{SmsPublishFailure, SmsRepo};
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::warn;

pub fn spawn(state: Arc<AppState>) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            if let Err(e) = tick(&state).await {
                warn!("sms retry tick failed: {e}");
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    })
}

async fn tick(state: &AppState) -> backend_core::Result<()> {
    let rows = state.sms.list_retryable_sms(25).await?;

    for row in rows {
        if let Err(e) = try_publish(state, row).await {
            warn!("sms publish failed: {e}");
        }
    }

    Ok(())
}

async fn try_publish(state: &AppState, row: db::SmsMessageRow) -> backend_core::Result<()> {
    let message = row
        .metadata
        .as_ref()
        .and_then(|v| v.get("message"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if message.is_empty() {
        state
            .sms
            .mark_sms_gave_up(&row.id, "missing message body")
            .await?;
        return Ok(());
    }

    let attempt = row.attempt_count.max(0) as u32 + 1;

    match state
        .sns
        .publish()
        .phone_number(row.phone_number.clone())
        .message(message.to_owned())
        .send()
        .await
    {
        Ok(out) => {
            let message_id = out.message_id().map(|s| s.to_owned());
            state
                .sms
                .mark_sms_sent(&row.id, message_id)
                .await?;
        }
        Err(e) => {
            let max_attempts = row.max_attempts.max(1) as u32;
            let gave_up = attempt >= max_attempts;
            let initial_backoff_seconds = state
                .config
                .sns
                .as_ref()
                .map(|v| v.initial_backoff_seconds)
                .unwrap_or(1);

            let backoff = backoff_seconds(
                initial_backoff_seconds,
                row.attempt_count.max(0) as u32,
            );

            let next_retry_at = if gave_up {
                None
            } else {
                Some(Utc::now() + chrono::Duration::seconds(backoff as i64))
            };

            state
                .sms
                .mark_sms_failed(SmsPublishFailure {
                    id: row.id.clone(),
                    gave_up,
                    error: e.to_string(),
                    next_retry_at,
                })
                .await?;
        }
    }

    Ok(())
}

fn backoff_seconds(initial: u64, attempt_count: u32) -> u64 {
    let base = initial.max(1);
    let factor = 2u64.saturating_pow(attempt_count.min(16));
    base.saturating_mul(factor).min(3600)
}
