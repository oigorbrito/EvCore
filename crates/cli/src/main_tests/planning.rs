    /// Plan mode must reach every level of the agent tree. Spawning is
    /// denied in plan mode, so the case this covers is the reachable
    /// one: a subagent already running when the user flips `/mode`.
    #[tokio::test]
    async fn spawned_subagents_inherit_the_plan_gate() {
        use orca_harness_core::{ToolCall, ToolDecision};

        let (ui, _rx) = mpsc::unbounded_channel();
        let mode = ModeHandle::new(Mode::Normal);
        let spawn = SubagentSpawn {
            stage: None,
            run: None,
            id: 1,
            parent_id: None,
            depth: 0,
            call_id: "c1".into(),
            task: "do the thing".into(),
            identity: Some(orca_harness_tools::SubagentIdentity::new(
                "openrouter",
                "anthropic/claude-sonnet-5",
            )),
        };
        let settings = orca_harness_tools::SubagentDepth::default();
        let extensions =
            subagent_extensions(&spawn, &ui, &mode, &PlanArea::new(), &settings, None, None);
        let names: Vec<&str> = extensions.iter().map(|ext| ext.name()).collect();
        assert!(names.contains(&"plan-mode"), "{names:?}");
        assert!(names.contains(&"truncation"), "{names:?}");

        let gate = extensions
            .iter()
            .find(|ext| ext.name() == "plan-mode")
            .expect("a plan gate");
        let call = ToolCall {
            id: "c2".into(),
            name: "write_file".into(),
            arguments: serde_json::json!({}),
        };
        // The inner gate shares the session's handle, so flipping the
        // mode reaches an already-spawned agent.
        assert!(matches!(
            gate.before_tool(&call).await.unwrap(),
            ToolDecision::Continue
        ));
        mode.set(Mode::Plan);
        assert!(matches!(
            gate.before_tool(&call).await.unwrap(),
            ToolDecision::Deny { .. }
        ));

        // The worker view is what actually reaches workers: orchestrate
        // does not propagate (its whole point is to push work down to
        // these agents), while plan does.
        mode.set(Mode::Orchestrate);
        let worker_gate = extensions
            .iter()
            .find(|ext| ext.name() == "plan-mode")
            .expect("a plan gate");
        let gate_call = ToolCall {
            id: "c3".into(),
            name: "shell".into(),
            arguments: serde_json::json!({"command": "cargo test"}),
        };
        assert!(matches!(
            worker_gate.before_tool(&gate_call).await.unwrap(),
            ToolDecision::Continue
        ));
    }

    fn planning(mode: Mode) -> Planning {
        Planning {
            mode: ModeHandle::new(mode),
            area: PlanArea::new(),
        }
    }

    /// A planning episode briefs the model once and names no file: what
    /// to write, and whether to write anything, is the agent's call.
    #[test]
    fn a_planning_episode_briefs_once_and_names_no_file() {
        let planning = planning(Mode::Plan);
        let mut context = Context::new();
        context.push_system("base prompt");

        assert!(planning.open_episode(&mut context), "the first turn briefs");
        let briefing = match context.messages().last() {
            Some(Message::System { content }) => content.clone(),
            other => panic!("expected a system briefing, got {other:?}"),
        };
        assert!(briefing.contains("docs/plan/"), "{briefing}");
        assert!(briefing.contains("You choose the name"), "{briefing}");
        assert!(briefing.contains("Decide for yourself"), "{briefing}");
        // Nothing has been written, so nothing is claimed.
        assert!(planning.area.written().is_empty());

        // Later turns in the same episode add nothing.
        let before = context.messages().len();
        assert!(!planning.open_episode(&mut context));
        assert_eq!(context.messages().len(), before);
    }

    /// Normal mode never briefs, so an ordinary session's context is
    /// untouched by any of this.
    #[test]
    fn normal_mode_opens_no_episode() {
        let planning = planning(Mode::Normal);
        let mut context = Context::new();
        context.push_system("base prompt");
        assert!(!planning.open_episode(&mut context));
        assert_eq!(context.messages().len(), 1, "no briefing pushed");
    }

    /// Ending an episode and planning again briefs afresh — the model in
    /// the second episode has to be told the rules too.
    #[test]
    fn a_second_episode_briefs_again() {
        let planning = planning(Mode::Plan);
        let mut context = Context::new();
        assert!(planning.open_episode(&mut context));
        planning.area.record("docs/plan/2026-08-22-first.md");

        let written = planning.area.end();
        assert_eq!(written, ["docs/plan/2026-08-22-first.md".to_string()]);
        assert!(planning.open_episode(&mut context));
        assert!(planning.area.written().is_empty(), "a fresh list");
    }

