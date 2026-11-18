//! Interrupted-run checks: a transient model error, partial failure and cancellation.

use super::*;

/// Inject one model transport error, then exercise partial model failure and
/// caller cancellation after a metered tool step (not just before starting).
struct RetryOnce {
    calls: AtomicUsize,
}
#[async_trait]
impl Model for RetryOnce {
    async fn generate(
        &self,
        _: &Context,
        _: &[ToolSchema],
    ) -> std::result::Result<ModelResponse, ModelError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            Err(ModelError::Request("temporary transport error".into()))
        } else {
            Ok(ModelResponse::final_text("retry recovered"))
        }
    }
}

pub(super) async fn interrupted_checks(harness: &Harness) -> Result {
    let model = Arc::new(RetryOnce {
        calls: AtomicUsize::new(0),
    });
    let agent = harness
        .agent(model.clone())
        .model_retry(RetryConfig::attempts(2).backoff_ms(0))
        .build()?;
    let session = agent.new_session().ephemeral().open()?;
    assert_eq!(
        session.run("retry release check").await?.text,
        "retry recovered"
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 2);
    session.shutdown(WAIT).await?;
    for cancel in [false, true] {
        let token = CancellationToken::new();
        let stop = token.clone();
        let tool = FnTool::new(
            "check",
            "Checkpoint",
            json!({"type":"object"}),
            move |_, ctx| {
                let stop = stop.clone();
                async move {
                    if cancel {
                        stop.cancel();
                        ctx.cancellation.cancelled().await;
                    }
                    Ok(json!({"checkpoint":"release checked"}))
                }
            },
        );
        let agent = harness
            .agent(ScriptedModel::new(vec![step(vec![call(
                "check",
                "check",
                json!({}),
            )])]))
            .tool(tool)
            .build()?;
        let session = agent.new_session().ephemeral().open()?;
        let outcome = session
            .run_outcome(RunRequest::new("check release").cancellation(token))
            .await?;
        assert!(matches!(
            &outcome.execution,
            Err(SdkError::Harness(error))
                if if cancel {
                    matches!(error, HarnessError::Cancelled)
                } else {
                    matches!(error, HarnessError::Model(_))
                }
        ));
        assert_eq!(outcome.metered_steps, 1);
        assert_eq!(outcome.usage.input_tokens, 10);
        assert!(outcome.persistence.is_ok());
        if !cancel {
            assert_eq!(
                tool_output(&outcome.messages, "check")["checkpoint"],
                "release checked"
            );
        }
        session.shutdown(WAIT).await?;
    }
    println!("Model retry, partial failure, and cooperative cancellation asserted.");
    Ok(())
}
