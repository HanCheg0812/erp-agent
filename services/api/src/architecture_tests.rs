use std::{fs, path::Path};

fn sources(directory: &Path) -> Vec<(String, String)> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(sources(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push((
                path.display().to_string(),
                fs::read_to_string(path).unwrap(),
            ));
        }
    }
    files
}

#[test]
fn http_adapters_do_not_perform_business_or_storage_work() {
    let files = sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));
    let mut count = 0;
    for (path, source) in files {
        if !path.ends_with("/handlers.rs") {
            continue;
        }
        count += 1;
        for forbidden in [
            "app.db",
            "app.http",
            "app.bucket",
            ".transaction(",
            ".build_transaction(",
            ".require(",
            "SELECT ",
            "INSERT ",
            "UPDATE ",
            "DELETE ",
        ] {
            assert!(!source.contains(forbidden), "{path} contains {forbidden}");
        }
    }
    assert!(
        count >= 8,
        "Expected all business HTTP adapters to be checked"
    );
}

#[test]
fn module_roots_only_declare_and_export_modules() {
    for (path, source) in sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src")) {
        if path.ends_with("/mod.rs") {
            for forbidden in ["fn ", "struct ", "impl ", "SELECT ", "INSERT "] {
                assert!(!source.contains(forbidden), "{path} contains {forbidden}");
            }
        }
    }
}

#[test]
fn layered_use_cases_do_not_execute_queries_or_depend_on_http() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for module in ["cases", "conversation", "permissions", "objects"] {
        for (path, source) in sources(&root.join(module).join("application")) {
            for forbidden in [
                "SELECT ",
                "INSERT ",
                "UPDATE ",
                "DELETE ",
                ".query(",
                ".query_one(",
                ".query_opt(",
                ".execute(",
                "Json<",
                "HeaderMap",
                "State<",
                "Extension<",
            ] {
                assert!(!source.contains(forbidden), "{path} contains {forbidden}");
            }
        }
        for (path, source) in sources(&root.join(module).join("repository")) {
            for forbidden in [
                ".require(",
                "actor.roles",
                ".commit(",
                ".transaction(",
                ".db.get(",
                ".http",
                "use axum",
                "application::",
            ] {
                assert!(!source.contains(forbidden), "{path} contains {forbidden}");
            }
        }
    }
}

#[test]
fn every_api_module_is_listed_in_the_test_matrix() {
    let manifest =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("TESTING.md")).unwrap();
    for module in [
        "agent_sessions",
        "auth",
        "cases",
        "conversation",
        "integrations",
        "objects",
        "permissions",
        "platform",
        "sandboxes",
        "worker",
    ] {
        assert!(
            manifest.contains(&format!("| `{module}` |")),
            "Missing test evidence for {module}"
        );
    }
}

#[test]
fn binary_entrypoint_only_delegates_to_the_library() {
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs")).unwrap();
    assert!(source.contains("erp_agent_api::run().await"));
    for forbidden in [
        "mod ",
        "Router",
        ".route(",
        "App::connect",
        "SELECT ",
        "INSERT ",
    ] {
        assert!(
            !source.contains(forbidden),
            "Binary entrypoint contains {forbidden}"
        );
    }
}
