use crate::core::graph::Theme;
use crate::core::test_helpers::TestRepo;

// ── Helpers ──────────────────────────────────────────────────────────────

/// Set up a test repo with an integration branch and one woven feature branch.
/// Feature branch has one commit (creating `feature.txt`), so `feature.txt`
/// exists in the working directory and can be modified to produce status diffs
/// with short IDs.
fn setup_with_woven_branch() -> TestRepo {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create feature-a at merge-base with one commit
    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    test_repo.switch_branch("feature-a");
    test_repo.commit("Add feature file", "feature.txt");
    test_repo.switch_branch("integration");

    // Weave feature-a into integration
    test_repo.merge_no_ff("feature-a");

    test_repo
}

/// Returns true if `filename` appears staged (added or modified) in porcelain output.
fn is_staged(porcelain: &str, filename: &str) -> bool {
    porcelain.lines().any(|line| {
        // Porcelain v1: XY filename
        // X = index status, Y = worktree status
        // A = added, M = modified in index column (position 0)
        let x = line.chars().next().unwrap_or(' ');
        (x == 'A' || x == 'M') && line.ends_with(filename)
    })
}

/// Shorthand: call `run()` in non-patch mode with the default dark theme.
fn run_add(files: Vec<String>) -> anyhow::Result<()> {
    let theme = Theme::dark();
    super::run(files, false, &theme)
}

// ── Tests ────────────────────────────────────────────────────────────────

/// R004: Stage a single file by its plain filename.
#[test]
fn add_single_file_by_name() {
    let test_repo = TestRepo::new();

    test_repo.write_file("hello.txt", "hello world");

    let result = test_repo.in_dir(|| run_add(vec!["hello.txt".to_string()]));

    assert!(result.is_ok(), "add failed: {:?}", result);

    let status = test_repo.status_porcelain();
    assert!(
        is_staged(&status, "hello.txt"),
        "hello.txt should be staged; status: {}",
        status
    );
}

/// R001: Stage a single file by its short ID.
///
/// This requires an integration branch with woven features so that
/// `resolve_arg` can map short IDs to file paths via `gather_repo_info`.
#[test]
fn add_single_file_by_shortid() {
    let test_repo = setup_with_woven_branch();

    // Modify the tracked file to produce an unstaged change with a short ID.
    test_repo.write_file("feature.txt", "modified content");

    // Compute the short ID using the same allocator that resolve_arg uses.
    let short_id = test_repo.in_dir(|| {
        let info = crate::core::repo::gather_repo_info(&test_repo.repo, false, 1).unwrap();
        let entities = info.collect_entities();
        let allocator = crate::core::shortid::IdAllocator::new(entities);
        allocator.get_file("feature.txt").to_string()
    });

    let result = test_repo.in_dir(|| run_add(vec![short_id.clone()]));
    assert!(
        result.is_ok(),
        "add by short ID '{}' failed: {:?}",
        short_id,
        result
    );

    let status = test_repo.status_porcelain();
    assert!(
        is_staged(&status, "feature.txt"),
        "feature.txt should be staged; status: {}",
        status
    );
}

/// R002: Stage multiple files in one call.
#[test]
fn add_multiple_files() {
    let test_repo = TestRepo::new();

    test_repo.write_file("a.txt", "aaa");
    test_repo.write_file("b.txt", "bbb");

    let result = test_repo.in_dir(|| run_add(vec!["a.txt".to_string(), "b.txt".to_string()]));

    assert!(result.is_ok(), "add multiple failed: {:?}", result);

    let status = test_repo.status_porcelain();
    assert!(
        is_staged(&status, "a.txt"),
        "a.txt should be staged; status: {}",
        status
    );
    assert!(
        is_staged(&status, "b.txt"),
        "b.txt should be staged; status: {}",
        status
    );
}

