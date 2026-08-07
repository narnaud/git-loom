use std::path::{Path, PathBuf};

use super::{PendingSync, WorktreeCheckout, parse_worktree_list};
use crate::core::test_helpers::TestRepo;
use crate::core::weave;
use crate::core::weave::Weave;
use crate::git::{self, RebaseOutcome};

#[test]
fn parse_worktree_list_filters_special_worktrees() {
    let porcelain = "\
worktree /repo
HEAD 1111111111111111111111111111111111111111
branch refs/heads/main

worktree /repo-wt
HEAD 2222222222222222222222222222222222222222
branch refs/heads/feature

worktree /repo-detached
HEAD 3333333333333333333333333333333333333333
detached

worktree /repo-bare
bare

worktree /repo-gone
HEAD 4444444444444444444444444444444444444444
branch refs/heads/gone
prunable gitdir file points to non-existent location
";
    let list = parse_worktree_list(porcelain);
    assert_eq!(
        list,
        vec![
            WorktreeCheckout {
                path: PathBuf::from("/repo"),
                branch: "main".to_string(),
            },
            WorktreeCheckout {
                path: PathBuf::from("/repo-wt"),
                branch: "feature".to_string(),
            },
        ]
    );
}

#[test]
fn current_worktree_is_exempt() {
    let t = TestRepo::new_with_remote();
    t.commit("C1", "c1.txt");
    // Dirty the current worktree — its branch moves together with it, so
    // planning a rewrite of that branch must neither bail nor plan a sync.
    t.write_file("c1.txt", "dirty");
    let syncs = git::plan_worktree_syncs(&t.workdir(), &["integration".to_string()]).unwrap();
    assert!(syncs.is_empty());
}

/// Weave setup: `feature` (a1.txt) woven into `integration`, checked out in
/// an external worktree, plus a `newbase` branch to rebase onto so the
/// rewrite produces different OIDs.
///
/// `newbase_file` is the file committed on `newbase`: use a fresh name for a
/// clean rebase, or "a1.txt" to force a conflict with the feature commit.
fn setup_worktree_repo(newbase_file: &str) -> (TestRepo, PathBuf, String) {
    let t = TestRepo::new_with_remote();
    let base = t.find_remote_branch_target("origin/main").to_string();
    t.create_branch_at("feature", &base);
    t.switch_branch("feature");
    t.commit("A1", "a1.txt");
    t.switch_branch("integration");
    t.commit("Int", "int.txt");
    t.merge_no_ff("feature");

    t.create_branch_at("newbase", &base);
    t.switch_branch("newbase");
    t.commit("N1", newbase_file);
    let newbase = t.head_oid().to_string();
    t.switch_branch("integration");

    let wt = t.workdir().parent().unwrap().join("wt");
    git::run_git(
        &t.workdir(),
        &["worktree", "add", wt.to_str().unwrap(), "feature"],
    )
    .unwrap();
    (t, wt, newbase)
}

fn weave_rebase(t: &TestRepo, newbase: &str) -> anyhow::Result<RebaseOutcome> {
    let graph = Weave::from_repo(&t.repo).unwrap();
    let todo = graph.to_todo();
    weave::run_rebase(&t.workdir(), Some(newbase), &todo)
}

fn wt_status(wt: &Path) -> String {
    git::run_git_stdout(wt, &["status", "--porcelain"]).unwrap()
}

fn pending_sync_file(t: &TestRepo) -> PathBuf {
    t.repo.path().join("loom").join("worktree-sync.json")
}

#[test]
fn clean_worktree_is_synced_after_rewrite() {
    let (t, wt, newbase) = setup_worktree_repo("u1.txt");
    let old_feature = t.get_branch_target("feature");

    let outcome = weave_rebase(&t, &newbase).unwrap();
    assert!(matches!(outcome, RebaseOutcome::Completed));

    let new_feature = t.get_branch_target("feature");
    assert_ne!(new_feature, old_feature, "branch should be rewritten");

    // The worktree's HEAD, index, and files all follow the new tip.
    assert_eq!(
        git::rev_parse(&wt, "HEAD").unwrap(),
        new_feature.to_string()
    );
    let status = wt_status(&wt);
    assert!(
        status.trim().is_empty(),
        "worktree should be clean: {status}"
    );
    assert!(
        wt.join("u1.txt").exists(),
        "new base file should be checked out"
    );
    assert!(!pending_sync_file(&t).exists());
}

