use crate::platform::*;
use serde::Serialize;
use uuid::Uuid;
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Principal {
    pub tenant_id: Uuid,
    pub user_id: Uuid,
    pub roles: Vec<String>,
    pub email: String,
}
impl Principal {
    pub fn require(&self, role: &str) -> Result<()> {
        if self.roles.iter().any(|r| r == role) {
            Ok(())
        } else {
            Err(Error::forbidden())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_checks_require_an_exact_role() {
        let principal = Principal {
            tenant_id: Uuid::nil(),
            user_id: Uuid::nil(),
            roles: vec!["planner".into(), "approver".into()],
            email: "test@example.test".into(),
        };
        assert!(principal.require("planner").is_ok());
        assert!(principal.require("approver").is_ok());
        assert_eq!(principal.require("plan").unwrap_err().1, "forbidden");
    }
}
