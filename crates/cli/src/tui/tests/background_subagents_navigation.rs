#[test]
fn down_enters_the_status_row_and_arrows_reach_agents() {
    let (worker, _rx) = mpsc::unbounded_channel();
    let mut app = test_app();
    start_test_subagent(&mut app, &worker);

    let initial = rendered_rows(&mut app, 120, 30).join("\n");
    assert!(initial.contains("agents 1 ↓"), "{initial}");

    handle_terminal_event(
        &mut app,
        CtEvent::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        &worker,
        120,
    );
    assert_eq!(app.status_focus, Some(StatusFocus::Context));
    handle_terminal_event(
        &mut app,
        CtEvent::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
        &worker,
        120,
    );
    assert_eq!(app.status_focus, Some(StatusFocus::Agents));
    let status = rendered_rows(&mut app, 120, 30).join("\n");
    assert!(status.contains("agents 1"), "{status}");
    assert!(status.contains("enter open"), "{status}");

    handle_terminal_event(
        &mut app,
        CtEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        &worker,
        120,
    );
    assert!(app.agent_browser.is_some());

    handle_ui_msg(
        &mut app,
        UiMsg::SubagentEvent {
            id: 7,
            parent_id: None,
            depth: 0,
            call_id: "outer-7".into(),
            event: HarnessEvent::AssistantDelta {
                text: "live transcript text".into(),
            },
        },
        &worker,
        120,
    );
    let browser = rendered_rows(&mut app, 120, 30).join("\n");
    assert!(browser.contains("inspect queue ordering"), "{browser}");
    assert!(browser.contains("live transcript text"), "{browser}");
    assert!(browser.contains("parent continues"), "{browser}");

    handle_terminal_event(
        &mut app,
        CtEvent::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        &worker,
        120,
    );
    assert!(app.agent_browser.is_none());
}

#[test]
fn status_line_counts_only_running_and_queued_agents() {
    let (worker, _rx) = mpsc::unbounded_channel();
    let mut app = browser_with_one_agent_per_state(&worker);
    app.agent_browser = None;
    start_named_test_subagent(&mut app, &worker, 10, None, 0, "queued", "queued task");
    finish_test_subagent(&mut app, &worker, 7, "active", HarnessEvent::AgentStart);
    let rendered = rendered_rows(&mut app, 160, 36).join("\n");
    assert!(rendered.contains("agents 2 ↓"), "{rendered}");
    for (id, call_id) in [(7, "active"), (10, "queued")] {
        finish_test_subagent(
            &mut app,
            &worker,
            id,
            call_id,
            HarnessEvent::Result {
                message: "done".into(),
            },
        );
    }
    let rendered = rendered_rows(&mut app, 160, 36).join("\n");
    assert!(
        !rendered.lines().last().unwrap().contains("agents "),
        "{rendered}"
    );
    assert_eq!(
        app.subagent_transcripts.len(),
        4,
        "history remains available"
    );
    press_in_browser(&mut app, &worker, KeyCode::Down);
    assert_eq!(app.status_focus, Some(StatusFocus::Context));
    press_in_browser(&mut app, &worker, KeyCode::Right);
    assert_eq!(app.status_focus, Some(StatusFocus::Agents));
    press_in_browser(&mut app, &worker, KeyCode::Enter);
    assert!(app.agent_browser.is_some());
}

#[test]
fn selected_agent_uses_the_normal_thinking_and_work_rails() {
    let (worker, _rx) = mpsc::unbounded_channel();
    let mut app = test_app();
    start_test_subagent(&mut app, &worker);
    app.agent_browser = Some(crate::tui::state::AgentBrowser::new(1));
    app.agent_browser.as_mut().unwrap().tab = crate::tui::state::AgentTab::All;

    for event in [
        HarnessEvent::ReasoningDelta {
            text: "inspect the file".into(),
        },
        HarnessEvent::ToolCall {
            tool_call_id: "read-1".into(),
            tool_name: "read_file".into(),
            input: serde_json::json!({"path": "README.md"}),
        },
        HarnessEvent::ToolStarted {
            tool_call_id: "read-1".into(),
            tool_name: "read_file".into(),
        },
        HarnessEvent::ToolFinished {
            tool_call_id: "read-1".into(),
            tool_name: "read_file".into(),
            is_error: false,
        },
        HarnessEvent::ToolResult {
            tool_call_id: "read-1".into(),
            tool_name: "read_file".into(),
            output: serde_json::json!({"content": "hello"}),
            is_error: false,
        },
        HarnessEvent::Result {
            message: "finished inspection".into(),
        },
    ] {
        handle_ui_msg(
            &mut app,
            UiMsg::SubagentEvent {
                id: 7,
                parent_id: None,
                depth: 0,
                call_id: "outer-7".into(),
                event,
            },
            &worker,
            160,
        );
    }

    let browser = rendered_rows(&mut app, 160, 36).join("\n");
    assert!(browser.contains("Thinking"), "{browser}");
    assert!(browser.contains("Work"), "{browser}");
    assert!(crate::tui::text::has_row(&browser, &["Read", "README.md"]), "{browser}");
    assert!(browser.contains("finished inspection"), "{browser}");
}

#[test]
fn down_keeps_history_behavior_when_the_composer_has_a_draft() {
    let (worker, _rx) = mpsc::unbounded_channel();
    let mut app = test_app();
    start_test_subagent(&mut app, &worker);
    app.composer = "unfinished prompt".into();
    app.cursor = app.composer.chars().count();

    handle_terminal_event(
        &mut app,
        CtEvent::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        &worker,
        100,
    );

    assert!(app.status_focus.is_none());
    assert_eq!(app.composer, "unfinished prompt");
}

#[test]
fn agent_browser_routes_mouse_and_ignores_composer_paste() {
    let (worker, _rx) = mpsc::unbounded_channel();
    let mut app = test_app();
    start_test_subagent(&mut app, &worker);
    app.agent_browser = Some(crate::tui::state::AgentBrowser::new(1));
    handle_terminal_event(
        &mut app,
        CtEvent::Paste("hidden draft".into()),
        &worker,
        160,
    );
    assert!(app.composer.is_empty());
    handle_terminal_event(
        &mut app,
        CtEvent::Mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: 1,
            row: 1,
            modifiers: KeyModifiers::NONE,
        }),
        &worker,
        160,
    );
    assert_eq!(app.agent_browser.as_ref().unwrap().scroll, 3);
    assert_eq!(app.scroll, 0);
}