#[test]
fn untracked_file_does_not_block_and_survives_sync() {
    let (t, wt, newbase) = setup_worktree_repo("u1.txt");
    std::fs::write(wt.join("notes.txt"), "scratch").unwrap();

    let outcome = weave_rebase(&t, &newbase).unwrap();
    assert!(matches!(outcome, RebaseOutcome::Completed));

    assert_eq!(
        git::rev_parse(&wt, "HEAD").unwrap(),
        t.get_branch_target("feature").to_string()
    );
    assert_eq!(
        std::fs::read_to_string(wt.join("notes.txt")).unwrap(),
        "scratch"
    );
}

#[test]
fn untracked_collision_blocks_sync_but_preserves_file() {
    let (t, wt, newbase) = setup_worktree_repo("u1.txt");
    let old_feature = t.get_branch_target("feature");
    // The rewritten feature will newly track u1.txt (from newbase); an
    // untracked u1.txt in the worktree must never be overwritten by the sync.
    std::fs::write(wt.join("u1.txt"), "draft").unwrap();

    let outcome = weave_rebase(&t, &newbase).unwrap();
    assert!(matches!(outcome, RebaseOutcome::Completed));

    // The branch was rewritten, but the sync refused and the draft survives.
    assert_ne!(t.get_branch_target("feature"), old_feature);
    assert_eq!(std::fs::read_to_string(wt.join("u1.txt")).unwrap(), "draft");
    assert!(
        !wt_status(&wt).trim().is_empty(),
        "worktree stays desynced when the sync is refused"
    );
    assert!(!pending_sync_file(&t).exists());
}

#[test]
fn edit_pause_saves_plan_and_syncs_on_continue() {
    let (t, wt, newbase) = setup_worktree_repo("u1.txt");
    let old_feature = t.get_branch_target("feature");

    // An `edit` at the feature tip: git rebase exits successfully but stays
    // in progress, so the sync must wait for the continue.
    let mut graph = Weave::from_repo(&t.repo).unwrap();
    graph.edit_commit(old_feature);
    let todo = graph.to_todo();
    let outcome = weave::run_rebase(&t.workdir(), Some(&newbase), &todo).unwrap();
    assert!(matches!(outcome, RebaseOutcome::Completed));
    assert!(git::rebase_is_in_progress(t.repo.path()));
    assert!(
        pending_sync_file(&t).exists(),
        "sync plan should survive the edit pause"
    );

    let outcome = git::continue_rebase(&t.workdir()).unwrap();
    assert!(matches!(outcome, RebaseOutcome::Completed));

    let new_feature = t.get_branch_target("feature");
    assert_ne!(new_feature, old_feature);
    assert_eq!(
        git::rev_parse(&wt, "HEAD").unwrap(),
        new_feature.to_string()
    );
    assert!(wt_status(&wt).trim().is_empty());
    assert!(!pending_sync_file(&t).exists());
}

#[test]
fn finish_skips_deleted_branch_and_removes_file() {
    let t = TestRepo::new_with_remote();
    let workdir = t.workdir();
    git::save_worktree_syncs(
        &workdir,
        &[PendingSync {
            branch: "nonexistent".to_string(),
            path: workdir.clone(),
            old_tip: t.head_oid().to_string(),
        }],
    )
    .unwrap();
    assert!(pending_sync_file(&t).exists());

    git::finish_worktree_syncs(&workdir);
    assert!(!pending_sync_file(&t).exists());
}

#[test]
fn dirty_worktree_refuses_rewrite_up_front() {
    let (t, wt, newbase) = setup_worktree_repo("u1.txt");
    std::fs::write(wt.join("a1.txt"), "local edit").unwrap();
    let old_feature = t.get_branch_target("feature");
    let old_head = t.head_oid();

    let err = weave_rebase(&t, &newbase).unwrap_err().to_string();
    assert!(
        err.contains("feature"),
        "error should name the branch: {err}"
    );
    // Git prints Windows paths with forward slashes; compare normalized.
    let err_slash = err.replace('\\', "/");
    let wt_slash = wt.to_str().unwrap().replace('\\', "/");
    assert!(
        err_slash.contains(&wt_slash),
        "error should name the worktree path: {err}"
    );
    assert!(err.contains("local changes"), "unexpected error: {err}");

    // Nothing was rewritten.
    assert_eq!(t.get_branch_target("feature"), old_feature);
    assert_eq!(t.head_oid(), old_head);
    assert_eq!(
        std::fs::read_to_string(wt.join("a1.txt")).unwrap(),
        "local edit"
    );
    assert!(!pending_sync_file(&t).exists());
}

#[test]
fn staged_change_in_worktree_also_refuses() {
    let (t, wt, newbase) = setup_worktree_repo("u1.txt");
    std::fs::write(wt.join("staged.txt"), "staged").unwrap();
    git::run_git(&wt, &["add", "staged.txt"]).unwrap();
    let old_feature = t.get_branch_target("feature");

    let err = weave_rebase(&t, &newbase).unwrap_err().to_string();
    assert!(err.contains("local changes"), "unexpected error: {err}");
    assert_eq!(t.get_branch_target("feature"), old_feature);
}

