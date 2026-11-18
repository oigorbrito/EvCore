    fn user(text: &str) -> Message {
        Message::User {
            content: text.into(),
            images: Vec::new(),
        }
    }

    fn assistant(text: &str) -> Message {
        Message::Assistant {
            content: Some(text.into()),
            tool_calls: Vec::new(),
        }
    }

    /// A three-turn conversation: system, then user/assistant pairs.
    fn conversation() -> Vec<Message> {
        vec![
            Message::System {
                content: "sys".into(),
            },
            user("one"),
            assistant("a1"),
            user("two"),
            assistant("a2"),
            user("three"),
            assistant("a3"),
        ]
    }

    #[test]
    fn rewind_cuts_on_a_user_boundary() {
        let messages = conversation();
        // One turn: cut at the last user message, dropping it and what
        // followed it.
        assert_eq!(rewind_cut(&messages, 1), Some((5, 1)));
        assert_eq!(rewind_cut(&messages, 2), Some((3, 2)));
        // Every cut lands on a user message, never mid-turn.
        for turns in 1..=3 {
            let (cut, _) = rewind_cut(&messages, turns).unwrap();
            assert!(matches!(messages[cut], Message::User { .. }), "{turns}");
        }
    }

    #[test]
    fn rewinding_past_the_start_rewinds_everything_it_can() {
        let messages = conversation();
        // Three turns exist; asking for ten keeps the system prompt.
        assert_eq!(rewind_cut(&messages, 10), Some((1, 3)));
        let (cut, dropped) = rewind_cut(&messages, 10).unwrap();
        assert_eq!(dropped, 3);
        let kept = context_from(&messages[..cut]);
        assert_eq!(kept.messages().len(), 1);
        assert!(matches!(kept.messages()[0], Message::System { .. }));
    }

    #[test]
    fn rewinding_a_conversation_with_no_turns_is_a_no_op() {
        let only_system = vec![Message::System {
            content: "sys".into(),
        }];
        assert_eq!(rewind_cut(&only_system, 1), None);
        assert_eq!(rewind_cut(&[], 1), None);
    }

    /// The remaining transcript must never end in tool calls with no
    /// results — a chat-completions endpoint rejects that shape.
    #[test]
    fn rewind_never_leaves_dangling_tool_calls() {
        let messages = vec![
            Message::System {
                content: "sys".into(),
            },
            user("one"),
            Message::Assistant {
                content: None,
                tool_calls: vec![orca_harness_core::ToolCall {
                    id: "c1".into(),
                    name: "shell".into(),
                    arguments: serde_json::json!({}),
                }],
            },
            Message::Tool {
                results: vec![ToolResult {
                    call_id: "c1".into(),
                    tool_name: "shell".into(),
                    output: serde_json::json!("ok"),
                    is_error: false,
                }],
            },
            user("two"),
        ];
        let (cut, _) = rewind_cut(&messages, 1).unwrap();
        let kept = context_from(&messages[..cut]);
        // The call and its result are both kept, or both dropped.
        if let Some(Message::Assistant { tool_calls, .. }) = kept.messages().last() {
            assert!(tool_calls.is_empty());
        }
        assert_eq!(kept.messages().len(), 4);
    }

    #[test]
    fn context_from_copies_the_messages_it_is_given() {
        let messages = conversation();
        let rebuilt = context_from(&messages[..3]);
        assert_eq!(rebuilt.messages().len(), 3);
        assert_eq!(
            serde_json::to_string(rebuilt.messages()).unwrap(),
            serde_json::to_string(&messages[..3]).unwrap()
        );
    }

