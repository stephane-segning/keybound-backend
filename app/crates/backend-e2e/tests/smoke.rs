mod common;

use anyhow::Result;
use common::{Env, http_client, send_json, wait_for_status};
use reqwest::Method;
use serde_json::Value;

#[tokio::test]
async fn smoke_services_are_reachable() -> Result<()> {
    let env = Env::from_env()?;
    let client = http_client()?;

    wait_for_status(
        &client,
        &format!("{}/health", env.user_storage_url),
        200,
        60,
    )
    .await?;
    wait_for_status(
        &client,
        &format!("{}/realms/e2e-testing", env.keycloak_url),
        200,
        60,
    )
    .await?;
    wait_for_status(
        &client,
        &format!("{}/__admin/requests", env.cuss_url),
        200,
        60,
    )
    .await?;

    let sms_reset = send_json(
        &client,
        Method::POST,
        &format!("{}/__admin/reset", env.sms_sink_url),
        None,
        Some(serde_json::json!({})),
    )
    .await?;
    assert_eq!(sms_reset.status, 200);
    assert_eq!(
        sms_reset
            .body
            .as_ref()
            .and_then(|body| body.get("reset"))
            .and_then(Value::as_bool),
        Some(true)
    );

    Ok(())
}
