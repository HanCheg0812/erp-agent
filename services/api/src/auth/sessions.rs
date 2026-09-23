use crate::platform::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use serde_json::{Value, json};
use tokio_postgres::GenericClient;
use uuid::Uuid;
pub fn token() -> String {
    let mut b = [0u8; 32];
    OsRng.fill_bytes(&mut b);
    URL_SAFE_NO_PAD.encode(b)
}
pub(super) async fn issue<C: GenericClient + Sync>(
    db: &C,
    tenant: Uuid,
    user: Uuid,
) -> Result<Value> {
    let token = token();
    let row=db.query_one("INSERT INTO auth_sessions(token_hash,tenant_id,user_id,expires_at) VALUES($1,$2,$3,now()+interval '8 hours') RETURNING expires_at::text",&[&digest(&token),&tenant,&user]).await?;
    Ok(json!({"access_token":token,"expires_at":row.get::<_,String>(0)}))
}
pub(super) async fn resolve(app: &App, token: &str) -> Result<super::Principal> {
    let db = app.db.get().await?;
    let row = db.query_opt("SELECT u.id,u.tenant_id,u.roles,u.email FROM auth_sessions s JOIN users u ON u.id=s.user_id AND u.tenant_id=s.tenant_id WHERE s.token_hash=$1 AND s.expires_at>now() AND u.active", &[&digest(token)]).await?.ok_or_else(Error::unauthorized)?;
    Ok(super::Principal {
        user_id: row.get(0),
        tenant_id: row.get(1),
        roles: row.get(2),
        email: row.get(3),
    })
}
pub(super) async fn revoke(app: &App, token: &str) -> Result<()> {
    app.db
        .get()
        .await?
        .execute(
            "DELETE FROM auth_sessions WHERE token_hash=$1",
            &[&digest(token)],
        )
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_tokens_are_url_safe_fixed_length_and_random() {
        let first = token();
        let second = token();
        assert_eq!(first.len(), 43);
        assert!(
            first
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        );
        assert_ne!(first, second);
    }
}
