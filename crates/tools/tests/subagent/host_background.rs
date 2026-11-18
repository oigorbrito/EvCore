#[tokio::test]
async fn background_sidekick_delivers_each_turn_with_one_handle_and_retained_context() {
    let model = Arc::new(ScriptedModel::new(vec![
        ModelResponse::final_text("first"),
        ModelResponse::final_text("second"),
    ]));
    let (ws, _dir) = temp_ws();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let tool = SubagentTool::new(model.clone(), &ws).background(
        orca_harness_tools::SubagentManager::new(1),
        move |event| {
            let _ = tx.send(event);
        },
    );

    let first = tool.start_sidekick(SubagentRequest::new("one")).unwrap();
    let first_event = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first_event.spawn.id, first.spawn_id);
    assert_eq!(first_event.result.unwrap()["answer"], "first");
    assert_eq!(tool.sidekick_status(first.spawn_id).unwrap().as_str(), "idle");

    let second = tool.sidekick_task(first.spawn_id, "two").unwrap();
    assert_eq!(second.spawn_id, first.spawn_id);
    let second_event = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second_event.spawn.id, first.spawn_id);
    assert_eq!(second_event.result.unwrap()["answer"], "second");
    assert!(rx.try_recv().is_err(), "one completion per turn");
    assert_eq!(model.observed_contexts()[1].messages().len(), 4);
}

#[tokio::test]
async fn queued_sidekick_is_busy_and_can_be_stopped_without_starting() {
    let release = CancellationToken::new();
    let started = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (ws, _dir) = temp_ws();
    let manager = orca_harness_tools::SubagentManager::new(1);
    let tool = SubagentTool::new(
        Arc::new(CountingGateModel {
            started: started.clone(),
            release: release.clone(),
        }),
        &ws,
    )
    .background(manager, |_| {});
    let blocker = tool.spawn_background(SubagentRequest::new("block slot")).unwrap();
    let sidekick = tool.start_sidekick(SubagentRequest::new("queued")).unwrap();
    assert_eq!(sidekick.status, BackgroundStatus::Queued);
    assert_eq!(
        tool.sidekick_task(sidekick.spawn_id, "overlap")
            .unwrap_err()
            .to_string(),
        "sidekick is busy"
    );
    tool.stop_sidekick(sidekick.spawn_id).unwrap();
    release.cancel();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(started.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(tool.cancel_job(blocker.spawn_id) || tool.active_jobs().is_empty());
}

#[tokio::test]
async fn background_initial_failure_is_delivered_and_handle_can_retry() {
    struct FailOnceModel(std::sync::atomic::AtomicUsize);
    #[async_trait]
    impl Model for FailOnceModel {
        async fn generate(
            &self,
            _context: &Context,
            _tools: &[ToolSchema],
        ) -> Result<ModelResponse, ModelError> {
            if self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                Err(ModelError::InvalidResponse("first failed".into()))
            } else {
                Ok(ModelResponse::final_text("retry succeeded"))
            }
        }
    }

    let (ws, _dir) = temp_ws();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let tool = SubagentTool::new(
        Arc::new(FailOnceModel(std::sync::atomic::AtomicUsize::new(0))),
        &ws,
    )
    .background(orca_harness_tools::SubagentManager::new(1), move |event| {
        let _ = tx.send(event);
    });

    let acknowledgement = tool.start_sidekick(SubagentRequest::new("fail")).unwrap();
    let failed = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(failed.result.unwrap_err().contains("first failed"));
    assert_eq!(
        tool.sidekick_status(acknowledgement.spawn_id)
            .unwrap()
            .as_str(),
        "idle"
    );

    tool.sidekick_task(acknowledgement.spawn_id, "retry")
        .unwrap();
    let retried = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retried.result.unwrap()["answer"], "retry succeeded");
}

#[tokio::test]
async fn sidekick_rejects_busy_and_stop_cancels_active_task() {
    let release = CancellationToken::new();
    let (ws, _dir) = temp_ws();
    let tool = SubagentTool::new(
        Arc::new(GateModel {
            release: release.clone(),
        }),
        &ws,
    )
    .background(orca_harness_tools::SubagentManager::new(0), |_| {});
    // Seed the handle without waiting for its gated first model call.
    let run = tokio::spawn({
        let tool = tool.clone();
        async move {
            tool.start_sidekick_foreground(SubagentRequest::new("held"), CancellationToken::new(), None)
                .await
        }
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    let error = tool
        .sidekick_task_foreground(0, "overlap", CancellationToken::new(), None)
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(error, "sidekick is busy");
    tool.stop_sidekick(0).unwrap();
    assert!(run.await.unwrap().unwrap_err().to_string().contains("cancelled"));
    assert!(!release.is_cancelled());
}

#[tokio::test]
async fn aborting_initial_sidekick_future_removes_hidden_registration() {
    let release = CancellationToken::new();
    let (ws, _dir) = temp_ws();
    let manager = orca_harness_tools::SubagentManager::new(0);
    let tool = SubagentTool::new(
        Arc::new(GateModel {
            release: release.clone(),
        }),
        &ws,
    )
    .background(manager.clone(), |_| {});
    let run = tokio::spawn({
        let tool = tool.clone();
        async move {
            tool.start_sidekick_foreground(SubagentRequest::new("held"), CancellationToken::new(), None)
                .await
        }
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    run.abort();
    assert!(run.await.unwrap_err().is_cancelled());
    tokio::task::yield_now().await;

    assert_eq!(manager.live_workers(), 0);
    assert_eq!(
        tool.sidekick_status(0).unwrap_err().to_string(),
        "unknown sidekick handle"
    );
}

#[tokio::test]
async fn manager_reset_stops_idle_sidekicks() {
    let (ws, _dir) = temp_ws();
    let manager = orca_harness_tools::SubagentManager::new(0);
    let tool = SubagentTool::new(scripted_final("report"), &ws)
        .background(manager.clone(), |_| {});
    let report = tool
        .start_sidekick_foreground(SubagentRequest::new("task"), CancellationToken::new(), None)
        .await
        .unwrap();
    assert_eq!(manager.cancel_all(), 0, "ordinary cancellation leaves sidekicks intact");
    assert_eq!(tool.sidekick_status(report.spawn_id).unwrap().as_str(), "idle");
    manager.reset();
    assert_eq!(
        tool.sidekick_status(report.spawn_id).unwrap().as_str(),
        "stopped"
    );
}

#[tokio::test]
async fn failed_initial_sidekick_task_cleans_up_the_undisclosed_handle() {
    let release = CancellationToken::new();
    let (ws, _dir) = temp_ws();
    let tool = SubagentTool::new(
        Arc::new(GateModel {
            release: release.clone(),
        }),
        &ws,
    )
    .background(orca_harness_tools::SubagentManager::new(0), |_| {});
    let cancel = CancellationToken::new();
    let first = tokio::spawn({
        let tool = tool.clone();
        let cancel = cancel.clone();
        async move {
            tool.start_sidekick_foreground(SubagentRequest::new("first"), cancel, None)
                .await
        }
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    cancel.cancel();
    assert!(first.await.unwrap().unwrap_err().to_string().contains("cancelled"));
    assert_eq!(
        tool.sidekick_status(0).unwrap_err().to_string(),
        "unknown sidekick handle"
    );
    assert_eq!(
        tool.sidekick_status(999).unwrap_err().to_string(),
        "unknown sidekick handle"
    );
}

