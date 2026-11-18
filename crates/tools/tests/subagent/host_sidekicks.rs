#[tokio::test]
async fn sidekick_follow_up_reuses_context_and_stop_is_terminal() {
    let model = Arc::new(ScriptedModel::new(vec![
        ModelResponse::final_text("first report"),
        ModelResponse::final_text("follow-up report"),
    ]));
    let (ws, _dir) = temp_ws();
    let tool = SubagentTool::new(model.clone(), &ws)
        .background(orca_harness_tools::SubagentManager::new(0), |_| {});

    let first = tool
        .start_sidekick_foreground(SubagentRequest::new("inspect alpha"), CancellationToken::new(), None)
        .await
        .unwrap();
    assert_eq!(first.status.as_str(), "idle");
    assert_eq!(tool.sidekick_status(first.spawn_id).unwrap().as_str(), "idle");
    let follow_up = tool
        .sidekick_task_foreground(first.spawn_id, "check beta", CancellationToken::new(), None)
        .await
        .unwrap();
    assert_eq!(follow_up.answer, "follow-up report");
    let observed = model.observed_contexts();
    assert_eq!(observed.len(), 2);
    assert_eq!(observed[0].messages().len(), 2);
    assert_eq!(observed[1].messages().len(), 4, "first turn is retained");

    tool.stop_sidekick(first.spawn_id).unwrap();
    tool.stop_sidekick(first.spawn_id).unwrap();
    assert_eq!(
        tool.sidekick_status(first.spawn_id).unwrap().as_str(),
        "stopped"
    );
    let error = tool
        .sidekick_task_foreground(first.spawn_id, "too late", CancellationToken::new(), None)
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(error, "sidekick is stopped");
}

#[tokio::test]
async fn rebuilt_tool_adopts_live_sidekicks_with_their_context() {
    let model = Arc::new(ScriptedModel::new(vec![
        ModelResponse::final_text("alpha report"),
        ModelResponse::final_text("beta report"),
        ModelResponse::final_text("follow-up report"),
    ]));
    let (ws, _dir) = temp_ws();
    let manager = orca_harness_tools::SubagentManager::new(0);
    let old = SubagentTool::new(model.clone(), &ws).background(manager.clone(), |_| {});
    let live = old
        .start_sidekick_foreground(SubagentRequest::new("inspect alpha"), CancellationToken::new(), None)
        .await
        .unwrap();
    let stopped = old
        .start_sidekick_foreground(SubagentRequest::new("inspect beta"), CancellationToken::new(), None)
        .await
        .unwrap();
    old.stop_sidekick(stopped.spawn_id).unwrap();

    let rebuilt = SubagentTool::new(model.clone(), &ws).background(manager, |_| {});
    rebuilt.adopt_sidekicks(&old);
    drop(old);

    assert_eq!(rebuilt.sidekick_status(live.spawn_id).unwrap().as_str(), "idle");
    assert_eq!(rebuilt.sidekick_status(stopped.spawn_id).unwrap().as_str(), "stopped");
    let follow_up = rebuilt
        .sidekick_task_foreground(live.spawn_id, "check gamma", CancellationToken::new(), None)
        .await
        .unwrap();
    assert_eq!(follow_up.answer, "follow-up report");
    let observed = model.observed_contexts();
    assert_eq!(observed.last().unwrap().messages().len(), 4, "alpha context survives the rebuild");
}

#[tokio::test]
async fn cancel_all_leaves_idle_sidekick_available() {
    let (ws, _dir) = temp_ws();
    let manager = orca_harness_tools::SubagentManager::new(0);
    let tool = SubagentTool::new(scripted_final("report"), &ws)
        .background(manager.clone(), |_| {});
    let report = tool
        .start_sidekick_foreground(SubagentRequest::new("task"), CancellationToken::new(), None)
        .await
        .unwrap();

    assert_eq!(tool.cancel_all_jobs(), 0);
    assert_eq!(tool.sidekick_status(report.spawn_id).unwrap().as_str(), "idle");
}

#[tokio::test]
async fn persistent_background_true_returns_an_immediate_stable_handle() {
    let (ws, _dir) = temp_ws();
    let tool = SubagentTool::new(scripted_final("unused"), &ws)
        .background(orca_harness_tools::SubagentManager::new(0), |_| {});
    let acknowledgement = tool
        .call(
            json!({"task": "task", "persistent": true, "background": true}),
            &ctx(),
        )
        .await
        .unwrap();
    assert_eq!(acknowledgement["spawnId"], 0);
    assert_eq!(acknowledgement["status"], "running");
    assert_eq!(acknowledgement["persistent"], true);
}

#[tokio::test]
async fn background_sidekick_returns_before_model_and_stop_guards_late_completion() {
    let release = CancellationToken::new();
    let (ws, _dir) = temp_ws();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let tool = SubagentTool::new(
        Arc::new(GateModel {
            release: release.clone(),
        }),
        &ws,
    )
    .background(orca_harness_tools::SubagentManager::new(1), move |event| {
        let _ = tx.send(event);
    });

    let acknowledgement = tokio::time::timeout(
        Duration::from_millis(100),
        tool.call(json!({"task": "held", "persistent": true}), &ctx()),
    )
    .await
    .expect("persistent task must not await the model")
    .unwrap();
    let id = acknowledgement["spawnId"].as_u64().unwrap();
    assert_eq!(tool.sidekick_status(id).unwrap().as_str(), "busy");
    tool.stop_sidekick(id).unwrap();
    release.cancel();
    assert!(tokio::time::timeout(Duration::from_millis(100), rx.recv())
        .await
        .is_err());
    assert_eq!(tool.sidekick_status(id).unwrap().as_str(), "stopped");
}

