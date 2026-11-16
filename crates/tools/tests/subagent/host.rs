// Typed host operations must behave exactly like the model tool: same
// routing validation, admission, spawn ids, nesting, and cancellation.

use orca_harness_tools::{BackgroundStatus, SubagentOutcome, SubagentRequest, WorkerTiming};

#[test]
fn timing_zero_defaults_have_a_stable_serialized_shape() {
    let value = SubagentOutcome {
        answer: "done".into(),
        input_tokens: 0,
        output_tokens: 0,
        reasoning_tokens: None,
        cache_read_tokens: 0,
        cache_create_tokens: 0,
        runtime_ms: 0,
        steps: 0,
        tool_calls: 0,
        timing: WorkerTiming::default(),
        identity: None,
    }
    .into_value();

    assert_eq!(
        value["timing"],
        json!({
            "modelCallElapsedMs": [],
            "modelCumulativeMs": 0,
            "toolCallElapsed": [],
            "toolCumulativeMs": 0,
        })
    );
}

fn scripted_final(text: &str) -> Arc<ScriptedModel> {
    Arc::new(ScriptedModel::new(vec![
        ModelResponse::Final {
            text: text.into(),
            usage: Some(Usage {
                input_tokens: 11,
                output_tokens: 7,
                reasoning_tokens: Some(3),
                cache_read_tokens: 19,
                cache_create_tokens: 23,
                ..Default::default()
            }),
        },
        ModelResponse::Final {
            text: text.into(),
            usage: Some(Usage {
                input_tokens: 11,
                output_tokens: 7,
                reasoning_tokens: Some(3),
                cache_read_tokens: 19,
                cache_create_tokens: 23,
                ..Default::default()
            }),
        },
    ]))
}

#[tokio::test]
async fn typed_foreground_run_matches_the_tool_result() {
    let (ws, _dir) = temp_ws();
    let tool = SubagentTool::new(scripted_final("same answer"), &ws)
        .inherited_identity("test", "worker");

    let via_tool = tool
        .call(json!({"task": "compare"}), &ctx())
        .await
        .unwrap();
    let typed = tool
        .run_foreground(
            SubagentRequest::new("compare"),
            CancellationToken::new(),
            None,
        )
        .await
        .unwrap();

    assert_eq!(typed.answer, "same answer");
    assert_eq!(typed.reasoning_tokens, Some(3));
    assert_eq!(typed.cache_read_tokens, 19);
    assert_eq!(typed.cache_create_tokens, 23);
    assert_eq!(typed.identity.as_ref().unwrap().model, "worker");
    let mut typed_json = typed.into_value();
    let mut tool_json = via_tool;
    // Wall-clock fields legitimately differ between these separate runs.
    typed_json["runtimeMs"] = json!(0);
    tool_json["runtimeMs"] = json!(0);
    typed_json["timing"] = json!(null);
    tool_json["timing"] = json!(null);
    assert_eq!(typed_json, tool_json);
}

#[tokio::test]
async fn typed_background_spawn_shares_spawn_ids_and_notifies() {
    let release = CancellationToken::new();
    let (ws, _dir) = temp_ws();
    let manager = orca_harness_tools::SubagentManager::new(2);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let tool = SubagentTool::new(
        Arc::new(GateModel {
            release: release.clone(),
        }),
        &ws,
    )
    .background(manager.clone(), move |notification| {
        let _ = tx.send(notification);
    });
    assert!(tool.has_background());

    let via_tool = tool
        .call(json!({"task": "model job", "background": true}), &ctx())
        .await
        .unwrap();
    let typed = tool
        .spawn_background(SubagentRequest::new("host job"))
        .unwrap();
    assert_eq!(via_tool["spawnId"], 0);
    assert_eq!(typed.spawn_id, 1);
    assert_eq!(typed.status, BackgroundStatus::Running);
    assert_eq!(
        typed.clone().into_value(),
        json!({"spawnId": 1, "status": "running", "termination": "detached", "identity": null})
    );
    assert_eq!(tool.active_jobs().len(), 2);
    assert_eq!(manager.active().len(), 2);

    release.cancel();
    let mut completions = Vec::new();
    for _ in 0..2 {
        completions.push(
            tokio::time::timeout(Duration::from_secs(1), rx.recv())
                .await
                .expect("completion notification timed out")
                .expect("completion channel closed"),
        );
    }
    completions.sort_by_key(|notification| notification.spawn.id);
    assert_eq!(completions[0].spawn.call_id, "t");
    assert_eq!(completions[1].spawn.call_id, "host:1");
    assert_eq!(completions[1].spawn.task, "host job");
    assert_eq!(
        completions[1].result.as_ref().unwrap()["answer"],
        "background complete"
    );
    assert!(tool.active_jobs().is_empty());
}

