use crate::backend::ParseBackend;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Extract,
    Vision,
    Validate,
}

#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsPatch {
    timeout_seconds: Option<u32>,
    retain_original: Option<bool>,
}

#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepPatch {
    enabled: Option<bool>,
    backend: Option<String>,
    model_ref: Option<String>,
    options: Option<Value>,
    ruleset: Option<String>,
    depends_on: Option<Vec<Stage>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    id: String,
    extends: Option<String>,
    #[serde(default, rename = "abstract")]
    abstract_profile: bool,
    #[serde(default)]
    settings: SettingsPatch,
    #[serde(default)]
    steps: BTreeMap<Stage, StepPatch>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileSet {
    schema_version: u32,
    profiles: Vec<Profile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub enabled: bool,
    pub backend: Option<String>,
    pub model_ref: Option<String>,
    pub options: Value,
    pub ruleset: Option<String>,
    pub depends_on: Vec<Stage>,
}
impl Default for Step {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: None,
            model_ref: None,
            options: json!({}),
            ruleset: None,
            depends_on: vec![],
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedProfile {
    pub schema_version: u32,
    pub id: String,
    pub timeout_seconds: u32,
    pub retain_original: bool,
    pub steps: BTreeMap<Stage, Step>,
}
pub struct ProfileSnapshot {
    pub profile: ResolvedProfile,
    pub canonical_json: String,
    pub hash: String,
}

fn reject_null(value: &Value) -> Result<()> {
    match value {
        Value::Null => bail!("Null is not a profile override; use enabled=false to disable a step"),
        Value::Object(map) => {
            for value in map.values() {
                reject_null(value)?;
            }
        }
        Value::Array(items) => {
            for value in items {
                reject_null(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn merge_options(base: &mut Value, patch: &Value) -> Result<()> {
    if let (Value::Object(base), Value::Object(patch)) = (&mut *base, patch) {
        for (key, value) in patch {
            if let Some(previous) = base.get_mut(key) {
                merge_options(previous, value)?;
            } else {
                base.insert(key.clone(), value.clone());
            }
        }
    } else {
        ensure!(
            std::mem::discriminant(base) == std::mem::discriminant(patch),
            "Option override changes value type"
        );
        *base = patch.clone();
    }
    Ok(())
}
fn expand(
    id: &str,
    profiles: &BTreeMap<&str, &Profile>,
    visiting: &mut BTreeSet<String>,
) -> Result<ResolvedProfile> {
    ensure!(
        visiting.insert(id.into()),
        "Profile inheritance cycle at {id}"
    );
    let profile = profiles.get(id).context("Unknown parent or profile ID")?;
    let mut resolved = if let Some(parent) = &profile.extends {
        expand(parent, profiles, visiting)?
    } else {
        ResolvedProfile {
            schema_version: 1,
            id: id.into(),
            timeout_seconds: 180,
            retain_original: true,
            steps: BTreeMap::new(),
        }
    };
    resolved.id = id.into();
    if let Some(timeout) = profile.settings.timeout_seconds {
        resolved.timeout_seconds = timeout;
    }
    if let Some(retain) = profile.settings.retain_original {
        resolved.retain_original = retain;
    }
    ensure!(
        resolved.retain_original,
        "Original retention cannot be disabled"
    );
    ensure!(
        (1..=3600).contains(&resolved.timeout_seconds),
        "Timeout outside 1-3600 seconds"
    );
    for (id, patch) in &profile.steps {
        let step = resolved.steps.entry(*id).or_default();
        if let Some(backend) = &patch.backend {
            if step.backend.as_ref() != Some(backend) {
                step.options = json!({});
                step.model_ref = None;
            }
            step.backend = Some(backend.clone());
        }
        if let Some(enabled) = patch.enabled {
            step.enabled = enabled;
        }
        if let Some(model) = &patch.model_ref {
            step.model_ref = Some(model.clone());
        }
        if let Some(ruleset) = &patch.ruleset {
            step.ruleset = Some(ruleset.clone());
        }
        if let Some(dependencies) = &patch.depends_on {
            step.depends_on = dependencies.clone();
        }
        if let Some(options) = &patch.options {
            ensure!(options.is_object(), "Backend options must be an object");
            merge_options(&mut step.options, options)?;
        }
    }
    visiting.remove(id);
    Ok(resolved)
}
fn check_dependencies(
    id: Stage,
    steps: &BTreeMap<Stage, Step>,
    visiting: &mut BTreeSet<Stage>,
) -> Result<()> {
    ensure!(visiting.insert(id), "Step dependency cycle");
    let step = steps.get(&id).context("Missing step dependency")?;
    ensure!(step.enabled, "An enabled step depends on a disabled step");
    let mut seen = BTreeSet::new();
    for dependency in &step.depends_on {
        ensure!(seen.insert(*dependency), "Duplicate step dependency");
        check_dependencies(*dependency, steps, visiting)?;
    }
    visiting.remove(&id);
    Ok(())
}

pub fn resolve_profile(
    source: &str,
    id: &str,
    backends: &[&dyn ParseBackend],
) -> Result<ProfileSnapshot> {
    ensure!(source.len() <= 262_144, "Profile bundle size limit");
    let value: Value = serde_json::from_str(source)?;
    reject_null(&value)?;
    let bundle: ProfileSet = serde_json::from_value(value)?;
    ensure!(
        bundle.schema_version == 1 && (1..=64).contains(&bundle.profiles.len()),
        "Unsupported profile bundle"
    );
    let mut profiles = BTreeMap::new();
    for profile in &bundle.profiles {
        ensure!(
            !profile.id.is_empty() && profile.id.len() <= 100,
            "Invalid profile ID"
        );
        ensure!(
            profiles.insert(profile.id.as_str(), profile).is_none(),
            "Duplicate profile ID"
        );
    }
    ensure!(
        !profiles
            .get(id)
            .context("Unknown profile ID")?
            .abstract_profile,
        "Abstract profile cannot execute"
    );
    let profile = expand(id, &profiles, &mut BTreeSet::new())?;
    let mut registry = BTreeMap::new();
    for backend in backends {
        ensure!(
            registry
                .insert(backend.capabilities().backend.as_str(), backend)
                .is_none(),
            "Duplicate backend registration"
        );
    }
    ensure!(
        profile
            .steps
            .get(&Stage::Validate)
            .is_some_and(|step| step.enabled),
        "Validation step is required"
    );
    ensure!(
        [Stage::Extract, Stage::Vision]
            .iter()
            .any(|stage| profile.steps.get(stage).is_some_and(|step| step.enabled)),
        "Extraction or vision step is required"
    );
    for (id, step) in profile.steps.iter().filter(|(_, step)| step.enabled) {
        check_dependencies(*id, &profile.steps, &mut BTreeSet::new())?;
        if *id == Stage::Validate {
            ensure!(
                step.ruleset.as_ref().is_some_and(|s| !s.is_empty()),
                "Validation ruleset required"
            );
            ensure!(
                step.backend.is_none() && step.model_ref.is_none() && step.options == json!({}),
                "Validation does not accept a model backend"
            );
        } else {
            ensure!(step.ruleset.is_none(), "Ruleset belongs to validation step");
            let backend = registry
                .get(step.backend.as_deref().context("Missing backend")?)
                .context("Unregistered backend")?;
            let model = step.model_ref.as_ref().context("Missing model reference")?;
            ensure!(
                backend.capabilities().model_refs.contains(model),
                "Model reference incompatible with backend"
            );
            backend.validate_options(&step.options)?;
        }
    }
    let mut required = BTreeSet::new();
    let mut pending = profile.steps[&Stage::Validate].depends_on.clone();
    while let Some(stage) = pending.pop() {
        if required.insert(stage) {
            pending.extend(&profile.steps[&stage].depends_on);
        }
    }
    ensure!(
        profile.steps.iter().all(|(stage, step)| {
            !step.enabled || *stage == Stage::Validate || required.contains(stage)
        }),
        "Validation must depend on every enabled parsing step"
    );
    // serde_json's default map is sorted; serialize a Value to canonicalize
    // struct keys too. Arrays retain order because their order is meaningful.
    let canonical_json = serde_json::to_string(&serde_json::to_value(&profile)?)?;
    let hash = format!("{:x}", Sha256::digest(canonical_json.as_bytes()));
    Ok(ProfileSnapshot {
        profile,
        canonical_json,
        hash,
    })
}

#[derive(Serialize)]
pub struct StepIdentity<'a> {
    pub tenant_id: &'a str,
    pub revision_id: &'a str,
    pub input_sha256: &'a str,
    pub profile_hash: &'a str,
    pub step_id: &'a str,
    pub replicate_id: &'a str,
}
impl StepIdentity<'_> {
    pub fn idempotency_key(&self) -> Result<String> {
        ensure!(
            [
                self.tenant_id,
                self.revision_id,
                self.input_sha256,
                self.profile_hash,
                self.step_id,
                self.replicate_id
            ]
            .iter()
            .all(|value| !value.is_empty()),
            "Incomplete step identity"
        );
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }
}
