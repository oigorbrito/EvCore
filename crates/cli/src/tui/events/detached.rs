//! Bookkeeping for detached subagents and sidekick results, shared by the
//! harness and subagent event handlers.

use super::super::*;
use crate::tui::state::SubagentTranscriptStatus;

pub(super) fn mark_subagent_detached(app: &mut App, id: u64) {
    let terminal = app
        .subagent_transcripts
        .get_mut(&id)
        .is_some_and(|transcript| {
            transcript.detached = true;
            !transcript.status.is_active()
        });
    if terminal {
        app.subagent_activity.remove(&id);
    }
}

pub(super) fn mark_sidekick_result(app: &mut App, output: &serde_json::Value) {
    let Some(id) = output.get("spawnId").and_then(serde_json::Value::as_u64) else {
        return;
    };
    let Some(transcript) = app.subagent_transcripts.get_mut(&id) else {
        return;
    };
    if transcript.status == SubagentTranscriptStatus::Stopped {
        return;
    }
    if output
        .get("persistent")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        transcript.persistent = true;
        transcript.sidekick_established = true;
        transcript.status = match output.get("status").and_then(serde_json::Value::as_str) {
            Some("queued") => SubagentTranscriptStatus::Queued,
            Some("running") | Some("busy") => SubagentTranscriptStatus::Running,
            _ => transcript.status,
        };
        app.invalidate_agent_list();
        return;
    }
    match output
        .get("termination")
        .and_then(serde_json::Value::as_str)
    {
        Some("retained") => {
            transcript.persistent = true;
            transcript.sidekick_established = true;
            transcript.status = SubagentTranscriptStatus::Idle;
        }
        Some("stopped") => {
            transcript.persistent = true;
            transcript.sidekick_established = false;
            transcript.status = SubagentTranscriptStatus::Stopped;
        }
        _ => match output.get("status").and_then(serde_json::Value::as_str) {
            // A successful status lookup itself proves that the runtime
            // returned a reusable handle after initial creation.
            Some("idle") => {
                transcript.persistent = true;
                transcript.sidekick_established = true;
                transcript.status = SubagentTranscriptStatus::Idle;
            }
            Some("busy") => {
                transcript.persistent = true;
                transcript.sidekick_established = true;
                transcript.status = SubagentTranscriptStatus::Running;
            }
            Some("stopped") => {
                transcript.persistent = true;
                transcript.sidekick_established = false;
                transcript.status = SubagentTranscriptStatus::Stopped;
            }
            _ => return,
        },
    }
    transcript.elapsed = Some(transcript.started.elapsed());
    app.invalidate_agent_list();
}

pub(super) fn detached_spawn_id(tool_name: &str, output: &serde_json::Value) -> Option<u64> {
    if matches!(tool_name, "subagent" | "workflow")
        && output
            .get("termination")
            .and_then(serde_json::Value::as_str)
            == Some("detached")
    {
        output
            .get(if tool_name == "workflow" {
                "runId"
            } else {
                "spawnId"
            })
            .and_then(serde_json::Value::as_u64)
    } else {
        None
    }
}
