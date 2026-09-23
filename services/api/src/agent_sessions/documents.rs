use super::{
    leases::authorize,
    receipts::{receipt, retry},
};
use crate::platform::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
const DOCUMENTS: [&str; 6] = [
    "todos",
    "goals",
    "plan",
    "review",
    "gate_observations",
    "stream_checkpoint",
];

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Document {
    request_id: Uuid,
    expected_revision: i32,
    payload: Value,
}

fn parse_document(kind: &str, value: Value) -> Result<Document> {
    if !DOCUMENTS.contains(&kind) || serde_json::to_vec(&value).unwrap().len() > 2 * 1024 * 1024 {
        return Err(Error::bad("Invalid document"));
    }
    let input: Document = parse(value)?;
    if input.expected_revision < 0 || input.expected_revision > 2_000_000_000 {
        return Err(Error::bad("Invalid revision"));
    }
    Ok(input)
}

pub async fn document_get(app: App, (id, kind): (String, String), token: String) -> Result<Value> {
    if !DOCUMENTS.contains(&kind.as_str()) {
        return Err(Error::bad("Invalid document kind"));
    }
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    authorize(&tx, &id, &token, false).await?;
    let row=tx.query_opt("SELECT jsonb_build_object('revision',revision,'payload',payload) FROM agent_session_documents WHERE session_id=$1 AND kind=$2",&[&id,&kind]).await?;
    tx.commit().await?;
    Ok(row
        .map(|r| r.get(0))
        .unwrap_or(json!({"revision":0,"payload":null})))
}
pub async fn document_put(
    app: App,
    (id, kind): (String, String),
    token: String,
    value: Value,
) -> Result<Value> {
    let input = parse_document(&kind, value)?;
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    authorize(&tx, &id, &token, true).await?;
    let hash = json_digest(&json!({"operation":kind,"input":input}));
    if let Some(value) = retry(&tx, &id, input.request_id, &hash).await? {
        return Ok(value);
    }
    let current = tx
        .query_opt(
            "SELECT revision FROM agent_session_documents WHERE session_id=$1 AND kind=$2",
            &[&id, &kind],
        )
        .await?
        .map(|r| r.get::<_, i32>(0))
        .unwrap_or(0);
    if current != input.expected_revision {
        return Err(Error::conflict("stale_revision"));
    }
    let revision = current + 1;
    tx.execute("INSERT INTO agent_session_documents(session_id,kind,revision,payload) VALUES($1,$2,$3,$4) ON CONFLICT(session_id,kind) DO UPDATE SET revision=excluded.revision,payload=excluded.payload",&[&id,&kind,&revision,&input.payload]).await?;
    let result = receipt(&tx, &id, input.request_id, &hash, revision).await?;
    tx.commit().await?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_updates_accept_only_known_kinds_exact_fields_and_valid_revisions() {
        let value =
            || json!({"request_id":Uuid::nil(),"expected_revision":0,"payload":{"value":1}});
        for kind in DOCUMENTS {
            assert!(parse_document(kind, value()).is_ok(), "{kind}");
        }
        assert!(parse_document("arbitrary", value()).is_err());
        let mut invalid = value();
        invalid["expected_revision"] = json!(-1);
        assert!(parse_document("todos", invalid).is_err());
        let mut invalid = value();
        invalid["expected_revision"] = json!(2_000_000_001_i64);
        assert!(parse_document("todos", invalid).is_err());
        let mut invalid = value();
        invalid["extra"] = json!(true);
        assert!(parse_document("todos", invalid).is_err());
        let oversized = json!({"request_id":Uuid::nil(),"expected_revision":0,"payload":"x".repeat(2 * 1024 * 1024)});
        assert!(parse_document("todos", oversized).is_err());
    }
}
