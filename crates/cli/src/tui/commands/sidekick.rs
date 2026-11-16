use tokio::sync::mpsc;

use crate::msg::WorkerCmd;
use crate::tui::components::picker::ListPicker;
use super::super::state::{App, Overlay};
use super::super::{push_error, push_notice};

pub(super) fn sidekick_command(app: &mut App, args: &str, worker: &mpsc::UnboundedSender<WorkerCmd>) {
    const USAGE: &str = "usage: /sidekick [local|fast|mid|frontier] <task> | /sidekick stop";
    let mut parts = args.split_whitespace();
    let Some(first) = parts.next() else {
        push_error(app, USAGE);
        return;
    };
    if first == "stop" {
        if parts.next().is_some() {
            push_error(app, USAGE);
            return;
        }
        let mut ids = app
            .subagent_transcripts
            .values()
            .filter(|sidekick| {
                sidekick.persistent
                    && matches!(
                        sidekick.status,
                        crate::tui::state::SubagentTranscriptStatus::Queued
                            | crate::tui::state::SubagentTranscriptStatus::Running
                            | crate::tui::state::SubagentTranscriptStatus::Idle
                    )
            })
            .map(|sidekick| sidekick.id)
            .collect::<Vec<_>>();
        ids.sort_unstable();
        if ids.is_empty() {
            push_notice(app, "No active sidekicks.");
        } else {
            app.overlay = Some(Overlay::SidekickStop {
                picker: ListPicker::new(ids.len()),
                ids,
            });
        }
        return;
    }
    let (tier, task) = match first {
        "local" | "mid" | "frontier" => {
            (Some(first.to_string()), parts.collect::<Vec<_>>().join(" "))
        }
        "fast" => (
            Some("flash".to_string()),
            parts.collect::<Vec<_>>().join(" "),
        ),
        _ => (None, args.to_string()),
    };
    if task.trim().is_empty() {
        push_error(app, USAGE);
        return;
    }
    if worker
        .send(WorkerCmd::SidekickStart { task, tier })
        .is_err()
    {
        push_error(app, "worker is gone; restart the application");
    }
}
