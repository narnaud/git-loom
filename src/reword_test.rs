// ── Test Helpers ───────────────────────────────────────────────────────

use crate::core::test_helpers::TestRepo;

// ── Integration tests ──────────────────────────────────────────────────
// These tests require full git command execution and call the actual reword
// functions.

#[test]
fn reword_commit_with_message() {
    let test_repo = TestRepo::new();

    let c1_oid = test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");
    let c3_oid = test_repo.commit("Third commit", "file3.txt");

    let result = super::reword_commit(
        &test_repo.repo,
        &c1_oid.to_string(),
        Some("Updated first commit".to_string()),
    );

    if result.is_err() {
        eprintln!("Note: This test may fail on Windows due to PowerShell sequence editor issues");
        eprintln!("Error: {:?}", result);
    }
    assert!(result.is_ok(), "Failed to reword commit: {:?}", result);

    // The original c1_oid has been rewritten, so we need to find the new commit
    // by walking back from HEAD
    assert_eq!(test_repo.get_subject(2), "Updated first commit");

    // Other commits should have same messages but different hashes (because parent changed)
    assert_eq!(test_repo.get_subject(1), "Second commit");
    assert_eq!(test_repo.get_subject(0), "Third commit");

    assert_ne!(
        test_repo.get_oid(2),
        c1_oid,
        "First commit hash should have changed"
    );
    assert_ne!(
        test_repo.get_oid(0),
        c3_oid,
        "Third commit hash should have changed"
    );

    assert!(test_repo.is_on_branch());
}

#[test]
fn reword_commit_without_message() {
    let test_repo = TestRepo::new();
    let c1_oid = test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");

    // Set up a fake editor that replaces the message
    test_repo.set_fake_editor("Reworded by editor");

    let result = super::reword_commit(&test_repo.repo, &c1_oid.to_string(), None);

    if result.is_err() {
        eprintln!("Note: This test may fail due to platform-specific editor or PowerShell issues");
        eprintln!("Error: {:?}", result);
    }
    assert!(result.is_ok(), "Failed to reword commit: {:?}", result);

    assert_eq!(test_repo.get_subject(1), "Reworded by editor");
}

#[test]
fn reword_root_commit() {
    let test_repo = TestRepo::new();

    let root_commit = test_repo.get_commit(0);
    let root_oid = root_commit.id();
    assert_eq!(root_commit.parent_count(), 0, "Should be a root commit");

    let result = super::reword_commit(
        &test_repo.repo,
        &root_oid.to_string(),
        Some("Updated initial commit".to_string()),
    );

    if result.is_err() {
        eprintln!("Note: This test may fail on Windows due to PowerShell sequence editor issues");
        eprintln!("Error: {:?}", result);
    }
    assert!(result.is_ok(), "Failed to reword root commit: {:?}", result);

    assert_eq!(test_repo.get_subject(0), "Updated initial commit");
    assert_eq!(
        test_repo.get_commit(0).parent_count(),
        0,
        "Should still be a root commit"
    );

    assert_ne!(
        test_repo.get_oid(0),
        root_oid,
        "Root commit hash should have changed"
    );
}

#[test]
fn reword_root_commit_with_descendants() {
    let test_repo = TestRepo::new();

    let root_oid = test_repo.get_oid(0);

    test_repo.commit("Second commit", "file2.txt");
    test_repo.commit("Third commit", "file3.txt");

    let result = super::reword_commit(
        &test_repo.repo,
        &root_oid.to_string(),
        Some("Updated root".to_string()),
    );

    if result.is_err() {
        eprintln!("Note: This test may fail on Windows due to PowerShell sequence editor issues");
        eprintln!("Error: {:?}", result);
    }
    assert!(
        result.is_ok(),
        "Failed to reword root commit with descendants: {:?}",
        result
    );

    assert_eq!(test_repo.get_subject(2), "Updated root");
    assert_eq!(
        test_repo.get_commit(2).parent_count(),
        0,
        "Should still be a root commit"
    );

    assert_eq!(test_repo.get_subject(1), "Second commit");
    assert_eq!(test_repo.get_subject(0), "Third commit");
}

