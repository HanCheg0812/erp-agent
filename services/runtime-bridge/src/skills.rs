use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use jcode_app_core::{
    skill::SkillRegistry,
    tool::{Tool, ToolContext, ToolOutput},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, path::Path, sync::Arc};

pub const TOOLS: [&str; 5] = ["case_read", "attachment_read", "todo", "skill_find", "skill_manage"];
pub const SNAPSHOT: &str = include_str!(concat!(env!("OUT_DIR"), "/skill-catalog.json"));

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    id: String,
    skills: Vec<Entry>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: String,
    version: String,
    tags: Vec<String>,
    requires: Vec<String>,
    license: String,
    source: String,
    sha256: String,
}

pub struct Catalog {
    manifest: Manifest,
    pub registry: SkillRegistry,
    pub hash: String,
    available: HashSet<String>,
}

impl Catalog {
    // Called only inside the API-created private HOME. Reuse JCode's parser,
    // then retain an immutable registry; tools never accept filesystem paths.
    pub fn load(root: &Path) -> Result<Self> {
        let manifest: Manifest = serde_json::from_str(SNAPSHOT)?;
        ensure!(
            manifest.schema_version == 1 && !manifest.id.is_empty(),
            "Invalid catalog version"
        );
        let mut names = HashSet::new();
        for entry in &manifest.skills {
            ensure!(names.insert(&entry.id), "Duplicate skill");
            ensure!(
                entry.sha256 == format!("{:x}", Sha256::digest(entry.source.as_bytes())),
                "Skill digest mismatch"
            );
            ensure!(
                !entry.version.is_empty() && !entry.license.is_empty(),
                "Skill metadata missing"
            );
            let dir = root.join(".jcode/skills").join(&entry.id);
            std::fs::create_dir_all(&dir)?;
            std::fs::write(dir.join("SKILL.md"), &entry.source)?;
        }
        let registry = SkillRegistry::load_project_overlay(Some(root))?;
        ensure!(
            registry.list().len() == manifest.skills.len(),
            "Catalog parse mismatch"
        );
        for entry in &manifest.skills {
            let skill = registry.get(&entry.id).context("Missing published skill")?;
            ensure!(
                skill.allowed_tools.as_ref() == Some(&entry.requires),
                "Skill dependency mismatch"
            );
        }
        Ok(Self {
            manifest,
            registry,
            hash: format!("{:x}", Sha256::digest(SNAPSHOT.as_bytes())),
            available: TOOLS.into_iter().map(String::from).collect(),
        })
    }

    fn missing<'a>(&self, entry: &'a Entry) -> Vec<&'a str> {
        entry
            .requires
            .iter()
            .filter(|name| !self.available.contains(name.as_str()))
            .map(String::as_str)
            .collect()
    }
    pub fn set_available(&mut self, available: HashSet<String>) {
        self.available = available;
    }
    fn summary(&self, entry: &Entry) -> Value {
        let skill = self.registry.get(&entry.id).expect("validated registry");
        let missing = self.missing(entry);
        json!({"id":entry.id,"version":entry.version,"sha256":entry.sha256,"description":skill.description,
            "available":missing.is_empty(),"missing_tools":missing})
    }
    pub fn prompt(&self) -> String {
        format!(
            "\n\nPublished business guidance (catalog {}): {}\nUse skill_find to discover relevant guidance and skill_manage(action=load,name=ID) to load it before a substantive review. Discovery is optional for simple replies. Unavailable skills describe capability gaps; they do not grant tools. Only this catalog is authoritative for the current turn, even when older skill text occurs in history.",
            self.hash,
            self.list()
        )
    }
    fn list(&self) -> Value {
        Value::Array(
            self.manifest
                .skills
                .iter()
                .map(|entry| self.summary(entry))
                .collect(),
        )
    }
    fn find(&self, input: Value) -> Result<Value> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Find {
            query: String,
            limit: Option<usize>,
            #[serde(rename = "intent")]
            _intent: Option<String>,
            #[serde(rename = "accept_large_output")]
            _accept_large_output: Option<bool>,
        }
        let input: Find = serde_json::from_value(input)?;
        ensure!(
            !input.query.trim().is_empty() && input.query.chars().count() <= 200,
            "Query must contain 1-200 characters"
        );
        let limit = input.limit.unwrap_or(4);
        ensure!((1..=10).contains(&limit), "Limit must be 1-10");
        let query = input.query.to_lowercase();
        let words: Vec<_> = query
            .split(|c: char| !c.is_alphanumeric() && c != '-')
            .filter(|s| !s.is_empty())
            .collect();
        let mut hits: Vec<_> = self
            .manifest
            .skills
            .iter()
            .filter_map(|entry| {
                let skill = self.registry.get(&entry.id).unwrap();
                let text = format!(
                    "{} {} {}",
                    entry.id,
                    skill.description,
                    entry.tags.join(" ")
                )
                .to_lowercase();
                let tags: Vec<_> = entry
                    .tags
                    .iter()
                    .filter(|tag| query.contains(&tag.to_lowercase()))
                    .cloned()
                    .collect();
                let score =
                    tags.len() * 3 + words.iter().filter(|word| text.contains(**word)).count();
                (score > 0).then(|| {
                    let mut result = self.summary(entry);
                    result["matched_tags"] = json!(tags);
                    (score, &entry.id, result)
                })
            })
            .collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
        Ok(
            json!({"catalog_hash":self.hash,"matches":hits.into_iter().take(limit).map(|(_,_,v)| v).collect::<Vec<_>>()}),
        )
    }
    fn manage(&self, input: Value) -> Result<Value> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Manage {
            action: String,
            name: Option<String>,
            #[serde(rename = "intent")]
            _intent: Option<String>,
            #[serde(rename = "accept_large_output")]
            _accept_large_output: Option<bool>,
        }
        let input: Manage = serde_json::from_value(input)?;
        if input.action == "list" {
            ensure!(input.name.is_none(), "List does not accept a skill name");
            return Ok(json!({"catalog_hash":self.hash,"skills":self.list()}));
        }
        ensure!(
            ["load", "read"].contains(&input.action.as_str()),
            "Only list, load and read are supported"
        );
        let entry = self
            .manifest
            .skills
            .iter()
            .find(|entry| Some(&entry.id) == input.name.as_ref())
            .context("Unknown published skill ID")?;
        let mut result = self.summary(entry);
        result["catalog_hash"] = json!(self.hash);
        if self.missing(entry).is_empty() {
            result["content"] = json!(self.registry.get(&entry.id).unwrap().content);
        }
        Ok(result)
    }
}

