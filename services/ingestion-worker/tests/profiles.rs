use async_trait::async_trait;
use erp_ingestion::{backend::*, profile::*};
use serde_json::{Value, json};

struct FixtureBackend(ParserCapabilities);
fn backend(name: &str, model: &str) -> FixtureBackend {
    FixtureBackend(ParserCapabilities {
        backend: name.into(),
        model_refs: vec![model.into()],
        mime_types: vec!["application/pdf".into()],
        regions: false,
        tables: true,
        coordinates: true,
        asynchronous: true,
        cancellation: false,
        remote_idempotency: true,
    })
}
#[async_trait]
impl ParseBackend for FixtureBackend {
    fn capabilities(&self) -> &ParserCapabilities {
        &self.0
    }
    fn validate_options(&self, options: &Value) -> Result<(), ParseError> {
        let valid = options.as_object().is_some_and(|map| {
            map.iter().all(|(key, value)| match key.as_str() {
                "language" => value.is_string(),
                "pages" => value
                    .as_array()
                    .is_some_and(|items| items.iter().all(Value::is_u64)),
                _ => false,
            })
        });
        if valid {
            Ok(())
        } else {
            Err(ParseError {
                kind: ErrorKind::Configuration,
                message: "Unknown or invalid fixture option".into(),
                retry_after_seconds: None,
            })
        }
    }
    async fn submit(&self, request: &ParseRequest) -> Result<Submission, ParseError> {
        Ok(Submission::Pending(RemoteJob {
            backend: self.0.backend.clone(),
            remote_id: "fixture-job".into(),
            idempotency_key: request.idempotency_key.clone(),
        }))
    }
    async fn poll(&self, _handle: &RemoteJob) -> Result<JobProgress, ParseError> {
        Ok(JobProgress::Pending)
    }
    async fn cancel(&self, _handle: &RemoteJob) -> Result<CancelOutcome, ParseError> {
        Ok(CancelOutcome::Unsupported)
    }
}
fn bundle() -> Value {
    json!({"schema_version":1,"profiles":[
        {"id":"base@1","abstract":true,"settings":{"timeout_seconds":120},"steps":{
            "extract":{"enabled":true,"backend":"fixture-a","model_ref":"model-a@1","options":{"language":"auto","pages":[1,2]}},
            "validate":{"enabled":true,"ruleset":"evidence@1","depends_on":["extract"]}
        }},
        {"id":"child@1","extends":"base@1","steps":{"extract":{"options":{"pages":[3]}}}}
    ]})
}
#[test]
fn inheritance_arrays_stable_snapshots_and_backend_switch() {
    let a = backend("fixture-a", "model-a@1");
    let b = backend("fixture-b", "model-b@1");
    let mut input = bundle();
    let first = resolve_profile(&input.to_string(), "child@1", &[&a, &b]).unwrap();
    assert_eq!(first.profile.timeout_seconds, 120);
    assert_eq!(
        first.profile.steps[&Stage::Extract].options,
        json!({"language":"auto","pages":[3]})
    );
    let reordered = serde_json::to_string_pretty(&input).unwrap();
    assert_eq!(
        first.hash,
        resolve_profile(&reordered, "child@1", &[&a, &b])
            .unwrap()
            .hash
    );
    input["profiles"][1]["steps"]["extract"] =
        json!({"backend":"fixture-b","model_ref":"model-b@1"});
    let next = resolve_profile(&input.to_string(), "child@1", &[&a, &b]).unwrap();
    assert_eq!(next.profile.steps[&Stage::Extract].options, json!({}));
    assert_ne!(first.hash, next.hash);
    assert!(first.canonical_json.contains("model-a@1"));
}
#[test]
fn invalid_graphs_permissions_options_and_models_are_rejected() {
    let a = backend("fixture-a", "model-a@1");
    assert!(resolve_profile(&bundle().to_string(), "base@1", &[&a]).is_err());
    for patch in [
        json!({"extends":"missing"}),
        json!({"extends":"child@1"}),
        json!({"tenant_id":"other"}),
        json!({"settings":{"retain_original":false}}),
        json!({"settings":{"timeout_seconds":0}}),
        json!({"steps":{"unknown":{"enabled":true}}}),
        json!({"steps":{"extract":{"options":{"language":null}}}}),
        json!({"steps":{"extract":{"options":{"language":123}}}}),
        json!({"steps":{"extract":{"options":{"shell":"bad"}}}}),
        json!({"steps":{"extract":{"model_ref":"wrong-model"}}}),
        json!({"steps":{"extract":{"backend":"missing"}}}),
        json!({"steps":{"extract":{"enabled":false}}}),
        json!({"steps":{"extract":{"depends_on":["validate"]}}}),
        json!({"steps":{"validate":{"enabled":false}}}),
        json!({"steps":{"validate":{"depends_on":[]}}}),
        json!({"steps":{"vision":{"enabled":true,"backend":"fixture-a","model_ref":"model-a@1"}}}),
    ] {
        let mut input = bundle();
        input["profiles"][1]
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        assert!(
            resolve_profile(&input.to_string(), "child@1", &[&a]).is_err(),
            "accepted {patch}"
        );
    }
    let mut input = bundle();
    input["profiles"][0]["extends"] = json!("child@1");
    assert!(resolve_profile(&input.to_string(), "child@1", &[&a]).is_err());
}
#[test]
fn validation_waits_for_all_stages_through_transitive_dependencies() {
    let a = backend("fixture-a", "model-a@1");
    let mut input = bundle();
    input["profiles"][1]["steps"]["vision"] = json!({
        "enabled":true,"backend":"fixture-a","model_ref":"model-a@1","depends_on":["extract"]
    });
    input["profiles"][1]["steps"]["validate"] = json!({"depends_on":["vision"]});
    let resolved = resolve_profile(&input.to_string(), "child@1", &[&a]).unwrap();
    assert_eq!(
        resolved.profile.steps[&Stage::Validate].depends_on,
        [Stage::Vision]
    );
}
#[test]
fn retry_keys_preserve_attempt_identity_and_separate_experiments_and_tenants() {
    let mut identity = StepIdentity {
        tenant_id: "t1",
        revision_id: "r1",
        input_sha256: "input",
        profile_hash: "profile",
        step_id: "extract",
        replicate_id: "production",
    };
    let first = identity.idempotency_key().unwrap();
    assert_eq!(first, identity.idempotency_key().unwrap());
    identity.replicate_id = "experiment-2";
    assert_ne!(first, identity.idempotency_key().unwrap());
    identity.replicate_id = "production";
    identity.tenant_id = "t2";
    assert_ne!(first, identity.idempotency_key().unwrap());
}
#[tokio::test]
async fn remote_handle_can_roundtrip_for_recovery_without_resubmitting() {
    let backend: Box<dyn ParseBackend> = Box::new(backend("fixture-a", "model-a@1"));
    let request = ParseRequest {
        tenant_id: "t1".into(),
        revision_id: "r1".into(),
        run_id: "run1".into(),
        step_id: "extract".into(),
        input: ArtifactRef {
            artifact_id: "original".into(),
            sha256: "input-hash".into(),
            bytes: 123,
            mime_type: "application/pdf".into(),
        },
        profile_hash: "profile-hash".into(),
        model_ref: "model-a@1".into(),
        options: json!({}),
        idempotency_key: "retry-key".into(),
        deadline_unix_ms: 1,
    };
    let Submission::Pending(handle) = backend.submit(&request).await.unwrap() else {
        panic!("expected handle");
    };
    let restored: RemoteJob =
        serde_json::from_slice(&serde_json::to_vec(&handle).unwrap()).unwrap();
    assert_eq!(restored.idempotency_key, request.idempotency_key);
    assert!(matches!(
        backend.poll(&restored).await.unwrap(),
        JobProgress::Pending
    ));
    assert!(matches!(
        backend.cancel(&restored).await.unwrap(),
        CancelOutcome::Unsupported
    ));
}