/// R003: `zz` stages all changes.
#[test]
fn add_zz_stages_all() {
    let test_repo = TestRepo::new();

    test_repo.write_file("one.txt", "1");
    test_repo.write_file("two.txt", "2");
    test_repo.write_file("three.txt", "3");

    let result = test_repo.in_dir(|| run_add(vec!["zz".to_string()]));

    assert!(result.is_ok(), "add zz failed: {:?}", result);

    let status = test_repo.status_porcelain();
    assert!(
        is_staged(&status, "one.txt"),
        "one.txt should be staged; status: {}",
        status
    );
    assert!(
        is_staged(&status, "two.txt"),
        "two.txt should be staged; status: {}",
        status
    );
    assert!(
        is_staged(&status, "three.txt"),
        "three.txt should be staged; status: {}",
        status
    );
}

/// Negative test: nonexistent file returns an error.
#[test]
fn add_nonexistent_file_errors() {
    let test_repo = TestRepo::new();

    let result = test_repo.in_dir(|| run_add(vec!["nope.txt".to_string()]));

    assert!(result.is_err(), "expected error for nonexistent file");
}

/// Negative test: invalid short ID that matches nothing returns an error.
#[test]
fn add_invalid_shortid_errors() {
    let test_repo = TestRepo::new();

    let result = test_repo.in_dir(|| run_add(vec!["zq".to_string()]));

    assert!(result.is_err(), "expected error for invalid short ID");
}

/// `loom add -p` with patch flag should not crash (placeholder).
#[test]
fn add_patch_flag_placeholder() {
    let test_repo = TestRepo::new();
    let theme = Theme::dark();

    let result = test_repo.in_dir(|| super::run(vec![], true, &theme));

    assert!(result.is_ok(), "add -p should not crash: {:?}", result);
}

/// Untracked files in subdirectories should appear in collect_file_entries.
#[test]
fn collect_entries_includes_untracked_subdirs() {
    let test_repo = TestRepo::new();

    // Create untracked files in a subdirectory (with content).
    let subdir = test_repo.workdir().join("subdir");
    std::fs::create_dir_all(&subdir).unwrap();
    std::fs::write(subdir.join("a.txt"), "aaa\n").unwrap();
    std::fs::write(subdir.join("b.txt"), "bbb\n").unwrap();
    // Also one root-level file.
    test_repo.write_file("root.txt", "root\n");

    let workdir = test_repo.repo.workdir().expect("not bare").to_path_buf();
    let entries = test_repo.in_dir(|| super::collect_file_entries(&test_repo.repo, &workdir, &[]));
    let entries = entries.unwrap();

    let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
    assert!(
        paths.contains(&"root.txt"),
        "expected root.txt, got: {:?}",
        paths
    );
    assert!(
        paths.contains(&"subdir/a.txt"),
        "expected subdir/a.txt, got: {:?}",
        paths
    );
    assert!(
        paths.contains(&"subdir/b.txt"),
        "expected subdir/b.txt, got: {:?}",
        paths
    );

    // Each file should have exactly one hunk.
    for entry in &entries {
        assert!(
            !entry.hunks.is_empty(),
            "file '{}' should have at least one hunk",
            entry.path
        );
    }
}

/// Empty untracked files should still appear in collect_file_entries.
#[test]
fn collect_entries_includes_empty_untracked_files() {
    let test_repo = TestRepo::new();

    // Create empty untracked files.
    test_repo.write_file("empty.txt", "");
    let subdir = test_repo.workdir().join("newdir");
    std::fs::create_dir_all(&subdir).unwrap();
    std::fs::write(subdir.join("also_empty.txt"), "").unwrap();

    let workdir = test_repo.repo.workdir().expect("not bare").to_path_buf();
    let entries = test_repo.in_dir(|| super::collect_file_entries(&test_repo.repo, &workdir, &[]));
    let entries = entries.unwrap();

    let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
    assert!(
        paths.contains(&"empty.txt"),
        "expected empty.txt, got: {:?}",
        paths
    );
    assert!(
        paths.contains(&"newdir/also_empty.txt"),
        "expected newdir/also_empty.txt, got: {:?}",
        paths
    );
}
