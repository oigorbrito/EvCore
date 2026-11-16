    /// The prompt must keep telling the model to plan and batch independent
    /// calls — dropping this silently reverts the agent to one call per turn.
    #[test]
    fn system_prompt_instructs_concurrent_batching() {
        let prompt = system_prompt(&Workspace::new(PathBuf::from(".")), false);
        assert!(prompt.starts_with("You are EvCore, a coding agent"));
        assert!(prompt.contains("execute concurrently"));
        assert!(prompt.contains("plan the batch"));
        assert!(prompt.contains("one response"));
    }

    #[test]
    fn system_prompt_requires_bounded_subagent_delegation() {
        let prompt = system_prompt(&Workspace::new(PathBuf::from(".")), false);
        assert!(prompt.contains("Delegate to subagents deliberately"));
        assert!(prompt.contains("exact result or deliverable expected"));
        assert!(prompt.contains("explicit stopping condition"));
        assert!(prompt.contains("Avoid open-ended delegation"));
    }

    /// The advertised tool list must match what build_agent registers:
    /// web_fetch is always on, web_search only with a Firecrawl key.
    #[test]
    fn system_prompt_advertises_web_tools_to_match_registration() {
        let ws = Workspace::new(PathBuf::from("."));
        let without = system_prompt(&ws, false);
        assert!(without.contains("web_fetch"));
        assert!(!without.contains("web_search"));
        let with = system_prompt(&ws, true);
        assert!(with.contains("web_search"));
    }

    /// `skill` is the one registered tool the prompt must *not* name.
    /// Whether it exists depends on the skill folders and on /skills
    /// toggles, both of which move mid-session, while this string is
    /// built once and re-pushed verbatim by /clear — so a mention here
    /// would eventually advertise a tool that is not registered. The
    /// tool's own description carries the explanation and the catalog.
    #[test]
    fn system_prompt_leaves_skills_to_the_tool_description() {
        let prompt = system_prompt(&Workspace::new(PathBuf::from(".")), false);
        assert!(!prompt.contains("skill"), "{prompt}");
    }

    #[test]
    fn system_prompt_advertises_persistent_compute_and_subagent() {
        let prompt = system_prompt(&Workspace::new(PathBuf::from(".")), false);
        assert!(prompt.contains("pykernel"));
        assert!(prompt.contains("bun_repl"));
        assert!(!prompt.contains("processes), kernel (persistent Python"));
        assert!(prompt.contains("subagent"));
    }

    /// The prompt must keep telling the model to plan ahead of each call and
    /// to keep intermediate state in the persistent compute tool that fits.
    #[test]
    fn system_prompt_instructs_planning_and_persistent_compute_state() {
        let prompt = system_prompt(&Workspace::new(PathBuf::from(".")), false);
        assert!(prompt.contains("Plan before every tool call"));
        assert!(prompt.contains("pykernel for persistent Python state"));
        assert!(prompt.contains("bun_repl for persistent JavaScript"));
        assert!(prompt.contains("fanning out"));
    }

    #[test]
    fn system_prompt_does_not_advertise_removed_task_list() {
        let prompt = system_prompt(&Workspace::new(PathBuf::from(".")), false);
        assert!(!prompt.contains("todo_write"));
    }

    #[test]
    fn system_prompt_advertises_scoped_memory_tools() {
        let prompt = system_prompt(&Workspace::new(PathBuf::from(".")), false);
        assert!(prompt.contains("memory_search"));
        assert!(prompt.contains("global and current-workspace memory"));
        assert!(prompt.contains("memory_manage"));
        assert!(prompt.contains("save, update, or forget durable memory"));
        assert!(prompt.contains(MEMORY_GUIDANCE));
        assert!(prompt.contains("Memory recall is automatic"));
        assert!(prompt.contains("Do not call memory_search merely to repeat"));
        assert!(prompt.contains("or when you identify stable, directly stated user information"));
        assert!(prompt.contains("likely to help in future sessions"));
        assert!(prompt.contains("do not store ordinary conversation, one-off task details"));
        assert!(prompt.contains("duplicates of recalled memory"));
        assert!(prompt.contains("workspace scope (is_global=false)"));
        assert!(prompt.contains("Choose global scope (is_global=true)"));
        assert!(prompt.contains("When approval is enabled"));
        assert!(prompt.contains("approval gate is the user's final decision"));
        assert!(!prompt.contains("only when the user explicitly asks"));
    }

    /// `system_prompt` is a fixed string with tests asserting what is and
    /// is not in it; the user's instructions are appended by the caller,
    /// never folded in, so no AGENTS.md can change what it returns.
    #[test]
    fn instructions_append_to_the_prompt_without_entering_it() {
        let ws = Workspace::new(PathBuf::from("."));
        let base = system_prompt(&ws, false);
        let instructions = instructions::Instructions {
            sources: vec![instructions::Source {
                label: "AGENTS.md".into(),
                body: "never mention skill".into(),
                bytes: 20,
                truncated: false,
            }],
        };
        let mut composed = system_prompt(&ws, false);
        composed.push_str(&instructions.block().unwrap());

        assert!(composed.starts_with(&base), "the base prompt is unchanged");
        assert!(composed.contains("never mention skill"));
        // ...and the function itself still returns the base prompt only.
        assert_eq!(system_prompt(&ws, false), base);
    }

