//! Best-effort discovery of the active model's context window, reported to the UI.

use tokio::sync::mpsc;

use crate::msg::{Provider, UiMsg};
use crate::Endpoint;

/// Discover the active model's context window from the endpoint, best
/// effort, and report it to the UI. OpenRouter's catalog carries
/// `context_length`; a local ollama exposes it via the native
/// `/api/show`. Plain OpenAI endpoints publish nothing — `None`.
pub(super) fn spawn_window_probe(
    endpoint: &Endpoint,
    ui: mpsc::UnboundedSender<UiMsg>,
    capacity: orca_harness_extensions::ContextCapacity,
) {
    // Never let a newly selected model inherit the previous model's limit,
    // or let a slower probe for an older selection overwrite a newer one.
    let revision = capacity.begin_update();
    let endpoint = endpoint.clone();
    tokio::spawn(async move {
        let window = match endpoint.provider {
            // `/models` lists dated ids, so a configured alias never matches
            // the catalog; resolve the one model instead of paging all of them.
            Provider::Anthropic => orca_harness_model_providers::anthropic::retrieve_model(
                &endpoint.base_url,
                endpoint.api_key.as_deref(),
                &endpoint.model,
            )
            .await
            .ok()
            .and_then(|model| model.context_length),
            Provider::OpenAiCodex
            | Provider::OpenRouter
            | Provider::Vercel
            | Provider::CheaperInference => endpoint
                .list_models()
                .await
                .ok()
                .and_then(|models| {
                    models
                        .into_iter()
                        .find(|candidate| candidate.id == endpoint.model)
                })
                .and_then(|candidate| candidate.context_length),
            Provider::Local => ollama_context_window(&endpoint.base_url, &endpoint.model).await,
            Provider::OpenAi => None,
        };
        if capacity.finish_update(revision, window) {
            let _ = ui.send(UiMsg::ContextWindow(window));
        }
    });
}

/// Ollama native `/api/show`: prefer an explicit `num_ctx` parameter (the
/// serving window) over the model's trained maximum (`*.context_length`).
async fn ollama_context_window(base_url: &str, model: &str) -> Option<u64> {
    let host = base_url.trim_end_matches('/').trim_end_matches("/v1");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()?;
    let body: serde_json::Value = client
        .post(format!("{host}/api/show"))
        .json(&serde_json::json!({"model": model}))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let num_ctx = body["parameters"].as_str().and_then(|params| {
        params.lines().find_map(|line| {
            let mut parts = line.split_whitespace();
            (parts.next() == Some("num_ctx")).then(|| parts.next()?.parse().ok())?
        })
    });
    num_ctx.or_else(|| {
        body["model_info"]
            .as_object()?
            .iter()
            .find_map(|(key, value)| key.ends_with(".context_length").then(|| value.as_u64())?)
    })
}
