//! The checklist phases: prepare the release checklist, then recover and archive it.

use super::*;

pub(super) async fn prepare(harness: &Harness, mcp: Mcp) -> Result<String> {
    let memory = harness.memory()?;
    let preference = memory.save(
        "Rust release checklist must stay concise",
        "preference",
        false,
        "host",
    )?;
    let draft = memory.save("draft release", "workflow", false, "host")?;
    assert_eq!(
        memory
            .update(&draft.id, "review release", None)?
            .unwrap()
            .content,
        "review release"
    );
    assert!(memory.forget(&draft.id)?);
    assert_eq!(memory.list(10)?.len(), 1);
    assert_eq!(memory.search("Rust", 5)?[0].id, preference.id);

    // Explicit roots exclude the user's real home skills.
    let skills = Skills::new(
        harness.workspace().root(),
        Some(harness.state_dir().into()),
        None,
    );
    let path = skills.scaffold("release_checklist", SkillDestination::Workspace)?;
    std::fs::write(
        path,
        "---\nname: release_checklist\ndescription: Review a Rust release\n---\nRun tests before tagging.\n",
    )?;
    assert_eq!(skills.reload().skills.len(), 1);
    skills.disable("release_checklist");
    assert!(skills.tool().is_none());
    skills.enable("release_checklist");

    let attempts = Arc::new(AtomicUsize::new(0));
    let counter = attempts.clone();
    let collect = FnTool::new(
        "collect_checks",
        "Collect release evidence (idempotent)",
        json!({"type":"object"}),
        move |_, _| {
            let n = counter.fetch_add(1, Ordering::SeqCst);
            async move {
                if n == 0 {
                    Err(ToolError::msg("temporary evidence-store failure"))
                } else {
                    Ok(json!({"evidence":"tests passed; ".repeat(80)}))
                }
            }
        },
    );
    let forbidden_calls = Arc::new(AtomicUsize::new(0));
    let counter = forbidden_calls.clone();
    let publish = FnTool::new(
        "publish_release",
        "Publish a release",
        json!({"type":"object"}),
        move |_, _| {
            counter.fetch_add(1, Ordering::SeqCst);
            async { Ok(json!({"published":true})) }
        },
    );
    let model = Arc::new(ScriptedModel::new(vec![
        step(vec![
            call("skill", "skill", json!({"name":"release_checklist"})),
            call("evidence", "collect_checks", json!({})),
            call("denied", "publish_release", json!({})),
            call(
                "select",
                "mcp_select_tool",
                json!({"name":"mcp__release__check"}),
            ),
        ]),
        step(vec![call(
            "tag",
            "mcp__release__check",
            json!({"tag":"v1.0"}),
        )]),
        ModelResponse::final_text("Release checklist prepared; publication requires approval."),
    ]));
    let agent = harness
        .agent(model.clone())
        .name("release-host")
        .system_prompt("Prepare a concise Rust release checklist.")
        .tool(collect)
        .tool(publish)
        .policy(ToolPolicy::new().rule(|c: &ToolCall| {
            if c.name == "publish_release" {
                PolicyOutcome::Deny("host approval required".into())
            } else {
                PolicyOutcome::Allow
            }
        }))
        .tool_retry(RetryConfig::attempts(2).backoff_ms(0))
        .truncation(TruncationConfig {
            max_string_chars: 128,
            ..Default::default()
        })
        .memory(MemoryConfig::read_write(memory))
        .skills(skills)
        .mcp(mcp)
        .events(true)
        .usage(true)
        .build()?;
    let session = agent.new_session().persistent().open()?;
    let id = session.id().unwrap();
    let result = async {
        let mut handle = session
            .start(RunRequest::new("Prepare the Rust release checklist").event_capacity(128))?;
        let mut events = handle.take_events().unwrap();
        let reader = tokio::spawn(async move {
            let mut count = 0;
            while let Some(event) = events.recv().await {
                match event {
                    RunEvent::Harness(_) => count += 1,
                    RunEvent::Overflow { .. } => panic!("unexpected overflow"),
                }
            }
            count
        });
        let outcome = tokio::time::timeout(WAIT, handle.outcome()).await??;
        assert!(reader.await? > 0);
        assert_eq!(outcome.dropped_events, 0);
        let result = outcome.into_result()?;
        // Only responses carrying Usage are metered.
        assert_eq!(result.metered_steps, 2);
        assert_eq!(result.usage.input_tokens, 20);
        assert_eq!(result.usage.output_tokens, 6);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert_eq!(forbidden_calls.load(Ordering::SeqCst), 0);
        let transcript = serde_json::to_string(&result.messages)?;
        assert!(transcript.contains("host approval required"));
        assert!(tool_output(&result.messages, "skill")
            .to_string()
            .contains("Run tests before tagging"));
        assert!(tool_output(&result.messages, "tag")
            .to_string()
            .contains("verified tag v1.0"));
        assert!(transcript.contains("_readFull"));
        assert!(model
            .observed_contexts()
            .iter()
            .any(|ctx| serde_json::to_string(ctx.messages())
                .unwrap()
                .contains(&preference.content)));
        assert!(harness.sessions().list().iter().any(|s| s.meta.id == id));
        println!("Prepared: policy denied publication, evidence retried, skill and MCP loaded.");
        Ok::<_, Box<dyn std::error::Error>>(id)
    }
    .await;
    session.shutdown(WAIT).await?;
    result
}

/// Resume on a new agent, recover the untruncated evidence, then compact a
/// separate branch. The original persistent conversation remains unchanged.
pub(super) async fn recover_and_archive(harness: &Harness, id: &str) -> Result {
    let agent = harness
        .agent(ScriptedModel::tool_round(
            vec![call(
                "recover",
                "read_tool_result",
                json!({"callId":"evidence","maxChars":4096}),
            )],
            "Evidence recovered",
        ))
        .compaction(Compaction::Manual)
        .build()?;
    let session = agent.resume_session(id)?;
    let result = async {
        let result = session.run("Recover release evidence").await?;
        assert!(tool_output(&result.messages, "recover")
            .to_string()
            .contains(&"tests passed; ".repeat(80)));
        let before = session.messages().await;
        let fork = session.fork().await?;
        assert_ne!(fork.id(), session.id());
        let report = fork
            .compact(CompactConfig {
                tail_budget_tokens: 10,
            })
            .await?;
        assert!(report.messages_after < report.messages_before);
        assert!(!report.summary.is_empty());
        assert_eq!(
            serde_json::to_value(session.messages().await)?,
            serde_json::to_value(before)?
        );
        fork.shutdown(WAIT).await?;
        println!("Resumed evidence and compacted an isolated archive fork.");
        Ok(())
    }
    .await;
    session.shutdown(WAIT).await?;
    result
}
