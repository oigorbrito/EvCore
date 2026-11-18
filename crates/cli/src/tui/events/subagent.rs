//! Folds subagent lifecycle events into the transcript and workflow stages.

use super::super::*;
use crate::tui::state::subagent_history::{
    append_display_text, bounded_text, compact_activity, display_value,
};
use crate::tui::state::workflow::StageState;
use crate::tui::state::{
    SpawnActivity, SubagentTranscript, SubagentTranscriptEntry, SubagentTranscriptStatus,
};
use orca_harness_extensions::HarnessEvent;

use super::detached::{detached_spawn_id, mark_subagent_detached};

pub(crate) fn handle_subagent_event(
    app: &mut App,
    id: u64,
    parent_id: Option<u64>,
    depth: u32,
    call_id: String,
    event: HarnessEvent,
) {
    if let HarnessEvent::ToolResult {
        tool_name, output, ..
    } = &event
    {
        if let Some(child_id) = detached_spawn_id(tool_name, output) {
            mark_subagent_detached(app, child_id);
        }
    }
    // Nested worker updates can change identity rows inside the selected history.
    if let Some(browser) = &mut app.agent_browser {
        browser.body_cache = None;
    }
    record_subagent_event(app, id, parent_id, depth, &call_id, &event);
    match event {
        HarnessEvent::ToolCall {
            tool_call_id,
            tool_name,
            input,
        } => {
            let spawn = app
                .subagent_activity
                .entry(id)
                .or_insert_with(|| SpawnActivity {
                    call_id,
                    parent_id,
                    depth,
                    task: String::new(),
                    identity: None,
                    tools: Vec::new(),
                    omitted_tools: 0,
                    pending: std::collections::HashMap::new(),
                });
            let index = spawn.tools.len();
            spawn.tools.push(ToolActivity::new(
                tool_call_id.clone(),
                tool_name,
                display_value(&input),
            ));
            spawn.pending.insert(tool_call_id, index);
        }
        HarnessEvent::ToolStarted { tool_call_id, .. } => {
            if let Some(spawn) = app.subagent_activity.get_mut(&id) {
                if let Some(index) = spawn.pending.get(&tool_call_id).copied() {
                    if let Some(tool) = spawn.tools.get_mut(index) {
                        tool.execution_started();
                    }
                }
            }
        }
        HarnessEvent::ToolFinished {
            tool_call_id,
            is_error,
            ..
        } => {
            if let Some(spawn) = app.subagent_activity.get_mut(&id) {
                if let Some(index) = spawn.pending.get(&tool_call_id).copied() {
                    if let Some(tool) = spawn.tools.get_mut(index) {
                        tool.finish(is_error);
                    }
                }
            }
        }
        HarnessEvent::ToolResult {
            tool_call_id,
            output,
            is_error,
            ..
        } => {
            if let Some(spawn) = app.subagent_activity.get_mut(&id) {
                if let Some(index) = spawn.pending.remove(&tool_call_id) {
                    if let Some(tool) = spawn.tools.get_mut(index) {
                        tool.record_result(display_value(&output), is_error);
                    }
                }
            }
        }
        _ => {}
    }
    if let Some(spawn) = app.subagent_activity.get_mut(&id) {
        spawn.omitted_tools = spawn
            .omitted_tools
            .saturating_add(compact_activity(&mut spawn.tools, &mut spawn.pending));
    }
}

