//! Provider login commands from the UI: start a login in the background, and
//! apply its result once it finishes.

use std::sync::Arc;

use tokio::sync::mpsc;

use orca_harness_core::{Agent, Model};
use orca_harness_extensions::ContextCapacity;
use orca_harness_tools::SubagentTool;

use super::{rebuild, BuiltAgent};
use crate::msg::{Provider, UiMsg, WorkerCmd};
use crate::{config, spawn_window_probe, Endpoint};

pub(super) fn provider(
    login_attempt: &mut u64,
    login_task: &mut Option<tokio::task::JoinHandle<()>>,
    ui: &mpsc::UnboundedSender<UiMsg>,
    command_tx: &mpsc::UnboundedSender<WorkerCmd>,
    provider: Provider,
) {
    if let Some(task) = login_task.take() {
        task.abort();
    }
    *login_attempt = login_attempt.wrapping_add(1);
    let attempt = *login_attempt;
    let progress_ui = ui.clone();
    let completion_tx = command_tx.clone();
    *login_task = Some(tokio::spawn(async move {
        let result = crate::auth::login(provider, move |message| {
            let _ = progress_ui.send(UiMsg::Notice(message));
        })
        .await;
        let _ = completion_tx.send(WorkerCmd::LoginFinished {
            provider,
            attempt,
            result,
        });
    }));
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn finished<F>(
    agent: &mut Agent<Arc<dyn Model>>,
    subagent: &mut Arc<SubagentTool<Arc<dyn Model>>>,
    build: &mut F,
    endpoint: &mut Endpoint,
    login_attempt: u64,
    login_task: &mut Option<tokio::task::JoinHandle<()>>,
    context_capacity: &ContextCapacity,
    ui: &mpsc::UnboundedSender<UiMsg>,
    provider: Provider,
    attempt: u64,
    result: Result<(), String>,
) where
    F: FnMut(&Endpoint, bool) -> BuiltAgent,
{
    if attempt != login_attempt {
        return;
    }
    *login_task = None;
    match result {
        Ok(()) => {
            let mut candidate = endpoint.clone();
            candidate.provider = provider;
            candidate.base_url = provider.base_url().into();
            candidate.api_key = None;
            candidate.reasoning_effort = None;
            let requested = config::stored_model(provider.label());
            candidate.model = match candidate.catalog_model(requested.as_deref()).await {
                Ok(model) => model,
                Err(error) => {
                    let _ = ui.send(UiMsg::Notice(format!(
                        "login succeeded, but model catalog failed: {error}"
                    )));
                    return;
                }
            };
            *endpoint = candidate;
            let _ = config::save_provider(provider.label());
            (*agent, *subagent) = rebuild(build, &*endpoint, false, subagent);
            spawn_window_probe(&*endpoint, ui.clone(), context_capacity.clone());
            let _ = ui.send(UiMsg::ProviderChanged {
                provider,
                model: endpoint.model.clone(),
            });
        }
        Err(error) => {
            let _ = ui.send(UiMsg::Notice(format!("login failed: {error}")));
        }
    }
}
