#[tokio::test]
async fn dropping_followup_during_pending_model_clears_busy_and_manager_accounting() {
    struct FollowupGateModel {
        calls: std::sync::atomic::AtomicUsize,
        entered: Arc<tokio::sync::Notify>,
    }

    #[async_trait]
    impl Model for FollowupGateModel {
        async fn generate(
            &self,
            _context: &Context,
            _tools: &[ToolSchema],
        ) -> Result<ModelResponse, ModelError> {
            if self
                .calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                == 0
            {
                Ok(ModelResponse::final_text("initial"))
            } else {
                self.entered.notify_one();
                std::future::pending().await
            }
        }
    }

    let entered = Arc::new(tokio::sync::Notify::new());
    let (ws, _dir) = temp_ws();
    let manager = orca_harness_tools::SubagentManager::new(1);
    let tool = SubagentTool::new(
        Arc::new(FollowupGateModel {
            calls: std::sync::atomic::AtomicUsize::new(0),
            entered: entered.clone(),
        }),
        &ws,
    )
    .background(manager.clone(), |_| {});
    let id = tool
        .start_sidekick_foreground(SubagentRequest::new("initial"), CancellationToken::new(), None)
        .await
        .unwrap()
        .spawn_id;
    let followup = tokio::spawn({
        let tool = tool.clone();
        async move {
            tool.sidekick_task_foreground(id, "pending model", CancellationToken::new(), None)
                .await
        }
    });
    entered.notified().await;
    followup.abort();
    assert!(followup.await.unwrap_err().is_cancelled());
    tokio::task::yield_now().await;

    assert_eq!(tool.sidekick_status(id).unwrap().as_str(), "idle");
    assert_eq!(manager.live_workers(), 0);
}

#[tokio::test]
async fn dropping_followup_during_pending_tool_rolls_back_and_clears_busy() {
    let entered = Arc::new(tokio::sync::Notify::new());
    let entered_for_tool = entered.clone();
    let tools = Arc::new(move || {
        let entered = entered_for_tool.clone();
        vec![Arc::new(orca_harness_core::FnTool::new(
            "hold",
            "hold until dropped",
            json!({"type": "object"}),
            move |_args, _ctx| {
                let entered = entered.clone();
                async move {
                    entered.notify_one();
                    std::future::pending().await
                }
            },
        )) as Arc<dyn Tool>]
    });
    let model = Arc::new(ScriptedModel::new(vec![
        ModelResponse::final_text("initial"),
        ModelResponse::ToolCalls {
            content: None,
            calls: vec![call("held", "hold", json!({}))],
            usage: None,
        },
        ModelResponse::final_text("after rollback"),
    ]));
    let manager = orca_harness_tools::SubagentManager::new(1);
    let tool = SubagentTool::with_tools(model.clone(), tools).background(manager.clone(), |_| {});
    let id = tool
        .start_sidekick_foreground(SubagentRequest::new("initial"), CancellationToken::new(), None)
        .await
        .unwrap()
        .spawn_id;
    let followup = tokio::spawn({
        let tool = tool.clone();
        async move {
            tool.sidekick_task_foreground(id, "pending tool", CancellationToken::new(), None)
                .await
        }
    });
    entered.notified().await;
    followup.abort();
    assert!(followup.await.unwrap_err().is_cancelled());
    tokio::task::yield_now().await;
    assert_eq!(tool.sidekick_status(id).unwrap().as_str(), "idle");
    assert_eq!(manager.live_workers(), 0);

    let report = tool
        .sidekick_task_foreground(id, "retry", CancellationToken::new(), None)
        .await
        .unwrap();
    assert_eq!(report.answer, "after rollback");
    assert_eq!(model.observed_contexts()[2].messages().len(), 4);
}

#[tokio::test]
async fn followup_gets_a_fresh_timeout_window() {
    let settings = orca_harness_tools::SubagentDepth::default();
    settings.set_timeout_secs(1);
    let model = Arc::new(ScriptedModel::new(vec![
        ModelResponse::final_text("initial"),
        ModelResponse::final_text("followup"),
    ]));
    let (ws, _dir) = temp_ws();
    let tool = SubagentTool::new(model, &ws)
        .max_depth(settings)
        .background(orca_harness_tools::SubagentManager::new(1), |_| {});
    let id = tool
        .start_sidekick_foreground(SubagentRequest::new("initial"), CancellationToken::new(), None)
        .await
        .unwrap()
        .spawn_id;

    tokio::time::sleep(Duration::from_millis(1100)).await;
    let report = tool
        .sidekick_task_foreground(id, "after initial timeout window", CancellationToken::new(), None)
        .await
        .unwrap();
    assert_eq!(report.answer, "followup");
}
