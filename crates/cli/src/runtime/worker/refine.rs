//! Refine: a one-shot proposer pass over the conversation, then an approval
//! gate before any skill is written. Returns `false` when the UI is gone and
//! the worker must stop.

use tokio::sync::mpsc;

use orca_harness_core::Context;

use super::workspace_relative;
use crate::msg::UiMsg;
use crate::{skills, Endpoint};

pub(super) async fn run(
    endpoint: &Endpoint,
    skills: &skills::Skills,
    context: &Context,
    ui: &mpsc::UnboundedSender<UiMsg>,
    refine_applied: &mut Vec<crate::refine::Applied>,
) -> bool {
    // A one-shot proposer pass over a scratch context: the
    // user's conversation is read, never written. The model
    // is built fresh from the live endpoint so provider and
    // model switches are always respected.
    let model = endpoint.build_model_for_ui(Some(ui.clone()));
    let existing: Vec<(String, String)> = skills
        .catalog()
        .into_iter()
        .map(|entry| (entry.name, entry.description))
        .collect();
    let result = crate::refine::run_proposer(model.as_ref(), context.messages(), &existing).await;
    let result = match result {
        // Declining is success, not silence: say why and stop.
        Ok(crate::refine::Refined::Nothing(reason)) => {
            let reason = match reason.is_empty() {
                true => String::new(),
                false => format!(": {reason}"),
            };
            let _ = ui.send(UiMsg::Notice(format!(
                "refine: no skill proposed — nothing in this trajectory is worth \
                 packaging{reason}"
            )));
            return true;
        }
        Ok(crate::refine::Refined::Skill(outcome)) => Ok(outcome),
        Err(err) => Err(err),
    };
    let proposal = result.as_ref().ok().map(|outcome| outcome.proposal.clone());
    if ui.send(UiMsg::RefineDone(Box::new(result))).is_err() {
        return false;
    }
    let Some(proposal) = proposal else {
        return true;
    };
    // Accept/reject rides the standard tool-approval gate
    // (y/n/a/A) rather than a bespoke prompt.
    let (respond, decision) = tokio::sync::oneshot::channel();
    let request = crate::msg::ApprovalRequest {
        tool_name: "refine".into(),
        detail: format!("apply skill {}", proposal.name),
        yes_no: true,
        respond,
    };
    if ui.send(UiMsg::Approval(request)).is_err() {
        return false;
    }
    use crate::msg::ApprovalResponse as R;
    let approved = matches!(
        decision.await,
        Ok(R::AllowOnce | R::AllowAlways | R::AllowAlwaysSave)
    );
    if !approved {
        let _ = ui.send(UiMsg::Notice(format!(
            "refine: proposal {} discarded",
            proposal.name
        )));
        return true;
    }
    let applied = skills
        .project_root()
        .map(std::path::Path::to_path_buf)
        .ok_or_else(|| "no project skills folder to apply into".to_string())
        .and_then(|root| crate::refine::apply(&root, &proposal));
    let notice = match applied {
        Ok(applied) => {
            skills.reload();
            let notice = format!(
                "applied skill {} → {} · /refine undo reverts it",
                proposal.name,
                workspace_relative(&applied.dir)
            );
            refine_applied.push(applied);
            notice
        }
        Err(err) => format!("refine apply failed: {err}"),
    };
    let _ = ui.send(UiMsg::Notice(notice));
    true
}