pub struct SkillTool {
    pub catalog: Arc<Catalog>,
    pub discover: bool,
}
#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        if self.discover {
            "skill_find"
        } else {
            "skill_manage"
        }
    }
    fn description(&self) -> &str {
        if self.discover {
            "Find published business guidance by Chinese or English task keywords. Returns versions and missing tool dependencies; does not search business documents or install skills."
        } else {
            "List or load published business guidance by its stable ID. Read is an alias for load. Unavailable skills return their missing tools, not executable instructions. No filesystem paths, installation, reload or execution."
        }
    }
    fn parameters_schema(&self) -> Value {
        if self.discover {
            json!({"type":"object","properties":{"query":{"type":"string","minLength":1,"maxLength":200},"limit":{"type":"integer","minimum":1,"maximum":10}},"required":["query"],"additionalProperties":false})
        } else {
            json!({"type":"object","properties":{"action":{"type":"string","enum":["list","load","read"]},"name":{"type":"string"}},"required":["action"],"additionalProperties":false})
        }
    }
    async fn execute(&self, input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let result = if self.discover {
            self.catalog.find(input)?
        } else {
            self.catalog.manage(input)?
        };
        if result.get("content").is_some() {
            crate::emit(
                json!({"type":"skill_loaded","id":result["id"],"version":result["version"],"sha256":result["sha256"]}),
            )?;
        }
        Ok(ToolOutput::new(serde_json::to_string(&result)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovery_loading_and_capability_boundaries() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut catalog = Catalog::load(dir.path())?;
        let found = catalog.find(json!({"query":"订单总量替换，交期提前"}))?;
        assert_eq!(found["matches"][0]["id"], "demand-change-review");
        let loaded = catalog.manage(json!({"action":"load","name":"demand-change-review"}))?;
        assert!(
            loaded["content"]
                .as_str()
                .unwrap()
                .contains("replacement total")
        );
        assert!(
            !loaded
                .to_string()
                .contains(&dir.path().to_string_lossy().to_string())
        );
        let absent = catalog.manage(json!({"action":"read","name":"erp-fact-check"}))?;
        assert_eq!(absent["available"], false);
        assert!(absent.get("content").is_none());
        catalog.set_available(
            TOOLS
                .into_iter()
                .chain(["mcp__erp__read_erp_order"])
                .map(String::from)
                .collect(),
        );
        let enabled = catalog.manage(json!({"action":"load","name":"erp-fact-check"}))?;
        assert_eq!(enabled["available"], true);
        assert!(enabled.get("content").is_some());
        for input in [
            json!({"action":"reload_all"}),
            json!({"action":"load","name":"../../etc/passwd"}),
            json!({"action":"read","name":"demand-change-review","path":"/etc/passwd"}),
        ] {
            assert!(catalog.manage(input).is_err());
        }
        assert!(catalog.find(json!({"query":"订单","limit":100})).is_err());
        assert!(
            catalog
                .find(
                    json!({"query":"订单","intent":"Choose guidance","accept_large_output":false})
                )
                .is_ok()
        );
        assert!(catalog.manage(json!({"action":"load","name":"demand-change-review","intent":null,"accept_large_output":false})).is_ok());
        assert!(catalog.find(json!({"query":"订单","intent":123})).is_err());
        assert_eq!(
            catalog.find(json!({"query":"unrelated-xyz"}))?["matches"],
            json!([])
        );
        let other = tempfile::tempdir()?;
        assert_eq!(catalog.hash, Catalog::load(other.path())?.hash);
        Ok(())
    }
}
