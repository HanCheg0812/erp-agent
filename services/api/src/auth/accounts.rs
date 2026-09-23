use super::{
    passwords::{hash, password_work, verify},
    sessions::issue,
};
use crate::platform::*;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Login {
    tenant: Option<String>,
    email: String,
    password: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Register {
    email: String,
    password: String,
}
fn email(value: &str) -> Result<String> {
    let value = value.trim().to_lowercase();
    if value.len() > 254 || !email_address::EmailAddress::is_valid(&value) {
        return Err(Error::bad("Enter a valid email address"));
    }
    Ok(value)
}
fn throttle(app: &App, email: &str) -> Result<()> {
    let mut map = app.attempts.lock().unwrap();
    map.retain(|_, v| v.0.elapsed().as_secs() < 60);
    for (key, limit) in [("global".to_owned(), 200), (digest(email), 10)] {
        let value = map.entry(key).or_insert((std::time::Instant::now(), 0));
        value.1 += 1;
        if value.1 > limit {
            return Err(Error(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "Too many attempts; try again later",
            ));
        }
    }
    Ok(())
}
pub async fn register(app: &App, value: Value) -> Result<Value> {
    let input: Register = parse(value)?;
    let email = email(&input.email)?;
    throttle(&app, &email)?;
    if !(10..=256).contains(&input.password.chars().count()) {
        return Err(Error::bad("Password must contain 10 to 256 characters"));
    }
    let hashed = password_work(&app, move || hash(&input.password)).await??;
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    tx.query_one(
        "SELECT pg_advisory_xact_lock(hashtextextended($1,0))",
        &[&format!("register:{email}")],
    )
    .await?;
    if tx
        .query_opt(
            "SELECT id FROM users WHERE lower(email)=$1 LIMIT 1",
            &[&email],
        )
        .await?
        .is_some()
    {
        return Err(Error(
            StatusCode::CONFLICT,
            "registration_unavailable",
            "Registration is unavailable for this email",
        ));
    }
    let tenant = Uuid::new_v4();
    let user = Uuid::new_v4();
    tx.execute(
        "INSERT INTO tenants(id,slug) VALUES($1,$2)",
        &[&tenant, &format!("personal-{}", tenant.simple())],
    )
    .await?;
    tx.execute("INSERT INTO users(id,tenant_id,email,password_hash,roles) VALUES($1,$2,$3,$4,ARRAY['planner'])",&[&user,&tenant,&email,&hashed]).await?;
    tx.execute(
        "INSERT INTO personal_accounts(email,user_id) VALUES($1,$2)",
        &[&email, &user],
    )
    .await?;
    let output = issue(&*tx, tenant, user).await?;
    tx.commit().await?;
    Ok(output)
}
pub async fn login(app: &App, value: Value) -> Result<Value> {
    let input: Login = parse(value)?;
    let email = email(&input.email)?;
    throttle(&app, &email)?;
    text(&input.password, 256)?;
    let db = app.db.get().await?;
    let rows=db.query("SELECT u.id,u.tenant_id,u.password_hash FROM users u JOIN tenants t ON t.id=u.tenant_id WHERE lower(u.email)=$1 AND u.active AND ($2::text IS NULL OR t.slug=$2) LIMIT 2",&[&email,&input.tenant]).await?;
    let stored = if rows.len() == 1 {
        rows[0].get::<_, String>(2)
    } else {
        format!("scrypt:{}:{}", "0".repeat(32), "0".repeat(128))
    };
    let legacy = stored.starts_with("scrypt:");
    let password = input.password;
    let checked = password_work(&app, move || {
        let valid = verify(&password, &stored);
        let upgrade = if valid && legacy {
            Some(hash(&password))
        } else {
            None
        };
        (valid, upgrade)
    })
    .await?;
    if !checked.0 || rows.len() != 1 {
        return Err(Error::unauthorized());
    }
    let user: Uuid = rows[0].get(0);
    let tenant: Uuid = rows[0].get(1);
    if let Some(hash) = checked.1 {
        db.execute(
            "UPDATE users SET password_hash=$2 WHERE id=$1",
            &[&user, &hash?],
        )
        .await?;
    }
    issue(&**db, tenant, user).await
}
pub async fn seed(app: &App) -> anyhow::Result<()> {
    let email = email(&std::env::var("DEMO_EMAIL")?)?;
    let password = std::env::var("DEMO_PASSWORD")?;
    let tenant = std::env::var("DEMO_TENANT")?;
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    let row=tx.query_one("INSERT INTO tenants(slug) VALUES($1) ON CONFLICT(slug) DO UPDATE SET slug=excluded.slug RETURNING id",&[&tenant]).await?;
    tx.execute("INSERT INTO users(tenant_id,email,password_hash,roles) VALUES($1,$2,$3,ARRAY['planner','approver']) ON CONFLICT(tenant_id,email) DO NOTHING",&[&row.get::<_,Uuid>(0),&email,&hash(&password)?]).await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_is_normalized_and_rejects_invalid_or_oversized_values() {
        assert_eq!(
            email("  Person+Tag@Example.COM ").unwrap(),
            "person+tag@example.com"
        );
        for value in ["", "not-an-email", "a@", "@example.com"] {
            assert!(email(value).is_err(), "{value}");
        }
        assert!(email(&format!("{}@example.com", "a".repeat(255))).is_err());
    }
}
