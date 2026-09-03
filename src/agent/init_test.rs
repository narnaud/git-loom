use super::*;

fn skill_path(base: &Path) -> PathBuf {
    base.join("skills").join("git-loom").join("SKILL.md")
}

#[test]
fn installs_skill_into_dir() {
    let dir = tempfile::tempdir().unwrap();
    run(AgentKind::Claude, false, Some(dir.path().to_path_buf())).unwrap();

    let target = skill_path(dir.path());
    assert!(target.exists(), "skill file not created");
    let content = std::fs::read_to_string(&target).unwrap();
    assert_eq!(content, CLAUDE_SKILL);
}

#[test]
fn install_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    run(AgentKind::Claude, false, Some(dir.path().to_path_buf())).unwrap();
    // Second run: identical content, must succeed without touching the file.
    run(AgentKind::Claude, false, Some(dir.path().to_path_buf())).unwrap();
    let content = std::fs::read_to_string(skill_path(dir.path())).unwrap();
    assert_eq!(content, CLAUDE_SKILL);
}

#[test]
fn install_overwrites_stale_content() {
    let dir = tempfile::tempdir().unwrap();
    let target = skill_path(dir.path());
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "old skill from a previous loom version").unwrap();

    run(AgentKind::Claude, false, Some(dir.path().to_path_buf())).unwrap();

    let content = std::fs::read_to_string(&target).unwrap();
    assert_eq!(content, CLAUDE_SKILL);
}

#[test]
fn creates_nested_directories() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("deep").join("nested").join("base");
    run(AgentKind::Claude, false, Some(base.clone())).unwrap();
    assert!(skill_path(&base).exists());
}

#[test]
fn skill_has_frontmatter_and_no_patch_advice() {
    // Sanity-check the embedded asset itself: valid frontmatter and the
    // critical invocation rules.
    assert!(CLAUDE_SKILL.starts_with("---\nname: git-loom\n"));
    assert!(CLAUDE_SKILL.contains("description:"));
    assert!(CLAUDE_SKILL.contains("--agent"));
    assert!(CLAUDE_SKILL.contains("--patch"));
    assert!(CLAUDE_SKILL.contains("loom continue"));
}

#[test]
fn not_outdated_when_nothing_is_installed() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!is_outdated(AgentKind::Claude, &skill_path(dir.path())));
}

#[test]
fn not_outdated_right_after_install() {
    let dir = tempfile::tempdir().unwrap();
    run(AgentKind::Claude, false, Some(dir.path().to_path_buf())).unwrap();
    assert!(!is_outdated(AgentKind::Claude, &skill_path(dir.path())));
}

#[test]
fn outdated_when_content_differs() {
    let dir = tempfile::tempdir().unwrap();
    let target = skill_path(dir.path());
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();

    // A skill written by an older loom.
    std::fs::write(&target, "old skill from a previous loom version").unwrap();
    assert!(is_outdated(AgentKind::Claude, &target));

    // A local edit reads the same way: the file is loom-owned, so any
    // divergence is reported and `agent init` overwrites it.
    std::fs::write(&target, format!("{}\nlocal note\n", CLAUDE_SKILL)).unwrap();
    assert!(is_outdated(AgentKind::Claude, &target));
}
