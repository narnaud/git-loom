use std::fs;

use tempfile::TempDir;

/// Helper: create a fake .git dir in a temp directory.
fn setup_git_dir() -> (TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let git_dir = dir.path().join(".git");
    fs::create_dir_all(&git_dir).unwrap();
    (dir, git_dir)
}

#[test]
fn init_and_finalize_creates_log_file() {
    let (_dir, git_dir) = setup_git_dir();

    super::init(&git_dir, "git loom fold aa bb");
    super::log_command("git", "rebase --interactive", 230, true, "");
    let path = super::finalize();

    assert!(path.is_some());
    let path = path.unwrap();
    assert!(path.exists());

    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("git loom fold aa bb"));
    assert!(content.contains("[git] rebase --interactive"));
    assert!(content.contains("[230ms]"));
}

#[test]
fn finalize_noop_when_not_initialized() {
    // Ensure no panic and returns None when logger was never initialized
    let result = super::finalize();
    assert!(result.is_none());
}

#[test]
fn finalize_noop_when_no_entries() {
    let (_dir, git_dir) = setup_git_dir();

    super::init(&git_dir, "git loom status");
    // No log_command calls
    let result = super::finalize();
    assert!(result.is_none());
}

#[test]
fn log_command_noop_when_not_initialized() {
    // Should not panic
    super::log_command("git", "status", 5, true, "");
}

#[test]
fn failed_command_includes_stderr() {
    let (_dir, git_dir) = setup_git_dir();

    super::init(&git_dir, "git loom fold aa bb");
    super::log_command(
        "git",
        "commit --amend",
        12,
        false,
        "error: could not apply abc1234",
    );
    let path = super::finalize().unwrap();

    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("FAILED"));
    assert!(content.contains("[stderr]"));
    assert!(content.contains("error: could not apply abc1234"));
}

#[test]
fn successful_command_excludes_stderr_section() {
    let (_dir, git_dir) = setup_git_dir();

    super::init(&git_dir, "git loom commit");
    super::log_command("git", "add -A", 5, true, "");
    let path = super::finalize().unwrap();

    let content = fs::read_to_string(&path).unwrap();
    assert!(!content.contains("FAILED"));
    assert!(!content.contains("[stderr]"));
}

#[test]
fn annotations_appear_in_log() {
    let (_dir, git_dir) = setup_git_dir();

    super::init(&git_dir, "git loom fold aa bb");
    super::log_command("git", "rebase --interactive", 500, true, "");
    super::annotate(
        "generated todo",
        "label onto\nreset onto\npick abc1234 First commit",
    );
    let path = super::finalize().unwrap();

    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("[generated todo]"));
    assert!(content.contains("pick abc1234 First commit"));
}

#[test]
fn pruning_keeps_only_max_count() {
    let (_dir, git_dir) = setup_git_dir();
    let logs_dir = git_dir.join("loom").join("logs");
    fs::create_dir_all(&logs_dir).unwrap();

    // Create 12 fake log files
    for i in 0..12 {
        let name = format!("2026-03-04_14-30-{:02}_000.log", i);
        fs::write(logs_dir.join(&name), format!("log {}", i)).unwrap();
    }

    super::prune_logs(&logs_dir, 10);

    let remaining: Vec<_> = fs::read_dir(&logs_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(remaining.len(), 10);

    // The oldest two (00, 01) should have been removed
    assert!(!logs_dir.join("2026-03-04_14-30-00_000.log").exists());
    assert!(!logs_dir.join("2026-03-04_14-30-01_000.log").exists());
    assert!(logs_dir.join("2026-03-04_14-30-02_000.log").exists());
}

#[test]
fn latest_log_path_returns_newest() {
    let (_dir, git_dir) = setup_git_dir();
    let logs_dir = git_dir.join("loom").join("logs");
    fs::create_dir_all(&logs_dir).unwrap();

    fs::write(logs_dir.join("2026-03-04_14-30-00_000.log"), "old").unwrap();
    fs::write(logs_dir.join("2026-03-04_14-30-05_000.log"), "newest").unwrap();
    fs::write(logs_dir.join("2026-03-04_14-30-02_000.log"), "middle").unwrap();

    let path = super::latest_log_path(&git_dir).unwrap();
    assert!(path.ends_with("2026-03-04_14-30-05_000.log"));
}

#[test]
fn latest_log_path_returns_none_when_no_logs() {
    let (_dir, git_dir) = setup_git_dir();
    let result = super::latest_log_path(&git_dir);
    assert!(result.is_none());
}

#[test]
fn multiple_entries_in_order() {
    let (_dir, git_dir) = setup_git_dir();

    super::init(&git_dir, "git loom fold aa bb");
    super::log_command("git", "rebase --interactive", 500, true, "");
    super::log_command("git", "reset --hard HEAD", 5, true, "");
    super::log_command("git", "commit --amend --no-edit", 12, false, "error msg");
    let path = super::finalize().unwrap();

    let content = fs::read_to_string(&path).unwrap();
    let rebase_pos = content.find("rebase --interactive").unwrap();
    let reset_pos = content.find("reset --hard HEAD").unwrap();
    let commit_pos = content.find("commit --amend --no-edit").unwrap();

    // Entries should appear in execution order
    assert!(rebase_pos < reset_pos);
    assert!(reset_pos < commit_pos);
}
