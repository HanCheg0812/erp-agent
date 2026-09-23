use super::{
    leases::authorize,
    receipts::{receipt, retry},
};
use crate::platform::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
const KINDS: [&str; 4] = [
    "messages",
    "env_snapshots",
    "memory_injections",
    "replay_events",
];
pub async fn load(app: App, id: String, token: String) -> Result<Value> {
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    let (revision, fence, metadata) = authorize(&tx, &id, &token, false).await?;
    let mut state = metadata.ok_or_else(Error::missing)?;
    state["id"] = json!(id);
    let rows=tx.query("SELECT e.kind,e.payload FROM agent_session_entries e JOIN agent_session_heads h ON h.session_id=e.session_id AND h.kind=e.kind AND h.generation=e.generation WHERE e.session_id=$1 AND e.position<h.length ORDER BY e.kind,e.position",&[&id]).await?;
    for kind in KINDS {
        state[kind] = json!(
            rows.iter()
                .filter(|r| r.get::<_, String>(0) == kind)
                .map(|r| r.get::<_, Value>(1))
                .collect::<Vec<_>>()
        );
    }
    tx.commit().await?;
    Ok(json!({"revision":revision,"fence":fence,"state":state}))
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Vector {
    mode: String,
    values: Vec<Value>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Vectors {
    messages: Vector,
    env_snapshots: Vector,
    memory_injections: Vector,
    replay_events: Vector,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Delta {
    request_id: Uuid,
    expected_revision: i32,
    metadata: Value,
    vectors: Vectors,
}

fn validate_delta(id: &str, input: &Delta) -> Result<()> {
    if input.expected_revision < 0
        || input.expected_revision > 2_000_000_000
        || !input.metadata.is_object()
        || input.metadata["id"] != id
        || KINDS.iter().any(|kind| input.metadata.get(kind).is_some())
    {
        return Err(Error::bad("Invalid session metadata"));
    }
    for vector in [
        &input.vectors.messages,
        &input.vectors.env_snapshots,
        &input.vectors.memory_injections,
        &input.vectors.replay_events,
    ] {
        if !["replace", "append"].contains(&vector.mode.as_str()) || vector.values.len() > 10000 {
            return Err(Error::bad("Invalid vector delta"));
        }
    }
    Ok(())
}

fn vector_head(mode: &str, revision: i32, old: Option<(i32, i32)>) -> (i32, i32) {
    if mode == "replace" || old.is_none() {
        (revision, 0)
    } else {
        old.unwrap()
    }
}

pub async fn save(app: App, id: String, token: String, value: Value) -> Result<Value> {
    let input: Delta = parse(value)?;
    validate_delta(&id, &input)?;
    let vectors = [
        &input.vectors.messages,
        &input.vectors.env_snapshots,
        &input.vectors.memory_injections,
        &input.vectors.replay_events,
    ];
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    let (current, _, _) = authorize(&tx, &id, &token, true).await?;
    let hash = json_digest(&json!({"operation":"save","input":input}));
    if let Some(v) = retry(&tx, &id, input.request_id, &hash).await? {
        return Ok(v);
    }
    if current != input.expected_revision {
        return Err(Error::conflict("stale_revision"));
    }
    let revision = current + 1;
    let mut heads = vec![];
    for (kind, vector) in KINDS.iter().zip(vectors) {
        let old = tx
            .query_opt(
                "SELECT generation,length FROM agent_session_heads WHERE session_id=$1 AND kind=$2",
                &[&id, kind],
            )
            .await?;
        let old = old.map(|row| (row.get(0), row.get(1)));
        let (generation, start) = vector_head(&vector.mode, revision, old);
        tx.execute("INSERT INTO agent_session_entries(session_id,kind,generation,position,payload) SELECT $1,$2,$3,$4::integer+ordinality-1,value FROM jsonb_array_elements($5::jsonb) WITH ORDINALITY",&[&id,kind,&generation,&start,&json!(vector.values)]).await?;
        let length = start
            .checked_add(vector.values.len() as i32)
            .ok_or_else(|| Error::bad("Session size limit"))?;
        tx.execute("INSERT INTO agent_session_heads(session_id,kind,generation,length) VALUES($1,$2,$3,$4) ON CONFLICT(session_id,kind) DO UPDATE SET generation=excluded.generation,length=excluded.length",&[&id,kind,&generation,&length]).await?;
        heads.push(json!({"kind":kind,"generation":generation,"length":length}));
    }
    tx.execute("INSERT INTO agent_session_revisions(session_id,revision,metadata,heads) VALUES($1,$2,$3,$4)",&[&id,&revision,&input.metadata,&json!(heads)]).await?;
    tx.execute(
        "UPDATE agent_sessions SET revision=$2,metadata=$3,updated_at=now() WHERE id=$1",
        &[&id, &revision, &input.metadata],
    )
    .await?;
    let result = receipt(&tx, &id, input.request_id, &hash, revision).await?;
    tx.commit().await?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delta(id: &str) -> Delta {
        Delta {
            request_id: Uuid::nil(),
            expected_revision: 0,
            metadata: json!({"id":id,"title":"Fixture"}),
            vectors: Vectors {
                messages: Vector {
                    mode: "append".into(),
                    values: vec![],
                },
                env_snapshots: Vector {
                    mode: "append".into(),
                    values: vec![],
                },
                memory_injections: Vector {
                    mode: "replace".into(),
                    values: vec![],
                },
                replay_events: Vector {
                    mode: "append".into(),
                    values: vec![],
                },
            },
        }
    }

    #[test]
    fn delta_validation_binds_session_revision_metadata_and_vectors() {
        let id = "session_0123456789abcdef0123456789abcdef";
        assert!(validate_delta(id, &delta(id)).is_ok());
        let mut value = delta(id);
        value.expected_revision = -1;
        assert!(validate_delta(id, &value).is_err());
        let mut value = delta(id);
        value.expected_revision = 2_000_000_001;
        assert!(validate_delta(id, &value).is_err());
        let mut value = delta(id);
        value.metadata["id"] = json!("other");
        assert!(validate_delta(id, &value).is_err());
        let mut value = delta(id);
        value.metadata["messages"] = json!([]);
        assert!(validate_delta(id, &value).is_err());
        let mut value = delta(id);
        value.vectors.messages.mode = "merge".into();
        assert!(validate_delta(id, &value).is_err());
        let mut value = delta(id);
        value.vectors.messages.values = vec![Value::Null; 10001];
        assert!(validate_delta(id, &value).is_err());
    }

    #[test]
    fn replace_starts_a_generation_and_append_continues_the_existing_head() {
        assert_eq!(vector_head("replace", 7, Some((3, 5))), (7, 0));
        assert_eq!(vector_head("append", 7, None), (7, 0));
        assert_eq!(vector_head("append", 7, Some((3, 5))), (3, 5));
    }
}
