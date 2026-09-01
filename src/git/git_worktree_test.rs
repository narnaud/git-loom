use std::path::{Path, PathBuf};

use super::{WorktreeCheckout, parse_worktree_list};
use crate::core::test_helpers::{TestRepo, mentions_path};
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
    // The integration branch is checked out right here, and the rebase moves
    // its ref, index and files together — that must not be refused.
    git::ensure_not_checked_out_elsewhere(&t.workdir(), &["integration".to_string()]).unwrap();
}

/// Weave setup: `feature` (a1.txt) woven into `integration`, plus a `newbase`
/// branch to rebase onto so the rewrite produces different OIDs.
fn setup_weave_repo() -> (TestRepo, String) {
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
    t.commit("N1", "u1.txt");
    let newbase = t.head_oid().to_string();
    t.switch_branch("integration");
    (t, newbase)
}

fn add_worktree(t: &TestRepo, name: &str, opts: &[&str], committish: &str) -> PathBuf {
    let path = t.workdir().parent().unwrap().join(name);
    let mut argv = vec!["worktree", "add"];
    argv.extend_from_slice(opts);
    argv.push(path.to_str().unwrap());
    argv.push(committish);
    git::run_git(&t.workdir(), &argv).unwrap();
    path
}

fn weave_rebase(t: &TestRepo, newbase: &str) -> anyhow::Result<RebaseOutcome> {
    let graph = Weave::from_repo(&t.repo).unwrap();
    let todo = graph.to_todo();
    weave::run_rebase(&t.workdir(), Some(newbase), &todo)
}

fn wt_status(wt: &Path) -> String {
    git::run_git_stdout(wt, &["status", "--porcelain"]).unwrap()
}

#[test]
fn feature_branch_checked_out_elsewhere_refuses_the_rewrite() {
    let (t, newbase) = setup_weave_repo();
    let wt = add_worktree(&t, "wt", &[], "feature");
    let old_feature = t.get_branch_target("feature");
    let old_head = t.head_oid();

    let err = weave_rebase(&t, &newbase).unwrap_err().to_string();
    assert!(err.contains("feature"), "{err}");
    assert!(mentions_path(&err, &wt), "{err}");

    // Nothing was rewritten, and the worktree is untouched.
    assert_eq!(t.get_branch_target("feature"), old_feature);
    assert_eq!(t.head_oid(), old_head);
    assert!(wt_status(&wt).trim().is_empty());
}

#[test]
fn integration_branch_checked_out_twice_refuses_the_rewrite() {
    let (t, newbase) = setup_weave_repo();
    // `--force` is the only way to check the same branch out twice, and it is
    // exactly what leaves the second worktree behind when the rebase moves the
    // branch: HEAD's own branch never appears in an `update-ref` line.
    let wt = add_worktree(&t, "wt", &["--force"], "integration");
    let old_head = t.head_oid();

    let err = weave_rebase(&t, &newbase).unwrap_err().to_string();
    assert!(err.contains("integration"), "{err}");
    assert!(mentions_path(&err, &wt), "{err}");
    assert_eq!(t.head_oid(), old_head);
}

#[test]
fn detached_worktree_is_unaffected() {
    let (t, newbase) = setup_weave_repo();
    let wt = add_worktree(&t, "wt", &["--detach"], "feature");
    let old_feature = t.get_branch_target("feature");

    // A detached worktree holds no branch, so the rewrite goes ahead.
    let outcome = weave_rebase(&t, &newbase).unwrap();
    assert!(matches!(outcome, RebaseOutcome::Completed));
    assert_ne!(t.get_branch_target("feature"), old_feature);
    assert!(wt.exists());
}

#[test]
fn worktree_with_a_gone_directory_is_ignored() {
    let (t, newbase) = setup_weave_repo();
    let wt = add_worktree(&t, "wt", &[], "feature");
    // Removing the directory without pruning leaves a stale registration; it
    // must not block the rewrite.
    std::fs::remove_dir_all(&wt).unwrap();

    let outcome = weave_rebase(&t, &newbase).unwrap();
    assert!(matches!(outcome, RebaseOutcome::Completed));
}
