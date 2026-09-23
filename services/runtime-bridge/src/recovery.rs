use anyhow::{Result, ensure};
use jcode_app_core::{
    message::{ContentBlock, Role},
    session::Session,
    session_store,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Checkpoint {
    after_message_id: String,
    text: String,
}

pub fn recover(session: &mut Session) -> Result<()> {
    let Some(checkpoint) =
        session_store::load_document::<Checkpoint>(&session.id, "stream_checkpoint")?
    else {
        return Ok(());
    };
    ensure!(
        checkpoint.text.len() <= 2 * 1024 * 1024,
        "Invalid stream checkpoint"
    );
    // The exact anchor makes recovery idempotent even if the process died
    // after saving the assistant message but before clearing its checkpoint.
    if session
        .messages
        .last()
        .is_some_and(|m| m.id == checkpoint.after_message_id)
        && !checkpoint.text.is_empty()
    {
        session.add_message(
            Role::Assistant,
            vec![ContentBlock::Text {
                text: checkpoint.text,
                cache_control: None,
            }],
        );
        session.save()?;
    }
    session_store::save_document(&session.id, "stream_checkpoint", &serde_json::Value::Null)
}