#[test]
fn reword_commit_with_working_tree_changes() {
    let test_repo = TestRepo::new();

    let c1_oid = test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");

    test_repo.write_file("file2.txt", "modified content");

    // Reword should handle working tree changes (via --autostash)
    let result = super::reword_commit(
        &test_repo.repo,
        &c1_oid.to_string(),
        Some("Updated first".to_string()),
    );

    if result.is_err() {
        eprintln!("Note: This test may fail on Windows due to PowerShell sequence editor issues");
        eprintln!("Error: {:?}", result);
    }
    assert!(
        result.is_ok(),
        "Failed to reword with working tree changes: {:?}",
        result
    );

    assert_eq!(
        test_repo.read_file("file2.txt"),
        "modified content",
        "Working tree changes should be preserved"
    );
}

#[test]
fn reword_branch_by_name() {
    let test_repo = TestRepo::new();

    test_repo.create_branch("feature-old");

    let result = super::reword_branch(&test_repo.repo, "feature-old", "feature-new");
    assert!(result.is_ok(), "Failed to rename branch: {:?}", result);

    assert!(
        !test_repo.branch_exists("feature-old"),
        "Old branch should not exist after rename"
    );

    assert!(
        test_repo.branch_exists("feature-new"),
        "New branch should exist after rename"
    );
    assert_eq!(
        test_repo.get_branch_target("feature-new"),
        test_repo.get_oid(0),
        "New branch should point to same commit"
    );
}

#[test]
fn reword_current_branch() {
    let test_repo = TestRepo::new();

    let current_branch_name = test_repo.current_branch_name();

    let result = super::reword_branch(&test_repo.repo, &current_branch_name, "renamed-main");
    assert!(
        result.is_ok(),
        "Failed to rename current branch: {:?}",
        result
    );

    assert!(test_repo.is_on_branch(), "HEAD should still be on a branch");
    assert_eq!(
        test_repo.current_branch_name(),
        "renamed-main",
        "HEAD should track renamed branch"
    );
}

#[test]
fn reword_commit_with_partial_hash() {
    let test_repo = TestRepo::new();

    let c1_oid = test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");

    let partial_hash = &c1_oid.to_string()[..7];
    let result = super::reword_commit(
        &test_repo.repo,
        partial_hash,
        Some("Updated via partial hash".to_string()),
    );

    if result.is_err() {
        eprintln!("Note: This test may fail on Windows due to PowerShell sequence editor issues");
        eprintln!("Error: {:?}", result);
    }
    assert!(
        result.is_ok(),
        "Failed to reword commit with partial hash: {:?}",
        result
    );

    assert_eq!(test_repo.get_subject(1), "Updated via partial hash");
}

#[test]
fn reword_nonexistent_commit_fails() {
    let test_repo = TestRepo::new();

    let result = super::reword_commit(
        &test_repo.repo,
        "0000000000000000000000000000000000000000",
        Some("New message".to_string()),
    );

    assert!(result.is_err(), "Should fail on nonexistent commit");
}

#[test]
fn reword_nonexistent_branch_fails() {
    let test_repo = TestRepo::new();

    let result = super::reword_branch(&test_repo.repo, "nonexistent-branch", "new-name");

    assert!(result.is_err(), "Should fail on nonexistent branch");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Failed to rename branch"),
        "Error should mention branch rename failure"
    );
}

#[test]
fn reword_branch_by_full_name_via_run() {
    let test_repo = TestRepo::new();

    test_repo.create_branch("feature-original");

    let result = test_repo.in_dir(|| {
        super::run(
            "feature-original".to_string(),
            Some("feature-renamed".to_string()),
        )
    });

    assert!(
        result.is_ok(),
        "Failed to rename branch via run: {:?}",
        result
    );

    assert!(
        !test_repo.branch_exists("feature-original"),
        "Old branch should not exist after rename"
    );

    assert!(
        test_repo.branch_exists("feature-renamed"),
        "New branch should exist after rename"
    );
}

// ── Conflict pause / continue / abort ──────────────────────────────────

