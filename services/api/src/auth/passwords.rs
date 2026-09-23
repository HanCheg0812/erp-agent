use crate::platform::*;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::http::StatusCode;
use rand::rngs::OsRng;
use subtle::ConstantTimeEq;
pub(super) async fn password_work<T: Send + 'static>(
    app: &App,
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T> {
    let permit = app
        .passwords
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error(StatusCode::TOO_MANY_REQUESTS, "busy", "Try again shortly"))?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        f()
    })
    .await
    .map_err(|_| Error::bad("Password operation failed"))
}
pub(super) fn hash(password: &str) -> Result<String> {
    Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map(|v| v.to_string())
        .map_err(|_| Error::bad("Password operation failed"))
}
pub(super) fn verify(password: &str, encoded: &str) -> bool {
    if encoded.starts_with("$argon2id$") {
        return PasswordHash::new(encoded).is_ok_and(|h| {
            Argon2::default()
                .verify_password(password.as_bytes(), &h)
                .is_ok()
        });
    }
    let parts: Vec<_> = encoded.split(':').collect();
    if parts.len() != 3 || parts[0] != "scrypt" || parts[1].len() != 32 || parts[2].len() != 128 {
        return false;
    }
    let Ok(expected) = hex::decode(parts[2]) else {
        return false;
    };
    let mut out = [0u8; 64];
    scrypt::scrypt(
        password.as_bytes(),
        parts[1].as_bytes(),
        &scrypt::Params::new(14, 8, 1, 64).unwrap(),
        &mut out,
    )
    .is_ok()
        && bool::from(out.ct_eq(&expected))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argon2_hash_only_verifies_the_original_password() {
        let encoded = hash("correct horse battery staple").unwrap();
        assert!(encoded.starts_with("$argon2id$"));
        assert!(verify("correct horse battery staple", &encoded));
        assert!(!verify("different", &encoded));
    }

    #[test]
    fn legacy_scrypt_and_malformed_hashes_are_handled_without_panics() {
        let password = "legacy-password";
        let salt = "0123456789abcdef0123456789abcdef";
        let mut output = [0u8; 64];
        scrypt::scrypt(
            password.as_bytes(),
            salt.as_bytes(),
            &scrypt::Params::new(14, 8, 1, 64).unwrap(),
            &mut output,
        )
        .unwrap();
        let encoded = format!("scrypt:{salt}:{}", hex::encode(output));
        assert!(verify(password, &encoded));
        assert!(!verify("different", &encoded));
        for value in ["", "scrypt:short:00", "$argon2id$invalid", "unknown:a:b"] {
            assert!(!verify(password, value), "{value}");
        }
    }
}