#[test]
fn detached_worktree_is_unaffected() {
    let (t, wt, newbase) = setup_worktree_repo("u1.txt");
    // Turn the worktree into a detached checkout of the same commit.
    git::run_git(&t.workdir(), &["worktree", "remove", wt.to_str().unwrap()]).unwrap();
    git::run_git(
        &t.workdir(),
        &[
            "worktree",
            "add",
            "--detach",
            wt.to_str().unwrap(),
            "feature",
        ],
    )
    .unwrap();
    let old_feature = t.get_branch_target("feature");

    let outcome = weave_rebase(&t, &newbase).unwrap();
    assert!(matches!(outcome, RebaseOutcome::Completed));

    // The branch was rewritten but the detached worktree stays where it was.
    assert_ne!(t.get_branch_target("feature"), old_feature);
    assert_eq!(
        git::rev_parse(&wt, "HEAD").unwrap(),
        old_feature.to_string()
    );
    assert!(wt_status(&wt).trim().is_empty());
}

#[test]
fn conflicted_rebase_syncs_worktree_after_continue() {
    let (t, wt, newbase) = setup_worktree_repo("a1.txt");
    let old_feature = t.get_branch_target("feature");

    let outcome = weave_rebase(&t, &newbase).unwrap();
    assert!(matches!(outcome, RebaseOutcome::Conflicted));
    assert!(
        pending_sync_file(&t).exists(),
        "sync plan should be saved while the rebase is paused"
    );
    // The worktree is untouched while the rebase is paused.
    assert_eq!(
        git::rev_parse(&wt, "HEAD").unwrap(),
        old_feature.to_string()
    );

    // Resolve the conflict and continue.
    t.write_file("a1.txt", "resolved");
    git::run_git(&t.workdir(), &["add", "a1.txt"]).unwrap();
    let outcome = git::continue_rebase(&t.workdir()).unwrap();
    assert!(matches!(outcome, RebaseOutcome::Completed));

    let new_feature = t.get_branch_target("feature");
    assert_ne!(new_feature, old_feature);
    assert_eq!(
        git::rev_parse(&wt, "HEAD").unwrap(),
        new_feature.to_string()
    );
    assert!(wt_status(&wt).trim().is_empty());
    assert_eq!(
        std::fs::read_to_string(wt.join("a1.txt")).unwrap(),
        "resolved"
    );
    assert!(!pending_sync_file(&t).exists());
}

#[test]
fn aborted_rebase_leaves_worktree_alone_and_cleans_up() {
    let (t, wt, newbase) = setup_worktree_repo("a1.txt");
    let old_feature = t.get_branch_target("feature");
    let old_head = t.head_oid();

    let outcome = weave_rebase(&t, &newbase).unwrap();
    assert!(matches!(outcome, RebaseOutcome::Conflicted));

    git::rebase_abort(&t.workdir()).unwrap();

    assert_eq!(t.get_branch_target("feature"), old_feature);
    assert_eq!(t.head_oid(), old_head);
    assert_eq!(
        git::rev_parse(&wt, "HEAD").unwrap(),
        old_feature.to_string()
    );
    assert!(wt_status(&wt).trim().is_empty());
    assert!(!pending_sync_file(&t).exists());
}

#[test]
fn worktree_modified_during_pause_is_not_synced() {
    let (t, wt, newbase) = setup_worktree_repo("a1.txt");
    let old_feature = t.get_branch_target("feature");

    let outcome = weave_rebase(&t, &newbase).unwrap();
    assert!(matches!(outcome, RebaseOutcome::Conflicted));

    // The user edits the worktree while the rebase is paused.
    std::fs::write(wt.join("a1.txt"), "user edit during pause").unwrap();

    t.write_file("a1.txt", "resolved");
    git::run_git(&t.workdir(), &["add", "a1.txt"]).unwrap();
    let outcome = git::continue_rebase(&t.workdir()).unwrap();
    assert!(matches!(outcome, RebaseOutcome::Completed));

    // The branch was rewritten (the worktree's HEAD symref follows it), but
    // the edited files and index were left alone — no reset --hard happened.
    let new_feature = t.get_branch_target("feature");
    assert_ne!(new_feature, old_feature);
    assert_eq!(
        git::rev_parse(&wt, "HEAD").unwrap(),
        new_feature.to_string()
    );
    assert_eq!(
        std::fs::read_to_string(wt.join("a1.txt")).unwrap(),
        "user edit during pause"
    );
    assert!(!pending_sync_file(&t).exists());
}