/// Build a woven integration branch whose second merge had to be resolved by
/// hand — the shape that makes a reword conflict.
///
/// Both feature branches insert a line into the same empty gap, so merging the
/// second one conflicts. Rewording below the merges forces git to rebuild them,
/// and with no rerere entry the merge conflicts again.
///
/// Returns the OID of `feature-a`'s only commit, the reword target.
fn woven_repo_with_hand_resolved_merge(test_repo: &TestRepo) -> git2::Oid {
    // Keep the machine's own rerere cache out of it: a recorded resolution
    // would be replayed and the reword would not conflict at all.
    test_repo.set_config("rerere.enabled", "false");

    test_repo.write_file("shared.txt", "first\nlast\n");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Add shared file");
    let shared_base = test_repo.head_oid().to_string();

    test_repo.create_branch_at("feature-a", &shared_base);
    test_repo.switch_branch("feature-a");
    test_repo.write_file("shared.txt", "first\nfrom-a\nlast\n");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("A1");
    let a1 = test_repo.head_oid();
    test_repo.switch_branch("integration");

    test_repo.create_branch_at("feature-b", &shared_base);
    test_repo.switch_branch("feature-b");
    test_repo.write_file("shared.txt", "first\nfrom-b\nlast\n");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("B1");
    test_repo.switch_branch("integration");

    test_repo.merge_no_ff("feature-a");

    // The second merge conflicts; resolving it by hand is what leaves a tree no
    // replay can reproduce on its own.
    let workdir = test_repo.workdir();
    let outcome = crate::git::merge_no_ff(&workdir, test_repo.repo.path(), "feature-b").unwrap();
    assert!(
        matches!(outcome, crate::git::MergeOutcome::Stopped),
        "the second merge should conflict"
    );
    test_repo.write_file("shared.txt", "first\nfrom-a\nfrom-b\nlast\n");
    test_repo.stage_files(&["shared.txt"]);
    // Through loom's wrapper on purpose: the test repo has `core.editor=false`,
    // so this also checks that `continue_merge` suppresses the editor.
    crate::git::continue_merge(&workdir, test_repo.repo.path()).unwrap();

    a1
}

fn state_path(test_repo: &TestRepo) -> std::path::PathBuf {
    test_repo.repo.path().join("loom").join("state.json")
}

/// Rewording below a hand-resolved merge makes git rebuild that merge and hit
/// the same conflict. The reword must pause with saved state instead of
/// aborting, so `loom continue` can finish it once the conflict is resolved.
#[test]
fn reword_conflict_pauses_and_continues() {
    let test_repo = TestRepo::new_with_remote();
    let a1 = woven_repo_with_hand_resolved_merge(&test_repo);

    let result = super::reword_commit(
        &test_repo.repo,
        &a1.to_string(),
        Some("A1 reworded".to_string()),
    );
    assert!(
        result.is_ok(),
        "reword should pause, not fail: {:?}",
        result
    );
    assert!(
        state_path(&test_repo).exists(),
        "loom state must exist while the reword is paused"
    );
    assert!(
        crate::git::rebase_is_in_progress(test_repo.repo.path()),
        "the rebase should still be paused for the user"
    );

    // Resolve the replayed merge the same way it was resolved originally.
    test_repo.write_file("shared.txt", "first\nfrom-a\nfrom-b\nlast\n");
    test_repo.stage_files(&["shared.txt"]);

    let workdir = test_repo.workdir();
    let git_dir = test_repo.repo.path().to_path_buf();
    crate::core::transaction::continue_cmd(&workdir, &git_dir).unwrap();

    assert!(
        !state_path(&test_repo).exists(),
        "state must be cleared once the reword completes"
    );
    assert!(!crate::git::rebase_is_in_progress(test_repo.repo.path()));
    assert!(
        test_repo
            .commit_messages()
            .contains(&"A1 reworded".to_string()),
        "reworded message should be in history, got: {:?}",
        test_repo.commit_messages()
    );
    assert_eq!(
        test_repo.read_file("shared.txt"),
        "first\nfrom-a\nfrom-b\nlast\n",
        "the resolution should survive"
    );
    assert_eq!(
        test_repo.head_commit().parent_count(),
        2,
        "integration should still be a merge commit"
    );
}

