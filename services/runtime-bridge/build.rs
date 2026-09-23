use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::HashSet, env, fs, path::PathBuf};

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../skills/business");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(root.join("manifest.json")).unwrap()).unwrap();
    let mut names = HashSet::new();
    for entry in manifest["skills"].as_array_mut().unwrap() {
        let id = entry["id"].as_str().unwrap().to_string();
        assert!(
            !id.is_empty()
                && id.len() < 64
                && id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
            "Invalid skill ID"
        );
        assert!(names.insert(id.clone()), "Duplicate skill ID");
        let dir = root.join(&id);
        let path = dir.join("SKILL.md");
        assert!(
            !dir.is_symlink() && !path.is_symlink(),
            "Skill symlinks are not supported"
        );
        let source = fs::read_to_string(path).unwrap();
        assert!(
            !source.is_empty() && source.len() <= 16_384,
            "Skill size limit"
        );
        entry["sha256"] = format!("{:x}", Sha256::digest(source.as_bytes())).into();
        entry["source"] = source.into();
    }
    let snapshot = serde_json::to_string(&manifest).unwrap();
    assert!(snapshot.len() <= 65_536, "Catalog size limit");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("skill-catalog.json"),
        snapshot,
    )
    .unwrap();
}
