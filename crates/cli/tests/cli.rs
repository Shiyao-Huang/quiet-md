use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_library() -> PathBuf {
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let path = std::env::temp_dir().join(format!("quiet-cli-test-{}-{n}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quiet"))
        .args(["--library", root.to_str().unwrap(), "--format", "json"])
        .args(args)
        .output()
        .unwrap()
}

fn data(output: &Output) -> Value {
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice::<Value>(&output.stdout).unwrap()["data"].clone()
}

#[test]
fn cli_covers_document_lifecycle_and_json_contract() {
    let root = temp_library();
    let created = run(&root, &["docs", "+create", "--title", "CLI note", "--content", "# CLI note\n\nhello quiet", "--content-format", "markdown"]);
    let created_data = data(&created);
    let id = created_data["document"]["id"].as_str().unwrap().to_string();
    assert_eq!(created_data["document"]["title"], "CLI note");
    let metadata = data(&run(&root, &["docs", "+metadata", &id, "--icon", "📝", "--cover", "/tmp/cover.png"]));
    assert_eq!(metadata["icon"], "📝");
    let metadata_read = data(&run(&root, &["docs", "+get", &id]));
    assert_eq!(metadata_read["document"]["icon"], "📝");
    assert_eq!(metadata_read["document"]["cover"]["src"], "/tmp/cover.png");

    let listed = data(&run(&root, &["docs", "+list"]));
    assert_eq!(listed["items"].as_array().unwrap().len(), 1);
    assert_eq!(listed["items"][0]["id"], id);

    let found = data(&run(&root, &["docs", "+search", "quiet"]));
    assert_eq!(found["items"].as_array().unwrap().len(), 1);

    let exported = data(&run(&root, &["docs", "+export", &id, "--as", "markdown"]));
    assert!(exported["content"].as_str().unwrap().contains("hello quiet"));

    let updated = run(&root, &["docs", "+append", &id, "--content", "\nappended", "--content-format", "markdown"]);
    assert!(updated.status.success(), "stderr: {}", String::from_utf8_lossy(&updated.stderr));
    let fetched = data(&run(&root, &["docs", "+get", &id]));
    assert!(fetched["document"]["plain_text"].as_str().unwrap().contains("appended"));

    let deleted = run(&root, &["docs", "+delete", &id, "--yes"]);
    assert!(deleted.status.success(), "stderr: {}", String::from_utf8_lossy(&deleted.stderr));
    let trash = data(&run(&root, &["trash", "list"]));
    assert_eq!(trash["items"].as_array().unwrap().len(), 1);

    let restored = run(&root, &["trash", "restore", &id, "--yes"]);
    assert!(restored.status.success(), "stderr: {}", String::from_utf8_lossy(&restored.stderr));
    let health = data(&run(&root, &["library", "doctor"]));
    assert_eq!(health["healthy"], true);
    assert_eq!(data(&run(&root, &["docs", "+list"]))["items"].as_array().unwrap().len(), 1);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn high_risk_write_requires_yes_and_dry_run_does_not_write() {
    let root = temp_library();
    let dry = run(&root, &["docs", "+create", "--title", "dry", "--dry-run"]);
    assert!(dry.status.success());
    assert_eq!(data(&dry)["dry_run"], true);
    assert_eq!(data(&run(&root, &["docs", "+list"]))["items"].as_array().unwrap().len(), 0);

    let bad = run(&root, &["docs", "+delete", "missing"]);
    assert!(!bad.status.success());
    let error: Value = serde_json::from_slice(&bad.stderr).unwrap();
    assert_eq!(error["error"]["type"], "conflict");
    let _ = fs::remove_dir_all(root);
}