/// `loom abort` after a paused reword must undo the amend along with the
/// rebase, leaving the original message and refs untouched.
#[test]
fn reword_conflict_abort_restores_original_state() {
    let test_repo = TestRepo::new_with_remote();
    let a1 = woven_repo_with_hand_resolved_merge(&test_repo);
    let original_head = test_repo.head_oid();
    let original_feature_a = test_repo.get_branch_target("feature-a");

    let result = super::reword_commit(
        &test_repo.repo,
        &a1.to_string(),
        Some("A1 reworded".to_string()),
    );
    assert!(
        result.is_ok(),
        "reword should pause, not fail: {:?}",
        result
    );

    let workdir = test_repo.workdir();
    let git_dir = test_repo.repo.path().to_path_buf();
    crate::core::transaction::abort_cmd(&workdir, &git_dir).unwrap();

    assert!(!state_path(&test_repo).exists(), "state must be cleared");
    assert!(!crate::git::rebase_is_in_progress(test_repo.repo.path()));
    assert_eq!(
        test_repo.head_oid(),
        original_head,
        "abort must restore the original integration tip"
    );
    assert_eq!(
        test_repo.get_branch_target("feature-a"),
        original_feature_a,
        "abort must restore feature-a"
    );
    assert!(
        !test_repo
            .commit_messages()
            .contains(&"A1 reworded".to_string()),
        "the amend must be undone, got: {:?}",
        test_repo.commit_messages()
    );
}

/// A reword that completes normally must not leave state behind for the paused
/// -operation guard to trip over.
#[test]
fn reword_without_conflict_leaves_no_state() {
    let test_repo = TestRepo::new();
    let c1 = test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");

    super::reword_commit(
        &test_repo.repo,
        &c1.to_string(),
        Some("Reworded first".to_string()),
    )
    .unwrap();

    assert!(
        !state_path(&test_repo).exists(),
        "a clean reword must clear its state file"
    );
}

#[test]
fn reword_refuses_when_the_replay_is_dropped() {
    // git drops a commit whose changes the base already has, yet still honors
    // its `edit` line and stops on the commit below — rewording there would
    // rewrite that one and lose the target.
    let (t, target) = crate::core::test_helpers::repo_with_dropped_replay();
    let head_before = t.head_oid();
    let alpha_before = t.get_branch_target("alpha");

    let err = super::reword_commit(
        &t.repo,
        &target.to_string(),
        Some("New message".to_string()),
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("is redundant"), "{err}");
    assert_eq!(t.head_oid(), head_before, "{err}");
    assert_eq!(t.get_branch_target("alpha"), alpha_before, "{err}");
    assert!(!crate::git::rebase_is_in_progress(t.repo.path()), "{err}");
}

#[test]
fn reword_refuses_when_the_upstream_cherry_picked_the_target() {
    // A cherry-pick upstream keeps the author and message, so the commit the
    // rebase stops on looks exactly like the target: only git reporting the
    // empty replay gives the drop away.
    let (t, target) = crate::core::test_helpers::repo_with_cherry_picked_replay();
    let head_before = t.head_oid();
    let alpha_before = t.get_branch_target("alpha");

    let err = super::reword_commit(
        &t.repo,
        &target.to_string(),
        Some("New message".to_string()),
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("is redundant"), "{err}");
    assert_eq!(t.head_oid(), head_before, "{err}");
    assert_eq!(t.get_branch_target("alpha"), alpha_before, "{err}");
    assert!(!crate::git::rebase_is_in_progress(t.repo.path()), "{err}");
}

#[test]
fn reword_gets_past_a_redundant_commit_below_the_target() {
    // Only the commit being rewritten has to survive the replay: one below it
    // whose changes are already upstream is dropped, as it always was, and the
    // reword goes through.
    let (t, _redundant, keeper) = crate::core::test_helpers::repo_with_a_redundant_commit_below();

    super::reword_commit(&t.repo, &keeper.to_string(), Some("Reworded".to_string())).unwrap();

    assert_eq!(t.branch_commit_summary("alpha"), "Reworded");
    assert!(!t.commit_messages().contains(&"branch change".to_string()));
    assert!(!crate::git::rebase_is_in_progress(t.repo.path()));
}

#[test]
fn reword_gets_past_a_redundant_commit_above_the_target() {
    // The rebase meets the redundant commit after the amend, on the `continue`
    // rather than the first run — a stop there is not a conflict either.
    let (t, keeper) = crate::core::test_helpers::repo_with_a_redundant_commit_above();

    super::reword_commit(&t.repo, &keeper.to_string(), Some("Reworded".to_string())).unwrap();

    assert!(t.commit_messages().contains(&"Reworded".to_string()));
    assert!(!t.commit_messages().contains(&"branch change".to_string()));
    assert!(!crate::git::rebase_is_in_progress(t.repo.path()));
    assert!(!t.repo.path().join("loom").join("state.json").exists());
}