#[tokio::test]
async fn typed_spawns_queue_under_the_same_limit_as_the_tool() {
    let started = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let release = CancellationToken::new();
    let (ws, _dir) = temp_ws();
    let manager = orca_harness_tools::SubagentManager::new(1);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let tool = SubagentTool::new(
        Arc::new(CountingGateModel {
            started: started.clone(),
            release: release.clone(),
        }),
        &ws,
    )
    .background(manager, move |notification| {
        let _ = tx.send(notification);
    });

    let first = tool.spawn_background(SubagentRequest::new("one")).unwrap();
    let second = tool.spawn_background(SubagentRequest::new("two")).unwrap();
    let third = tool
        .call(json!({"task": "three", "background": true}), &ctx())
        .await
        .unwrap();
    assert_eq!(first.status, BackgroundStatus::Running);
    assert_eq!(second.status, BackgroundStatus::Queued);
    assert_eq!(third["status"], "queued");
    let jobs = tool.active_jobs();
    assert_eq!(
        jobs.iter().map(|job| job.status).collect::<Vec<_>>(),
        [
            BackgroundStatus::Running,
            BackgroundStatus::Queued,
            BackgroundStatus::Queued
        ]
    );
    // Negative check: a too-short sleep can only pass spuriously, never fail.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(started.load(std::sync::atomic::Ordering::SeqCst), 1);

    release.cancel();
    for _ in 0..3 {
        tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("completion notification timed out")
            .expect("completion channel closed");
    }
    assert_eq!(started.load(std::sync::atomic::Ordering::SeqCst), 3);
}

#[tokio::test]
async fn typed_routing_validation_matches_the_tool() {
    let model = Arc::new(ScriptedModel::new(vec![]));
    let (ws, _dir) = temp_ws();
    let tool = SubagentTool::new(model.clone(), &ws).models([SubagentModel::new(
        "flash/test",
        "fast",
        model.clone(),
    )]);

    for requested in ["flash/test", "unknown/model"] {
        let via_tool = tool
            .call(json!({"task": "route", "model": requested}), &ctx())
            .await
            .unwrap_err()
            .to_string();
        let typed = tool
            .run_foreground(
                SubagentRequest::new("route").model(requested),
                CancellationToken::new(),
                None,
            )
            .await
            .unwrap_err()
            .to_string();
        assert_eq!(typed, via_tool, "{requested}");
        assert!(typed.contains(requested), "{typed}");
    }
    assert_eq!(model.generate_calls(), 0, "rejected routes never run");
}

#[tokio::test]
async fn typed_background_spawn_is_rejected_like_the_tool_without_a_host() {
    let model = Arc::new(ScriptedModel::new(vec![]));
    let (ws, _dir) = temp_ws();
    let tool = SubagentTool::new(model, &ws);
    assert!(!tool.has_background());

    let via_tool = tool
        .call(json!({"task": "no host", "background": true}), &ctx())
        .await
        .unwrap_err()
        .to_string();
    let typed = tool
        .spawn_background(SubagentRequest::new("no host"))
        .unwrap_err()
        .to_string();
    assert_eq!(typed, via_tool);
    assert!(tool.active_jobs().is_empty());
    assert!(!tool.cancel_job(0));
    assert_eq!(tool.cancel_all_jobs(), 0);
}

