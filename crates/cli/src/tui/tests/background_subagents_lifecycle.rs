#[test]
fn initial_sidekick_failure_is_failed_but_established_followup_failure_is_idle() {
    let (worker, _rx) = mpsc::unbounded_channel();
    let mut app = test_app();
    handle_ui_msg(
        &mut app,
        UiMsg::Event(HarnessEvent::ToolCall {
            tool_call_id: "outer-7".into(),
            tool_name: "subagent".into(),
            input: serde_json::json!({"task": "first", "persistent": true}),
        }),
        &worker,
        100,
    );
    start_test_subagent(&mut app, &worker);

    finish_test_subagent(
        &mut app,
        &worker,
        7,
        "outer-7",
        HarnessEvent::Error {
            message: "initial failed".into(),
        },
    );
    assert!(app.subagent_transcripts[&7].persistent);
    assert_eq!(
        app.subagent_transcripts[&7].status,
        crate::tui::state::SubagentTranscriptStatus::Failed,
        "creation failed before the runtime returned a reusable handle"
    );

    handle_ui_msg(
        &mut app,
        UiMsg::Event(HarnessEvent::ToolResult {
            tool_call_id: "outer-7".into(),
            tool_name: "subagent".into(),
            output: serde_json::json!({"spawnId": 7, "termination": "retained"}),
            is_error: false,
        }),
        &worker,
        100,
    );
    finish_test_subagent(
        &mut app,
        &worker,
        7,
        "follow-up-7",
        HarnessEvent::Error {
            message: "follow-up failed".into(),
        },
    );
    assert_eq!(
        app.subagent_transcripts[&7].status,
        crate::tui::state::SubagentTranscriptStatus::Idle
    );

    start_named_test_subagent(&mut app, &worker, 8, None, 0, "one-shot", "ordinary");
    finish_test_subagent(
        &mut app,
        &worker,
        8,
        "one-shot",
        HarnessEvent::Error {
            message: "ordinary failure".into(),
        },
    );
    assert_eq!(
        app.subagent_transcripts[&8].status,
        crate::tui::state::SubagentTranscriptStatus::Failed
    );
}

#[test]
fn stopped_sidekick_keeps_late_error_and_result_without_leaving_stopped() {
    let (worker, _rx) = mpsc::unbounded_channel();
    let mut app = test_app();
    start_test_subagent(&mut app, &worker);
    handle_ui_msg(
        &mut app,
        UiMsg::Event(HarnessEvent::ToolResult {
            tool_call_id: "stop-7".into(),
            tool_name: "subagent".into(),
            output: serde_json::json!({"spawnId": 7, "termination": "stopped"}),
            is_error: false,
        }),
        &worker,
        100,
    );
    for event in [
        HarnessEvent::Error {
            message: "late cancellation".into(),
        },
        HarnessEvent::Result {
            message: "late result".into(),
        },
    ] {
        finish_test_subagent(&mut app, &worker, 7, "outer-7", event);
        assert_eq!(
            app.subagent_transcripts[&7].status,
            crate::tui::state::SubagentTranscriptStatus::Stopped
        );
    }
    assert_eq!(app.subagent_transcripts[&7].latest_answer(), Some("late result"));
}

#[test]
fn queued_background_agent_reads_as_queued_until_its_inner_loop_starts() {
    let (worker, _rx) = mpsc::unbounded_channel();
    let mut app = test_app();
    start_named_test_subagent(&mut app, &worker, 7, None, 0, "outer-7", "first");
    start_named_test_subagent(&mut app, &worker, 8, None, 0, "outer-8", "second");
    for (id, call_id, status) in [(7, "outer-7", "running"), (8, "outer-8", "queued")] {
        handle_ui_msg(
            &mut app,
            UiMsg::Event(HarnessEvent::ToolResult {
                tool_call_id: call_id.into(),
                tool_name: "subagent".into(),
                output: serde_json::json!({
                    "spawnId": id,
                    "status": status,
                    "termination": "detached"
                }),
                is_error: false,
            }),
            &worker,
            160,
        );
    }
    // Only the first worker's inner loop has started.
    handle_ui_msg(
        &mut app,
        UiMsg::SubagentEvent {
            id: 7,
            parent_id: None,
            depth: 0,
            call_id: "outer-7".into(),
            event: HarnessEvent::AgentStart,
        },
        &worker,
        160,
    );

    let first = app.subagent_transcripts.get(&7).unwrap();
    let second = app.subagent_transcripts.get(&8).unwrap();
    assert_eq!(
        first.status,
        crate::tui::state::SubagentTranscriptStatus::Running
    );
    assert_eq!(
        second.status,
        crate::tui::state::SubagentTranscriptStatus::Queued
    );
    assert!(second.detached, "a queued spawn is still a detached one");
    assert!(
        app.subagent_activity.contains_key(&8),
        "queued is not terminal: the activity record must survive"
    );

    app.agent_browser = Some(crate::tui::state::AgentBrowser::new(2));
    app.agent_browser.as_mut().unwrap().picker.move_by(1);
    let rendered = rendered_rows(&mut app, 160, 36).join("\n");
    assert!(rendered.contains("queued ·"), "{rendered}");
    assert!(rendered.contains("active 1"), "{rendered}");
    assert!(rendered.contains("queued 1"), "{rendered}");
    assert!(rendered.contains("Waiting for a slot"), "{rendered}");

    // Its slot arrives: the first event from the inner loop flips it.
    handle_ui_msg(
        &mut app,
        UiMsg::SubagentEvent {
            id: 8,
            parent_id: None,
            depth: 0,
            call_id: "outer-8".into(),
            event: HarnessEvent::AgentStart,
        },
        &worker,
        160,
    );
    assert_eq!(
        app.subagent_transcripts.get(&8).unwrap().status,
        crate::tui::state::SubagentTranscriptStatus::Running
    );
    let rendered = rendered_rows(&mut app, 160, 36).join("\n");
    assert!(rendered.contains("active 2"), "{rendered}");
    assert!(!rendered.contains("queued"), "{rendered}");
}

