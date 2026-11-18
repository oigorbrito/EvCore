#[cfg(test)]
mod main_tests {
    use std::path::PathBuf;

    use tokio::sync::mpsc;

    use orca_harness_core::{Context, Message, ToolResult};
    use orca_harness_extensions::MEMORY_GUIDANCE;
    use orca_harness_tools::{SubagentSpawn, Workspace};

    use crate::instructions;
    use crate::mode::{Mode, ModeHandle};
    use crate::msg::Provider;
    use crate::plan::PlanArea;
    use crate::runtime::{context_from, rewind_cut, subagent_extensions};
    use crate::{parse_run_args, resolve_theme, select_provider, system_prompt, Config, Planning};

    #[test]
    fn resume_subcommand_reuses_session_options() {
        for command in ["resume", "--resume"] {
            let cfg = parse_run_args(
                [
                    command,
                    "session-id",
                    "--workspace",
                    "/tmp/orca",
                    "--openrouter",
                ]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            )
            .unwrap();
            assert_eq!(cfg.resume_id.as_deref(), Some("session-id"));
            assert_eq!(cfg.workspace, PathBuf::from("/tmp/orca"));
            assert!(!cfg.continue_latest);
        }
        for args in [
            vec!["resume"],
            vec!["resume", "--workspace"],
            vec!["resume", ""],
        ] {
            assert!(parse_run_args(args.into_iter().map(str::to_owned).collect()).is_err());
        }
    }

    #[test]
    fn bare_prompt_is_small_and_enforces_the_requested_output_contract() {
        let prompt = crate::headless::bare_system_prompt(
            &Workspace::new(PathBuf::from(".")),
            &["read_file".into(), "grep".into()],
        );
        assert!(prompt.starts_with("You are EvCore, a coding agent"));
        assert!(prompt.contains("read_file, grep"));
        assert!(prompt.contains("entire response must be only that line"));
        assert!(prompt.contains("Batch independent reads"));
        assert!(!prompt.contains("subagent"));
        assert!(!prompt.contains("memory"));
    }

    #[test]
    fn headless_benchmark_flags_parse_without_changing_interactive_defaults() {
        let cfg = parse_run_args(
            [
                "--openrouter",
                "--model",
                "anthropic/claude-sonnet-4.6",
                "--effort",
                "high",
                "--prompt-cache",
                "--bare",
                "--tools",
                "read_file,grep,glob",
                "--max-output-tokens",
                "512",
                "-p",
                "inspect",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        )
        .unwrap();

        assert_eq!(cfg.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(cfg.max_output_tokens, Some(512));
        assert!(cfg.prompt_cache);
        assert!(cfg.bare);
        assert_eq!(cfg.tools.unwrap(), ["read_file", "grep", "glob"]);
    }

    #[test]
    fn prompt_cache_defaults_on_and_can_be_disabled() {
        let enabled = parse_run_args(
            ["--openrouter", "-p", "inspect"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        )
        .unwrap();
        assert!(enabled.prompt_cache);

        let disabled = parse_run_args(
            ["--openrouter", "--no-prompt-cache", "-p", "inspect"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        )
        .unwrap();
        assert!(!disabled.prompt_cache);
    }

    #[test]
    fn bare_and_tool_allowlists_are_headless_only() {
        let error = match parse_run_args(vec!["--bare".into()]) {
            Ok(_) => panic!("--bare without a prompt must fail"),
            Err(error) => error,
        };
        assert!(error.contains("require headless"), "{error}");
    }

    #[test]
    fn normal_is_default_and_auto_requires_its_flag() {
        let default = parse_run_args(Vec::new()).unwrap();
        assert_eq!(default.mode(), Mode::Normal);

        let cfg = parse_run_args(vec!["--auto".into()]).unwrap();
        assert_eq!(cfg.mode(), Mode::Auto);

        let normal = parse_run_args(vec!["--normal".into()]).unwrap();
        assert_eq!(normal.mode(), Mode::Normal);

        let legacy_auto_approve = parse_run_args(vec!["--auto-approve".into()]).unwrap();
        assert_eq!(legacy_auto_approve.mode(), Mode::Normal);
    }

    #[test]
    fn theme_prefers_explicit_then_stored_then_orca() {
        assert_eq!(resolve_theme(None), "orca");
        assert_eq!(crate::config::stored_theme(), None);
        assert_eq!(resolve_theme(Some("default".to_string())), "default");
        assert_eq!(resolve_theme(Some("mono".to_string())), "mono");
        crate::config::save_theme("nord").unwrap();
        assert_eq!(resolve_theme(None), "nord");
        assert_eq!(resolve_theme(Some("mono".to_string())), "mono");
    }

    include!("main_tests/prompt.rs");
    include!("main_tests/rewind.rs");
    include!("main_tests/planning.rs");
    include!("main_tests/startup.rs");
}