#[test]
fn reword_works_with_a_short_core_abbrev() {
    // `core.abbrev` decides how long the hashes in a rebase todo are; matching
    // them must not assume a minimum length.
    let t = TestRepo::new_with_remote();
    t.set_config("core.abbrev", "4");
    let c1 = t.commit("First commit", "file1.txt");
    t.commit("Second commit", "file2.txt");

    super::reword_commit(&t.repo, &c1.to_string(), Some("Reworded".to_string())).unwrap();

    assert_eq!(t.get_subject(1), "Reworded");
}

// ── Change-Id ────────────────────────────────────────────────────────────

const CHANGE_ID: &str = "I0123456789abcdef0123456789abcdef01234567";

#[test]
fn reword_with_message_keeps_the_change_id() {
    let test_repo = TestRepo::new();
    let c1_oid = test_repo.commit(&format!("First\n\nChange-Id: {CHANGE_ID}\n"), "file1.txt");
    test_repo.commit("Second", "file2.txt");

    super::reword_commit(
        &test_repo.repo,
        &c1_oid.to_string(),
        Some("Updated first".to_string()),
    )
    .unwrap();

    assert_eq!(
        test_repo.get_message(1),
        format!("Updated first\n\nChange-Id: {CHANGE_ID}")
    );
}

#[test]
fn reword_with_editor_restores_a_dropped_change_id() {
    let test_repo = TestRepo::new();
    let c1_oid = test_repo.commit(&format!("First\n\nChange-Id: {CHANGE_ID}\n"), "file1.txt");
    test_repo.commit("Second", "file2.txt");
    test_repo.set_fake_editor("Reworded by editor");

    super::reword_commit(&test_repo.repo, &c1_oid.to_string(), None).unwrap();

    assert_eq!(
        test_repo.get_message(1),
        format!("Reworded by editor\n\nChange-Id: {CHANGE_ID}")
    );
    assert_eq!(test_repo.get_message(0), "Second");
}

/// A Change-Id written in the editor is deliberate: it wins over the old one.
#[test]
fn reword_with_editor_keeps_a_change_id_the_editor_wrote() {
    let test_repo = TestRepo::new();
    let c1_oid = test_repo.commit(&format!("First\n\nChange-Id: {CHANGE_ID}\n"), "file1.txt");
    let other = "I9999999999999999999999999999999999999999";
    test_repo.set_fake_editor(&format!("Reworded by editor\n\nChange-Id: {other}"));

    super::reword_commit(&test_repo.repo, &c1_oid.to_string(), None).unwrap();

    assert_eq!(
        test_repo.get_message(0),
        format!("Reworded by editor\n\nChange-Id: {other}")
    );
}

#[test]
fn reword_mints_a_change_id_for_a_commit_without_one() {
    let test_repo = TestRepo::new();
    let c1_oid = test_repo.commit("First", "file1.txt");

    super::reword_commit(
        &test_repo.repo,
        &c1_oid.to_string(),
        Some("Updated first".to_string()),
    )
    .unwrap();

    let msg = test_repo.get_message(0);
    assert!(msg.starts_with("Updated first\n\nChange-Id: I"), "{msg}");

    test_repo.set_config("loom.changeId", "false");
    let c2_oid = test_repo.commit("Plain", "file2.txt");
    super::reword_commit(
        &test_repo.repo,
        &c2_oid.to_string(),
        Some("Still plain".to_string()),
    )
    .unwrap();
    assert_eq!(test_repo.get_message(0), "Still plain");
}

/// A state file written when the field was `new_display` still resumes.
#[test]
fn after_continue_reads_a_state_file_with_the_old_field_name() {
    let test_repo = TestRepo::new();
    let context = serde_json::json!({ "display": "abc1234", "new_display": "def5678" });

    let ctx: super::RewordContext = serde_json::from_value(context.clone()).unwrap();
    assert_eq!(ctx.new_hash, "def5678");
    super::after_continue(&test_repo.workdir(), &context).unwrap();
}