fn record_subagent_event(
    app: &mut App,
    id: u64,
    parent_id: Option<u64>,
    depth: u32,
    call_id: &str,
    event: &HarnessEvent,
) {
    let previous_status = app.subagent_transcripts.get(&id).map(|t| t.status);
    // A sidekick can fail before its first retained report reaches the parent.
    // The initiating tool call is already in the parent activity, so it is the
    // earliest authoritative UI-side indication that this spawn is persistent.
    let initiated_persistent = app.activity_tools.iter().any(|tool| {
        tool.call_id == call_id
            && tool.tool_name == "subagent"
            && tool
                .input
                .get("persistent")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
    });
    let transcript = app.subagent_transcripts.entry(id).or_insert_with(|| {
        SubagentTranscript::new(
            id,
            parent_id,
            depth,
            call_id.to_string(),
            String::new(),
            None,
        )
    });
    transcript.persistent |= initiated_persistent;
    // Stop is terminal for the handle. Child events already in flight still
    // belong in the transcript, but must not resurrect or reclassify it.
    let stopped = transcript.status == SubagentTranscriptStatus::Stopped;
    // The first event out of the inner loop (`AgentStart`, normally) means
    // a queued background worker got its slot. Elapsed restarts so the row
    // times the run rather than the wait.
    if !stopped
        && (transcript.status == SubagentTranscriptStatus::Queued
            || transcript.status == SubagentTranscriptStatus::Idle
            || (transcript.sidekick_established
                && transcript.status == SubagentTranscriptStatus::Failed))
    {
        transcript.status = SubagentTranscriptStatus::Running;
        transcript.started = Instant::now();
    }
    match event {
        HarnessEvent::AssistantDelta { text } => {
            if transcript.streaming_assistant.is_empty() && !text.is_empty() {
                transcript.commit_settled_activity();
            }
            append_display_text(&mut transcript.streaming_assistant, text);
        }
        HarnessEvent::ReasoningDelta { text } => {
            if transcript.streaming_reasoning.is_empty() && !text.is_empty() {
                transcript.commit_settled_activity();
                transcript.reasoning_started = Some(Instant::now());
            }
            append_display_text(&mut transcript.streaming_reasoning, text);
        }
        HarnessEvent::Assistant { message } => {
            transcript.commit_activity();
            let streamed = std::mem::take(&mut transcript.streaming_assistant);
            transcript.push_assistant(if message.is_empty() {
                streamed
            } else {
                message.clone()
            });
        }
        HarnessEvent::ToolCall {
            tool_call_id,
            tool_name,
            input,
        } => {
            transcript.flush_reasoning();
            let streamed = std::mem::take(&mut transcript.streaming_assistant);
            transcript.push_assistant(streamed);
            let index = transcript.activity_tools.len();
            transcript.activity_tools.push(ToolActivity::new(
                tool_call_id.clone(),
                tool_name.clone(),
                display_value(input),
            ));
            transcript.pending_calls.insert(tool_call_id.clone(), index);
        }
        HarnessEvent::ToolStarted { tool_call_id, .. } => {
            if let Some(index) = transcript.pending_calls.get(tool_call_id).copied() {
                if let Some(tool) = transcript.activity_tools.get_mut(index) {
                    tool.execution_started();
                }
            }
        }
        HarnessEvent::ToolFinished {
            tool_call_id,
            is_error,
            ..
        } => {
            if let Some(index) = transcript.pending_calls.get(tool_call_id).copied() {
                if let Some(tool) = transcript.activity_tools.get_mut(index) {
                    tool.finish(*is_error);
                }
            }
        }
        HarnessEvent::ToolResult {
            tool_call_id,
            output,
            is_error,
            ..
        } => {
            if let Some(index) = transcript.pending_calls.remove(tool_call_id) {
                if let Some(tool) = transcript.activity_tools.get_mut(index) {
                    tool.record_result(display_value(output), *is_error);
                }
            }
        }
        HarnessEvent::Usage { usage } => {
            transcript.input_tokens = transcript.input_tokens.saturating_add(usage.input_tokens);
            transcript.output_tokens = transcript.output_tokens.saturating_add(usage.output_tokens);
            if let Some(tokens) = usage.reasoning_tokens {
                transcript.reasoning_tokens = Some(
                    transcript
                        .reasoning_tokens
                        .unwrap_or(0)
                        .saturating_add(tokens),
                );
            }
        }
        HarnessEvent::Result { message } => {
            transcript.finish_activity();
            let streamed = std::mem::take(&mut transcript.streaming_assistant);
            transcript.push_assistant(if message.is_empty() {
                streamed
            } else {
                message.clone()
            });
            if !stopped {
                transcript.status = if transcript.sidekick_established {
                    SubagentTranscriptStatus::Idle
                } else {
                    SubagentTranscriptStatus::Completed
                };
            }
            transcript.elapsed = Some(transcript.started.elapsed());
        }
        HarnessEvent::Error { message } => {
            transcript.finish_activity();
            let streamed = std::mem::take(&mut transcript.streaming_assistant);
            transcript.push_assistant(streamed);
            transcript.push_entry(SubagentTranscriptEntry::Error(bounded_text(message)));
            // A persistent sidekick retains its handle after a failed or
            // cancelled task. The Error entry records that task's failure;
            // Idle says the sidekick itself can accept another task.
            if !stopped {
                transcript.status = if transcript.sidekick_established {
                    SubagentTranscriptStatus::Idle
                } else {
                    SubagentTranscriptStatus::Failed
                };
            }
            transcript.elapsed = Some(transcript.started.elapsed());
        }
        HarnessEvent::AgentStart | HarnessEvent::ToolInputDelta { .. } => {}
    }
    transcript.compact_activity();
    let status_changed = previous_status != Some(transcript.status);
    let status = transcript.status;
    let terminal = !transcript.status.is_active();
    let terminal_detached = transcript.detached && !transcript.status.is_active();
    if terminal_detached {
        app.subagent_activity.remove(&id);
    }
    if status_changed {
        advance_workflow_stage(app, id, status, None);
        app.invalidate_agent_list();
    }
    if terminal {
        app.retain_agent_history();
    }
}

/// Join a spawned agent to the graph stage it executes. Called for workflow
/// stages only; ordinary subagents have no stage to bind.
pub(crate) fn bind_workflow_stage(app: &mut App, spawn: u64, run: u64, stage: &str) {
    let Some(workflow) = app.workflows.get_mut(&run) else {
        return;
    };
    let Some(at) = workflow.position(stage) else {
        return;
    };
    workflow.spawned(stage, spawn);
    app.workflow_stages.insert(spawn, (run, at));
}

/// Mirror an executing agent's status onto the stage it runs, so the graph
/// panel and the agent list can never disagree about one stage.
fn advance_workflow_stage(
    app: &mut App,
    spawn: u64,
    status: SubagentTranscriptStatus,
    detail: Option<String>,
) {
    let Some((run, at)) = app.workflow_stages.get(&spawn).copied() else {
        return;
    };
    let Some(workflow) = app.workflows.get_mut(&run) else {
        return;
    };
    let state = match status {
        SubagentTranscriptStatus::Queued => StageState::Queued,
        SubagentTranscriptStatus::Running => StageState::Running,
        SubagentTranscriptStatus::Idle | SubagentTranscriptStatus::Stopped => StageState::Done,
        SubagentTranscriptStatus::Completed => StageState::Done,
        SubagentTranscriptStatus::Failed => StageState::Failed,
    };
    workflow.advance(at, state, detail);
}
