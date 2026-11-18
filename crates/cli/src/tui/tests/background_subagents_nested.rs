#[test]
fn nested_background_agent_is_detached_and_releases_live_activity() {
    let (worker, _rx) = mpsc::unbounded_channel();
    let mut app = test_app();
    start_test_subagent(&mut app, &worker);
    start_named_test_subagent(&mut app, &worker, 8, Some(7), 1, "nested", "nested task");
    finish_test_subagent(
        &mut app,
        &worker,
        7,
        "outer-7",
        HarnessEvent::ToolResult {
            tool_call_id: "nested".into(),
            tool_name: "subagent".into(),
            output: serde_json::json!({"termination": "detached", "spawnId": 8}),
            is_error: false,
        },
    );
    assert!(app.subagent_transcripts[&8].detached);
    finish_test_subagent(
        &mut app,
        &worker,
        8,
        "nested",
        HarnessEvent::Result {
            message: "nested answer".into(),
        },
    );
    assert!(!app.subagent_activity.contains_key(&8));
    assert_eq!(
        app.subagent_transcripts[&8].latest_answer(),
        Some("nested answer")
    );
}

#[test]
fn queued_background_completion_marks_failure_and_deduplicates_observer_results() {
    let (worker, _rx) = mpsc::unbounded_channel();
    let mut app = test_app();
    start_test_subagent(&mut app, &worker);
    app.subagent_transcripts.get_mut(&7).unwrap().detached = true;
    handle_ui_msg(
        &mut app,
        UiMsg::SubagentCompleted {
            id: 7,
            is_error: true,
            message: "cancelled before acquiring slot".into(),
        },
        &worker,
        160,
    );
    assert_eq!(
        app.subagent_transcripts[&7].status,
        crate::tui::state::SubagentTranscriptStatus::Failed
    );
    assert!(!app.subagent_activity.contains_key(&7));
    let count = app.subagent_transcripts[&7].entries.len();
    handle_ui_msg(
        &mut app,
        UiMsg::SubagentCompleted {
            id: 7,
            is_error: true,
            message: "duplicate failure".into(),
        },
        &worker,
        160,
    );
    assert_eq!(app.subagent_transcripts[&7].entries.len(), count);
}
