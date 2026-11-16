#[tokio::test]
async fn typed_run_nests_children_by_the_shared_depth() {
    let (ws, _dir) = temp_ws();
    let nested = Arc::new(ScriptedModel::new(vec![
        ModelResponse::tool_calls(vec![call("1", "subagent", json!({"task": "inner"}))]),
        ModelResponse::final_text("grandchild done"),
        ModelResponse::final_text("child done"),
    ]));
    let spawns: Arc<Mutex<Vec<SubagentSpawn>>> = Arc::new(Mutex::new(Vec::new()));
    let recorded = spawns.clone();
    let tool = SubagentTool::new(nested.clone(), &ws)
        .max_depth(SubagentDepth::new(2))
        .spawn_extensions(Arc::new(move |spawn| {
            recorded.lock().unwrap().push(spawn.clone());
            Vec::new()
        }));
    let outcome = tool
        .run_foreground(
            SubagentRequest::new("outer"),
            CancellationToken::new(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(outcome.answer, "child done");
    assert_eq!(nested.generate_calls(), 3, "grandchild must actually run");
    assert_eq!(outcome.steps, 2);
    assert_eq!(outcome.tool_calls, 1);
    {
        let spawns = spawns.lock().unwrap();
        assert_eq!(
            spawns.iter().map(|spawn| spawn.depth).collect::<Vec<_>>(),
            [0, 1]
        );
        assert_eq!(spawns[0].call_id, format!("host:{}", spawns[0].id));
        assert_eq!(spawns[1].parent_id, Some(spawns[0].id));
    }

    let flat = Arc::new(ScriptedModel::new(vec![
        ModelResponse::tool_calls(vec![call("1", "subagent", json!({"task": "inner"}))]),
        ModelResponse::final_text("child done"),
    ]));
    let tool = SubagentTool::new(flat.clone(), &ws); // max_depth defaults to 1
    tool.run_foreground(
        SubagentRequest::new("outer"),
        CancellationToken::new(),
        None,
    )
    .await
    .unwrap();
    let contexts = flat.observed_contexts();
    let saw_unknown = contexts
        .last()
        .unwrap()
        .messages()
        .iter()
        .any(|m| match m {
            Message::Tool { results } => results
                .iter()
                .any(|r| r.is_error && r.output.to_string().contains("unknown tool")),
            _ => false,
        });
    assert!(saw_unknown, "at the depth limit the child has no subagent tool");
}

#[tokio::test]
async fn typed_cancellation_matches_the_tool_cancel_action() {
    let release = CancellationToken::new();
    let (ws, _dir) = temp_ws();
    let manager = orca_harness_tools::SubagentManager::new(3);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let spawns: Arc<Mutex<Vec<SubagentSpawn>>> = Arc::new(Mutex::new(Vec::new()));
    let recorded = spawns.clone();
    let tool = SubagentTool::new(
        Arc::new(GateModel {
            release: release.clone(),
        }),
        &ws,
    )
    .spawn_extensions(Arc::new(move |spawn| {
        recorded.lock().unwrap().push(spawn.clone());
        Vec::new()
    }))
    .background(manager, move |notification| {
        let _ = tx.send(notification);
    });

    let typed = tool.spawn_background(SubagentRequest::new("host")).unwrap();
    let via_tool = tool
        .call(json!({"task": "model", "background": true}), &ctx())
        .await
        .unwrap();
    let model_id = via_tool["spawnId"].as_u64().unwrap();
    {
        let spawns = spawns.lock().unwrap();
        assert_eq!(spawns.len(), 2, "host spawns announce to spawn extensions");
        assert_eq!(spawns[0].call_id, format!("host:{}", typed.spawn_id));
        assert_eq!(spawns[1].call_id, "t");
    }

    assert!(tool.cancel_job(typed.spawn_id));
    let host_cancelled = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("cancelled host job did not report")
        .expect("completion channel closed");
    assert_eq!(host_cancelled.spawn.id, typed.spawn_id);

    let cancelled = tool
        .call(json!({"action": "cancel", "spawnId": model_id}), &ctx())
        .await
        .unwrap();
    assert_eq!(cancelled["cancelled"], true);
    let model_cancelled = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("cancelled model job did not report")
        .expect("completion channel closed");
    assert_eq!(model_cancelled.spawn.id, model_id);

    let failure = |result: &Result<Value, String>| {
        let error = result.as_ref().unwrap_err();
        error
            .split(" (runtimeMs=")
            .next()
            .unwrap_or(error)
            .to_string()
    };
    assert_eq!(
        failure(&host_cancelled.result),
        failure(&model_cancelled.result)
    );
    assert!(tool.active_jobs().is_empty());
    assert!(!tool.cancel_job(typed.spawn_id), "finished jobs are unknown");

    tool.spawn_background(SubagentRequest::new("a")).unwrap();
    tool.spawn_background(SubagentRequest::new("b")).unwrap();
    assert_eq!(tool.cancel_all_jobs(), 2);
    for _ in 0..2 {
        tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("cancel_all completion timed out")
            .expect("completion channel closed");
    }
    assert!(tool.active_jobs().is_empty());
}

#[tokio::test]
async fn typed_foreground_run_stops_when_its_token_is_cancelled() {
    let release = CancellationToken::new();
    let (ws, _dir) = temp_ws();
    let stats = BackgroundStats::new();
    let tool = SubagentTool::new(
        Arc::new(GateModel {
            release: release.clone(),
        }),
        &ws,
    )
    .stats(stats.clone());

    let cancel = CancellationToken::new();
    let run = tokio::spawn({
        let tool = tool.clone();
        let cancel = cancel.clone();
        async move {
            tool.run_foreground(SubagentRequest::new("held"), cancel, None)
                .await
        }
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(stats.agents(), 1, "the worker is in flight");
    cancel.cancel();

    let error = tokio::time::timeout(Duration::from_secs(1), run)
        .await
        .expect("cancellation must end the run")
        .unwrap()
        .unwrap_err()
        .to_string();
    assert!(error.starts_with("subagent failed: cancelled"), "{error}");
    assert_eq!(stats.agents(), 0, "in-flight accounting unwinds on cancel");
    assert!(!release.is_cancelled(), "the model never finished on its own");
}

