//! Deterministic native-loop probe. Runs only against an explicitly scoped test store.
use anyhow::{Result, ensure};
use async_trait::async_trait;
use jcode_app_core::{
    agent::Agent,
    message::{ContentBlock, Message, Role, StreamEvent, ToolDefinition},
    protocol::ServerEvent,
    provider::{EventStream, Provider},
    session::Session,
    tool::Registry,
};
use std::{io::Write, sync::Arc, time::Duration};

#[path = "../src/recovery.rs"]
mod recovery;

#[derive(Clone)]
struct ScriptedProvider {
    scenario: String,
}
#[async_trait]
impl Provider for ScriptedProvider {
    fn name(&self) -> &str {
        "fixture"
    }
    fn model(&self) -> String {
        "fixture".into()
    }
    fn set_model(&self, _: &str) -> Result<()> {
        Ok(())
    }
    fn available_models(&self) -> Vec<&'static str> {
        vec!["fixture"]
    }
    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(self.clone())
    }
    async fn complete(
        &self,
        messages: &[Message],
        _: &[ToolDefinition],
        _: &str,
        _: Option<&str>,
    ) -> Result<EventStream> {
        if self.scenario == "recall" {
            ensure!(messages.iter().any(|m| m.role == Role::Assistant && m.content.iter().any(|b| matches!(b, ContentBlock::Text { text, .. } if text == "Durable partial 7391"))), "Next turn did not receive the interrupted answer");
        }
        let scenario = self.scenario.clone();
        Ok(Box::pin(futures::stream::unfold(
            (0, scenario),
            |(step, scenario)| async move {
                let event = match step {
                    0 => Ok(StreamEvent::TextDelta(if scenario == "recall" {
                        "Recall verified".into()
                    } else {
                        "Durable partial 7391".into()
                    })),
                    1 if scenario == "stream_error" => {
                        Err(anyhow::anyhow!("Synthetic stream disconnect"))
                    }
                    1 if scenario == "event_error" => Ok(StreamEvent::Error {
                        message: "Synthetic provider error".into(),
                        retry_after_secs: None,
                    }),
                    1 if scenario == "eof" => return None,
                    1 if ["stop", "crash", "timeout"].contains(&scenario.as_str()) => {
                        std::future::pending::<()>().await;
                        unreachable!()
                    }
                    1 => Ok(StreamEvent::MessageEnd {
                        stop_reason: Some("end_turn".into()),
                    }),
                    _ => return None,
                };
                Some((event, (step + 1, scenario)))
            },
        )))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let scenario = std::env::args().nth(1).unwrap();
    let id = std::env::var("JCODE_SESSION_STORE_ID")?;
    let mut session = if scenario == "recall" {
        Session::load(&id)?
    } else {
        Session::create_with_id(id.clone(), None, Some("Synthetic stream recovery".into()))
    };
    if scenario == "recall" {
        recovery::recover(&mut session)?;
        let count = session.messages.len();
        recovery::recover(&mut session)?;
        ensure!(
            session.messages.len() == count,
            "Recovery duplicated the partial answer"
        );
    }
    let mut agent = Agent::new_with_session(
        Arc::new(ScriptedProvider {
            scenario: scenario.clone(),
        }),
        Registry::empty(),
        session,
        None,
    );
    agent.set_memory_enabled(false);
    let shutdown = agent.graceful_shutdown_signal();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mode = scenario.clone();
    let output = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let ServerEvent::TextDelta { .. } = event {
                println!("visible");
                std::io::stdout().flush().unwrap();
                if mode == "stop" {
                    shutdown.fire();
                }
                if mode == "timeout" {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    shutdown.fire();
                }
            }
        }
    });
    let result = agent
        .run_once_streaming_mpsc("Synthetic request", vec![], None, tx)
        .await;
    output.await?;
    ensure!(
        result.is_err() == ["stream_error", "event_error", "eof"].contains(&scenario.as_str()),
        "Unexpected outcome: {result:?}"
    );
    let saved = Session::load(&id)?;
    ensure!(saved.messages.iter().any(|m| m.role == Role::Assistant && m.content.iter().any(|b| matches!(b, ContentBlock::Text { text, .. } if text == "Durable partial 7391"))), "Partial response was not persisted");
    println!("PASS {scenario}");
    Ok(())
}
