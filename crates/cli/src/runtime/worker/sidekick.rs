//! Sidekick commands from the UI: start a persistent subagent, or stop one.

use std::sync::Arc;

use orca_harness_core::Model;
use orca_harness_extensions::HarnessEvent;
use orca_harness_tools::SubagentTool;
use tokio::sync::mpsc;

use crate::msg::UiMsg;

pub(super) fn start(
    subagent: &Arc<SubagentTool<Arc<dyn Model>>>,
    ui: &mpsc::UnboundedSender<UiMsg>,
    background_subagent_sequence: &mut u64,
    task: String,
    tier: Option<String>,
) {
    let mut request = orca_harness_tools::SubagentRequest::new(task.clone());
    if let Some(tier) = tier {
        let Some(model) = subagent.max_depth_preferred_model(&tier) else {
            let _ = ui.send(UiMsg::Notice(format!(
                "sidekick start failed: no saved preferred {tier} model"
            )));
            return;
        };
        request = request.model(model);
    }
    let call_id = format!("sidekick-start-{}", background_subagent_sequence);
    *background_subagent_sequence += 1;
    let _ = ui.send(UiMsg::Event(HarnessEvent::ToolCall {
        tool_call_id: call_id.clone(),
        tool_name: "subagent".into(),
        input: serde_json::json!({"task": task, "persistent": true}),
    }));
    match subagent.start_sidekick(request) {
        Ok(ack) => {
            let spawn_id = ack.spawn_id;
            let identity = ack.identity.clone();
            let started_call_id = format!("sidekick-{spawn_id}");
            let _ = ui.send(UiMsg::SubagentStarted {
                id: spawn_id,
                parent_id: None,
                depth: 0,
                call_id: started_call_id,
                task,
                identity,
                run: None,
                stage: None,
            });
            let _ = ui.send(UiMsg::Event(HarnessEvent::ToolResult {
                tool_call_id: call_id,
                tool_name: "subagent".into(),
                output: ack.into_value(),
                is_error: false,
            }));
        }
        Err(error) => {
            let message = format!("sidekick start failed: {error}");
            let _ = ui.send(UiMsg::Event(HarnessEvent::ToolResult {
                tool_call_id: call_id,
                tool_name: "subagent".into(),
                output: serde_json::json!({"error": message}),
                is_error: true,
            }));
            let _ = ui.send(UiMsg::Notice(message));
        }
    }
}

pub(super) fn stop(
    subagent: &Arc<SubagentTool<Arc<dyn Model>>>,
    ui: &mpsc::UnboundedSender<UiMsg>,
    spawn_id: u64,
) {
    match subagent.stop_sidekick(spawn_id) {
        Ok(()) => {
            let _ = ui.send(UiMsg::Event(HarnessEvent::ToolResult {
            tool_call_id: format!("sidekick-stop-{spawn_id}"), tool_name: "subagent".into(),
            output: serde_json::json!({"spawnId": spawn_id, "status": "stopped", "termination": "stopped"}), is_error: false,
        }));
        }
        Err(error) => {
            let _ = ui.send(UiMsg::Notice(format!("sidekick stop failed: {error}")));
        }
    }
}
