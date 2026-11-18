//! Detached review: a child route echoes tasks while a gate proves background work.

use super::*;

/// A dedicated child route echoes rendered tasks; a gate proves detached work
/// can outlive the parent turn without consuming the parent's scripted replies.
struct Reviewer {
    release: CancellationToken,
    calls: AtomicUsize,
}
#[async_trait]
impl Model for Reviewer {
    async fn generate(
        &self,
        ctx: &Context,
        _: &[ToolSchema],
    ) -> std::result::Result<ModelResponse, ModelError> {
        self.release.cancelled().await;
        self.calls.fetch_add(1, Ordering::SeqCst);
        let task = ctx
            .messages()
            .iter()
            .rev()
            .find_map(|m| match m {
                Message::User { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .unwrap();
        Ok(ModelResponse::final_text(if task == "release dimensions" {
            r#"["tests","docs"]"#
        } else {
            task
        }))
    }
}
async fn wait_for(mut condition: impl FnMut() -> bool) -> Result {
    tokio::time::timeout(WAIT, async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    Ok(())
}
pub(super) async fn review(harness: &Harness) -> Result {
    let release = CancellationToken::new();
    let child = Arc::new(Reviewer {
        release: release.clone(),
        calls: AtomicUsize::new(0),
    });
    let parent = Arc::new(ScriptedModel::new(vec![
        ModelResponse::final_text("Review delegated"),
        ModelResponse::final_text("Review reports incorporated"),
    ]));
    let agent = harness
        .agent(parent.clone())
        .tools(ToolPreset::Coding)
        .processes(ProcessConfig::new().max_processes(2))
        .subagents(
            SubagentConfig::new()
                .background_limit(4)
                .model(SubagentModel::new(
                    "local/reviewer",
                    "offline release reviewer",
                    child.clone(),
                )),
        )
        .build()?;
    let session = agent.new_session().persistent().open()?;
    let processes = session.processes().unwrap();
    let result = async {
        let subagents = session.subagents().unwrap();
        assert!(subagents.settings().set_default_model(Some("local/reviewer".into())));
        let mut notifications = session.notifications();
        let ack = subagents.spawn(SubagentRequest::new("audit release checklist"))?;
        assert_eq!(
            session.run("Delegate release review").await?.text,
            "Review delegated"
        );
        assert_eq!(subagents.active().len(), 1);
        release.cancel();
        tokio::time::timeout(WAIT, async {
            loop {
                if let BackgroundNotification::SubagentFinished(n) = notifications.recv().await? {
                    if n.spawn.id == ack.spawn_id {
                        assert_eq!(
                            n.result.unwrap()["answer"],
                            "audit release checklist"
                        );
                        break;
                    }
                }
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        }).await??;
        wait_for(|| session.pending_completions() == 1).await?;

        let workflows = session.workflows().unwrap();
        let mut source = Stage::new("source", "release dimensions");
        source.schema = Some("string[]".into());
        let mut map = Stage::new("checks", "check {{ item }}");
        map.kind = Kind::Map;
        map.over = Some("source".into());
        let mut summary = Stage::new("summary", "review: {{ stages.checks.output }}");
        summary.needs = vec!["checks".into()];
        let workflow = workflows
            .submit(
                WorkflowSubmission::new([source, map, summary])
                    .max_stages(8)
                    .timeout(WAIT),
            )?;
        wait_for(|| {
            workflows
                .status(workflow.run_id)
                .is_some_and(|s| s.outcome.is_some())
                && session.pending_completions() == 2
        })
        .await?;
        let status = workflows.status(workflow.run_id).unwrap();
        assert_eq!(status.state, RunState::Done);
        let outcome = status.outcome.unwrap();
        assert_eq!(outcome.outputs["summary"], r#"review: ["check tests","check docs"]"#);
        assert_eq!(
            workflows.stage_output(workflow.run_id, "summary")?.answer,
            outcome.outputs["summary"]
        );
        assert_eq!(child.calls.load(Ordering::SeqCst), 5);
        assert_eq!(
            session.continue_run(RunRequest::continuation()).await?.text,
            "Review reports incorporated"
        );
        assert_eq!(session.pending_completions(), 0);
        let transcript = serde_json::to_string(&session.messages().await)?;
        assert!(transcript.contains("background_subagent_completions"));
        assert!(transcript.contains("audit release checklist"));
        assert!(transcript.contains("check tests"));
        tokio::time::timeout(WAIT, async {
            loop {
                if let BackgroundNotification::CompletionsDelivered { spawn_ids } =
                    notifications.recv().await?
                {
                    assert_eq!(spawn_ids.len(), 2);
                    break;
                }
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        }).await??;
        assert_eq!(parent.observed_contexts().len(), 2);

        let process = processes
            .spawn(
                ProcessSpawn::new(
                    "read start; printf 'release-ready\\n'; read finish; printf 'release-done\\n'",
                )
                .notify_on_match("release-ready"),
                None,
            )
            .await?;
        // Register first, then let the process emit readiness: no startup race.
        processes.write(&process.id, orca_harness_sdk::orchestration::ProcessWrite::new("start")).await?;
        // Readiness is explicitly synchronized via stdin rather than sleeps.
        let mut ready = false;
        tokio::time::timeout(WAIT, async {
            loop {
                if let BackgroundNotification::ProcessNotified(n) = notifications.recv().await? {
                    if n.id != process.id { continue; }
                    match n.kind {
                        ProcessNotificationKind::OutputMatch { pattern } => {
                            assert_eq!(pattern, "release-ready"); ready = true;
                            processes
                                .write(
                                    &process.id,
                                    orca_harness_sdk::orchestration::ProcessWrite::new("finish"),
                                )
                                .await?;
                        }
                        ProcessNotificationKind::Exit { exit_code } => {
                            assert!(ready);
                            assert_eq!(exit_code, Some(0));
                            break;
                        }
                    }
                }
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        }).await??;
        println!("Dedicated review route, dependency/map outputs, completion delivery, OS readiness/exit asserted.");
        Ok(())
    }.await;
    session.shutdown(WAIT).await?;
    assert!(!processes.is_open());
    assert!(session.run("after shutdown").await.is_err());
    result
}
