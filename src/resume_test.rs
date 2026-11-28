use std::path::Path;

use super::*;
use crate::core::transaction::{Rollback, state_path};

/// `--empty` outlives `git rebase --continue`, so the state file is what
/// carries the protection across the pause.
#[test]
fn continue_refuses_when_a_protected_commit_replays_empty() {
    // Paused at an `edit` rather than a conflict: either way the rebase
    // that continues is the one `run_rebase_protecting` started, and the
    // redundant commit above the stop replays on the continue.
    let (t, keeper) = crate::core::test_helpers::repo_with_a_redundant_commit_above();
    let workdir = t.workdir();
    let git_dir = t.repo.path().to_path_buf();
    let redundant = t.get_branch_target("alpha").to_string();

    let mut graph = crate::core::weave::Weave::from_repo(&t.repo).unwrap();
    assert!(graph.edit_commit(keeper));
    let protect = vec![redundant.clone()];
    let outcome = crate::core::weave::run_rebase_protecting(
        &workdir,
        Some(&graph.base_oid.to_string()),
        &graph.to_todo(),
        crate::git::Protected::named(&protect),
    )
    .unwrap();
    assert_eq!(outcome, git::RebaseOutcome::Paused);

    t.create_branch_at("_loom-track", &redundant);
    save(
        &git_dir,
        &LoomState {
            command: Resumable::Fold,
            rollback: Rollback {
                delete_branches: vec!["_loom-track".to_string()],
                ..Default::default()
            },
            context: serde_json::Value::Null,
            protect,
            targets: Vec::new(),
            skipped_fixups: Vec::new(),
        },
    )
    .unwrap();

    let err = continue_cmd(&workdir, &git_dir).unwrap_err().to_string();

    assert!(err.contains("is redundant"), "{err}");
    assert!(
        err.contains("loom drop"),
        "this rollback keeps the commit, so the hint stands: {err}"
    );
    assert!(!git::rebase_is_in_progress(&git_dir), "{err}");
    assert!(
        !state_path(&git_dir).exists(),
        "the state must go with the rollback, or `loom drop` is refused"
    );
    assert!(
        !t.branch_exists("_loom-track"),
        "the rollback runs too, not just the state removal"
    );
}

#[test]
fn continue_does_not_offer_to_drop_a_target() {
    let (t, keeper) = crate::core::test_helpers::repo_with_a_redundant_commit_above();
    let workdir = t.workdir();
    let git_dir = t.repo.path().to_path_buf();
    let redundant = t.get_branch_target("alpha").to_string();

    let mut graph = crate::core::weave::Weave::from_repo(&t.repo).unwrap();
    assert!(graph.edit_commit(keeper));
    let targets = vec![redundant];
    crate::core::weave::run_rebase_protecting(
        &workdir,
        Some(&graph.base_oid.to_string()),
        &graph.to_todo(),
        crate::git::Protected::default().targeting(&targets),
    )
    .unwrap();
    save(
        &git_dir,
        &LoomState {
            command: Resumable::Fold,
            rollback: Rollback::default(),
            context: serde_json::Value::Null,
            protect: Vec::new(),
            targets,
            skipped_fixups: Vec::new(),
        },
    )
    .unwrap();

    let err = continue_cmd(&workdir, &git_dir).unwrap_err().to_string();

    assert!(err.contains("is redundant"), "{err}");
    assert!(!err.contains("loom drop"), "{err}");
    assert!(!git::rebase_is_in_progress(&git_dir), "{err}");
}

/// The refusal leaves a live rebase when the tree is dirty, because the
/// undo is a hard reset: `continue` must leave the work, the rebase and the
/// state alone. (That it also stops warning "run `loom abort`" over a
/// message saying to save the work first is not visible from here.)
#[test]
fn continue_does_not_undo_over_a_dirty_tree() {
    let (t, keeper) = crate::core::test_helpers::repo_with_a_redundant_commit_above();
    let workdir = t.workdir();
    let git_dir = t.repo.path().to_path_buf();
    let mut graph = crate::core::weave::Weave::from_repo(&t.repo).unwrap();
    assert!(graph.edit_commit(keeper));
    // Every commit of the branch, so whichever replays empty first is one
    // the refusal has to catch.
    let base = graph.base_oid.to_string();
    let protect: Vec<String> =
        git::run_git_stdout(&workdir, &["rev-list", &format!("{base}..alpha")])
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect();
    crate::core::weave::run_rebase_protecting(
        &workdir,
        Some(&base),
        &graph.to_todo(),
        crate::git::Protected::named(&protect),
    )
    .unwrap();

    t.write_file("three.txt", "edited while paused\n");
    save(
        &git_dir,
        &LoomState {
            command: Resumable::Commit,
            rollback: Rollback {
                reset_mixed_to: base.clone(),
                ..Default::default()
            },
            context: serde_json::Value::Null,
            protect,
            targets: Vec::new(),
            skipped_fixups: Vec::new(),
        },
    )
    .unwrap();

    let err = continue_cmd(&workdir, &git_dir).unwrap_err().to_string();

    assert!(err.contains("is redundant"), "{err}");
    assert!(err.contains("stash"), "{err}");
    assert_eq!(t.read_file("three.txt"), "edited while paused\n", "{err}");
    assert!(state_path(&git_dir).exists(), "nothing was undone: {err}");
    assert!(git::rebase_is_in_progress(&git_dir), "{err}");
    git::rebase_abort(&workdir).unwrap();
}

/// An undo that resets past the commit takes it with it, so the refusal
/// must not send the user after it.
#[test]
fn continue_drops_the_drop_hint_when_the_rollback_removes_the_commit() {
    let (t, keeper) = crate::core::test_helpers::repo_with_a_redundant_commit_above();
    let workdir = t.workdir();
    let git_dir = t.repo.path().to_path_buf();
    let redundant = t.get_branch_target("alpha").to_string();

    let mut graph = crate::core::weave::Weave::from_repo(&t.repo).unwrap();
    assert!(graph.edit_commit(keeper));
    let protect = vec![redundant];
    crate::core::weave::run_rebase_protecting(
        &workdir,
        Some(&graph.base_oid.to_string()),
        &graph.to_todo(),
        crate::git::Protected::named(&protect),
    )
    .unwrap();

    // `commit`'s shape: its undo resets past the commit it made, leaving
    // the one the refusal names outside the history `loom drop` searches.
    save(
        &git_dir,
        &LoomState {
            command: Resumable::Commit,
            rollback: Rollback {
                reset_mixed_to: graph.base_oid.to_string(),
                ..Default::default()
            },
            context: serde_json::Value::Null,
            protect,
            targets: Vec::new(),
            skipped_fixups: Vec::new(),
        },
    )
    .unwrap();

    let err = continue_cmd(&workdir, &git_dir).unwrap_err().to_string();

    assert!(err.contains("is redundant"), "{err}");
    assert!(!err.contains("loom drop"), "{err}");
    assert!(err.contains("rolled back"), "{err}");
    assert!(!state_path(&git_dir).exists(), "{err}");
}

/// The abort can succeed and the rollback after it still fail. The state
/// file has to survive that: it is the only record of the half-applied undo,
/// and the message must not blame git, which is not holding anything.
#[test]
fn a_failed_rollback_keeps_the_state_and_does_not_blame_git() {
    let test_repo = crate::core::test_helpers::TestRepo::new();
    test_repo.commit("first", "a.txt");
    let workdir = test_repo.workdir();
    let git_dir = test_repo.repo.path().to_path_buf();

    // No rebase is in progress, so `abort_cmd` goes straight to the
    // rollback — which cannot reset to an OID that is not in the repo.
    save(
        &git_dir,
        &LoomState {
            command: Resumable::Commit,
            rollback: Rollback {
                reset_mixed_to: "0".repeat(40),
                ..Default::default()
            },
            context: serde_json::Value::Null,
            protect: Vec::new(),
            targets: Vec::new(),
            skipped_fixups: Vec::new(),
        },
    )
    .unwrap();

    let err = abort_cmd(&workdir, &git_dir).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("half-applied"), "{msg}");
    assert!(
        !msg.contains("index.lock"),
        "the abort worked — git is not what failed here: {msg}"
    );
    assert!(
        state_path(&git_dir).exists(),
        "the record of what is left to undo must survive"
    );
}

/// A rebase paused at an `edit` step is not a finished one: `continue_cmd`
/// must keep the state file and leave the command's post-rebase work for
/// later, however successfully `git rebase --continue` exits.
#[test]
fn continue_keeps_state_when_the_rebase_only_reaches_an_edit() {
    let test_repo = crate::core::test_helpers::TestRepo::new();
    let first = test_repo.commit("first", "a.txt");
    test_repo.commit("second", "b.txt");
    test_repo.commit("third", "c.txt");
    let workdir = test_repo.workdir();
    let git_dir = test_repo.repo.path().to_path_buf();

    // Every commit an `edit`, so the rebase stops twice: once now, and
    // once more when `loom continue` runs `git rebase --continue`.
    crate::git::run_git(
        &workdir,
        &[
            "-c",
            // `sed -i` is GNU-only; rewrite through a temp file instead.
            "sequence.editor=f() { sed 's/^pick/edit/' \"$1\" > \"$1.new\" && mv \"$1.new\" \"$1\"; }; f",
            "rebase",
            "-i",
            &first.to_string(),
        ],
    )
    .unwrap();

    // The context is null, which `drop::after_continue` cannot parse: if
    // the dispatch runs at all, the test fails.
    save(
        &git_dir,
        &LoomState {
            command: Resumable::Drop,
            rollback: Rollback::default(),
            context: serde_json::Value::Null,
            protect: Vec::new(),
            targets: Vec::new(),
            skipped_fixups: Vec::new(),
        },
    )
    .unwrap();

    continue_cmd(&workdir, &git_dir).unwrap();

    assert!(
        crate::git::rebase_is_in_progress(&git_dir),
        "the rebase only advanced to the next `edit` step"
    );
    assert!(
        state_path(&git_dir).exists(),
        "the state must survive a rebase that is still in progress"
    );
}

/// Commit `f.txt` holding `content`, returning the hash.
fn commit_f(t: &crate::core::test_helpers::TestRepo, content: &str, message: &str) -> String {
    t.write_file("f.txt", content);
    t.stage_files(&["f.txt"]);
    t.commit_staged(message);
    t.head_oid().to_string()
}

/// Resolve the conflict on `f.txt` as HEAD's side, which empties the step.
fn resolve_as_head(workdir: &Path) {
    git::run_git(workdir, &["checkout", "HEAD", "--", "f.txt"]).unwrap();
}

fn subjects(workdir: &Path) -> Vec<String> {
    git::run_git_stdout(workdir, &["log", "--format=%s"])
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

/// Run a todo that puts `moved` under `target`: both conflict in turn, the
/// second on the target itself. Returns the two hashes.
fn start_reorder_rebase(t: &crate::core::test_helpers::TestRepo) -> (String, String) {
    let base = commit_f(t, "base\n", "base");
    let target = commit_f(t, "s\n", "target");
    let moved = commit_f(t, "t\n", "moved");
    let todo = format!("label onto\n\nreset onto\npick {moved}\npick {target}\n");
    assert_eq!(
        crate::core::weave::run_rebase(&t.workdir(), Some(&base), &todo).unwrap(),
        git::RebaseOutcome::Stopped
    );
    (target, moved)
}

/// #484: a resolution that empties a target would make `--continue` drop
/// it, and a fixup behind it would then land on the commit below.
#[test]
fn continue_keeps_an_emptied_target_as_an_empty_commit() {
    let t = crate::core::test_helpers::TestRepo::new();
    let workdir = t.workdir();
    let git_dir = t.repo.path().to_path_buf();
    let (target, moved) = start_reorder_rebase(&t);
    save(
        &git_dir,
        &LoomState {
            command: Resumable::Drop,
            rollback: Rollback::default(),
            protect: Vec::new(),
            targets: vec![target.clone()],
            skipped_fixups: Vec::new(),
            context: serde_json::json!({ "commit_hash": moved }),
        },
    )
    .unwrap();

    t.write_file("f.txt", "t\n");
    git::run_git(&workdir, &["add", "f.txt"]).unwrap();
    continue_cmd(&workdir, &git_dir).unwrap();
    assert!(
        git::rebase_is_in_progress(&git_dir),
        "the target's own pick conflicts next"
    );

    resolve_as_head(&workdir);
    continue_cmd(&workdir, &git_dir).unwrap();

    assert!(!git::rebase_is_in_progress(&git_dir));
    assert!(!state_path(&git_dir).exists());
    assert_eq!(&subjects(&workdir)[..3], ["target", "moved", "base"]);
    assert_eq!(
        git::rev_parse(&workdir, "HEAD^{tree}").unwrap(),
        git::rev_parse(&workdir, "HEAD~1^{tree}").unwrap(),
        "kept with no changes of its own"
    );
    let identity = |rev: &str| {
        git::run_git_stdout(&workdir, &["show", "-s", "--format=%an <%ae> %at%n%B", rev]).unwrap()
    };
    assert_eq!(identity("HEAD"), identity(&target));
}

/// A pick outside `targets` that a resolution empties is the user's to
/// drop (#451), `protect` or not.
#[test]
fn continue_lets_an_emptied_protected_commit_go() {
    let t = crate::core::test_helpers::TestRepo::new();
    let workdir = t.workdir();
    let git_dir = t.repo.path().to_path_buf();
    let (target, moved) = start_reorder_rebase(&t);
    save(
        &git_dir,
        &LoomState {
            command: Resumable::Drop,
            rollback: Rollback::default(),
            protect: vec![target],
            targets: Vec::new(),
            skipped_fixups: Vec::new(),
            context: serde_json::json!({ "commit_hash": moved }),
        },
    )
    .unwrap();

    t.write_file("f.txt", "t\n");
    git::run_git(&workdir, &["add", "f.txt"]).unwrap();
    continue_cmd(&workdir, &git_dir).unwrap();
    resolve_as_head(&workdir);
    continue_cmd(&workdir, &git_dir).unwrap();

    assert!(!git::rebase_is_in_progress(&git_dir));
    assert_eq!(&subjects(&workdir)[..2], ["moved", "base"]);
}

/// Start a fold whose fixup conflicts: its diff is `m` → `f`, replayed
/// onto the target's `t`. `above` replays cleanly once the fixup is gone.
/// With `source` it is a `fold <commit> <commit>`, whose fixup line is
/// the source commit itself.
fn start_fixup_rebase(
    t: &crate::core::test_helpers::TestRepo,
    rest: &dyn Fn(&str) -> String,
    source: bool,
) -> (String, String) {
    let base = commit_f(t, "base\n", "base");
    let target = commit_f(t, "t\n", "target");
    let above = commit_f(t, "m\n", "above");
    let fixup = commit_f(t, "f\n", if source { "source" } else { "fixup! target" });
    let todo = format!(
        "label onto\n\nreset onto\npick {target}\nfixup {fixup}\npick {above}\n{}",
        rest(&fixup)
    );
    assert_eq!(
        crate::core::weave::run_rebase(&t.workdir(), Some(&base), &todo).unwrap(),
        git::RebaseOutcome::Stopped
    );
    t.create_branch_at("_loom-track", &target);
    save(
        t.repo.path(),
        &LoomState {
            command: Resumable::Fold,
            rollback: Rollback {
                delete_branches: vec!["_loom-track".to_string()],
                ..Default::default()
            },
            protect: Vec::new(),
            targets: vec![target.clone()],
            skipped_fixups: Vec::new(),
            context: if source {
                serde_json::json!({
                    "op": "CommitIntoCommit",
                    "source_hash": fixup,
                    "target_hash": target,
                })
            } else {
                serde_json::json!({
                    "op": "FilesIntoCommit",
                    "original_commit_hash": target,
                    "files_count": 1,
                })
            },
        },
    )
    .unwrap();
    (target, fixup)
}

/// #483: a resolution that empties a fixup makes `--continue` skip it, and
/// that fixup commit was the only place the change lived.
#[test]
fn continue_puts_a_skipped_fixups_change_back_in_the_worktree() {
    let t = crate::core::test_helpers::TestRepo::new();
    let workdir = t.workdir();
    let git_dir = t.repo.path().to_path_buf();
    start_fixup_rebase(&t, &|_| String::new(), false);

    resolve_as_head(&workdir);
    continue_cmd(&workdir, &git_dir).unwrap();

    assert!(!git::rebase_is_in_progress(&git_dir));
    assert!(!state_path(&git_dir).exists());
    assert_eq!(
        git::run_git_stdout(&workdir, &["show", "HEAD:f.txt"]).unwrap(),
        "m\n",
        "the fixup folded nothing, and `above` replayed over it"
    );
    assert_eq!(t.read_file("f.txt"), "f\n");
    assert_eq!(git::diff_cached(&workdir).unwrap(), "", "back unstaged");
    assert!(!t.branch_exists("_loom-track"));
}

/// The record of a skipped fixup has to outlive the stops after it, and a
/// change that no longer applies is parked rather than dropped.
#[test]
fn a_skipped_fixup_survives_a_later_stop_and_is_parked_when_it_will_not_apply() {
    let t = crate::core::test_helpers::TestRepo::new();
    let workdir = t.workdir();
    let git_dir = t.repo.path().to_path_buf();
    // `later` changes `f` → `y`, which conflicts on the `m` left once the
    // fixup is skipped.
    let (_, fixup) = start_fixup_rebase(
        &t,
        &|_| {
            let later = commit_f(&t, "y\n", "later");
            format!("pick {later}\n")
        },
        false,
    );

    resolve_as_head(&workdir);
    continue_cmd(&workdir, &git_dir).unwrap();
    assert!(git::rebase_is_in_progress(&git_dir), "`later` conflicts");
    assert_eq!(load(&git_dir).unwrap().unwrap().skipped_fixups, [fixup]);

    t.write_file("f.txt", "y\n");
    git::run_git(&workdir, &["add", "f.txt"]).unwrap();
    continue_cmd(&workdir, &git_dir).unwrap();

    assert!(!state_path(&git_dir).exists());
    assert_eq!(
        t.read_file("f.txt"),
        "y\n",
        "a change that will not apply leaves the tree alone"
    );
    let parked = std::fs::read_to_string(git_dir.join("loom").join("unfolded-0.patch")).unwrap();
    assert!(parked.contains("\n+f\n"), "{parked}");
}

/// A `loom continue` retried after its handler failed restores again
/// (Spec 014, *State Deleted Only After Success*): a change the tree
/// already holds must be left alone, not applied twice or parked as a
/// duplicate.
#[test]
fn a_retried_continue_does_not_restore_a_skipped_fixup_twice() {
    let t = crate::core::test_helpers::TestRepo::new();
    let workdir = t.workdir();
    let git_dir = t.repo.path().to_path_buf();
    let (target, fixup) = start_fixup_rebase(&t, &|_| String::new(), false);
    resolve_as_head(&workdir);
    continue_cmd(&workdir, &git_dir).unwrap();
    assert_eq!(t.read_file("f.txt"), "f\n");

    // The state as the handler left it before its failure.
    t.create_branch_at("_loom-track", &target);
    save(
        &git_dir,
        &LoomState {
            command: Resumable::Fold,
            rollback: Rollback::default(),
            protect: Vec::new(),
            targets: vec![target.clone()],
            skipped_fixups: vec![fixup],
            context: serde_json::json!({
                "op": "FilesIntoCommit",
                "original_commit_hash": target,
                "files_count": 1,
            }),
        },
    )
    .unwrap();
    continue_cmd(&workdir, &git_dir).unwrap();

    assert_eq!(t.read_file("f.txt"), "f\n");
    assert!(!state_path(&git_dir).exists());
    assert!(
        !git_dir.join("loom").join("unfolded-0.patch").exists(),
        "nothing to park: the change is already there"
    );
}

/// A retried `loom continue` meets a change it parked last time: naming
/// that patch again, not parking a duplicate.
#[test]
fn a_retried_continue_does_not_park_a_skipped_fixup_twice() {
    let t = crate::core::test_helpers::TestRepo::new();
    let workdir = t.workdir();
    let git_dir = t.repo.path().to_path_buf();
    // `later` conflicts on the `m` left once the fixup is skipped, so the
    // change is parked.
    let (target, fixup) = start_fixup_rebase(
        &t,
        &|_| {
            let later = commit_f(&t, "y\n", "later");
            format!("pick {later}\n")
        },
        false,
    );
    resolve_as_head(&workdir);
    continue_cmd(&workdir, &git_dir).unwrap();
    t.write_file("f.txt", "y\n");
    git::run_git(&workdir, &["add", "f.txt"]).unwrap();
    continue_cmd(&workdir, &git_dir).unwrap();
    let loom_dir = git_dir.join("loom");
    assert!(loom_dir.join("unfolded-0.patch").exists());

    // The state as the handler left it before its failure.
    t.create_branch_at("_loom-track", &target);
    save(
        &git_dir,
        &LoomState {
            command: Resumable::Fold,
            rollback: Rollback::default(),
            protect: Vec::new(),
            targets: vec![target.clone()],
            skipped_fixups: vec![fixup],
            context: serde_json::json!({
                "op": "FilesIntoCommit",
                "original_commit_hash": target,
                "files_count": 1,
            }),
        },
    )
    .unwrap();
    continue_cmd(&workdir, &git_dir).unwrap();

    assert!(!state_path(&git_dir).exists());
    assert!(!loom_dir.join("unfolded-1.patch").exists(), "parked once");
    assert_eq!(t.read_file("f.txt"), "y\n");
}

/// `fold <commit> <commit>` squashes the source commit itself: skipped,
/// the source is gone from history and its change comes back the same
/// way a fixup's does.
#[test]
fn continue_puts_a_skipped_source_commits_change_back_in_the_worktree() {
    let t = crate::core::test_helpers::TestRepo::new();
    let workdir = t.workdir();
    let git_dir = t.repo.path().to_path_buf();
    start_fixup_rebase(&t, &|_| String::new(), true);

    resolve_as_head(&workdir);
    continue_cmd(&workdir, &git_dir).unwrap();

    assert!(!state_path(&git_dir).exists());
    assert_eq!(&subjects(&workdir)[..2], ["above", "target"]);
    assert_eq!(t.read_file("f.txt"), "f\n");
    assert_eq!(git::diff_cached(&workdir).unwrap(), "");
}

/// A skipped fixup that will neither apply nor park lives only in its
/// commit, so the continue fails before dispatch deletes the state.
#[test]
fn continue_keeps_the_state_when_a_skipped_fixup_cannot_be_parked() {
    let t = crate::core::test_helpers::TestRepo::new();
    let workdir = t.workdir();
    let git_dir = t.repo.path().to_path_buf();
    // `later` conflicts on the `m` left once the fixup is skipped, so the
    // change will not apply over it.
    let (_, fixup) = start_fixup_rebase(
        &t,
        &|_| {
            let later = commit_f(&t, "y\n", "later");
            format!("pick {later}\n")
        },
        false,
    );
    resolve_as_head(&workdir);
    continue_cmd(&workdir, &git_dir).unwrap();
    t.write_file("f.txt", "y\n");
    git::run_git(&workdir, &["add", "f.txt"]).unwrap();
    // Every name the park would take is already in use.
    for n in 0..1000 {
        std::fs::write(git_dir.join("loom").join(format!("unfolded-{n}.patch")), "").unwrap();
    }

    let err = continue_cmd(&workdir, &git_dir).unwrap_err().to_string();

    assert!(err.contains("could not be saved"), "{err}");
    assert!(!git::rebase_is_in_progress(&git_dir));
    assert_eq!(load(&git_dir).unwrap().unwrap().skipped_fixups, [fixup]);
}

/// A skipped fixup whose diff cannot be read has nothing to park, so the
/// continue fails before dispatch: deleting the state would take the only
/// pointer to that change with it.
#[test]
fn continue_keeps_the_state_when_a_skipped_fixup_cannot_be_read() {
    let t = crate::core::test_helpers::TestRepo::new();
    t.commit("only", "a.txt");
    let workdir = t.workdir();
    let git_dir = t.repo.path().to_path_buf();
    save(
        &git_dir,
        &LoomState {
            command: Resumable::Drop,
            rollback: Rollback::default(),
            protect: Vec::new(),
            targets: Vec::new(),
            skipped_fixups: vec!["0".repeat(40)],
            context: serde_json::json!({ "commit_hash": "abc" }),
        },
    )
    .unwrap();

    let err = continue_cmd(&workdir, &git_dir).unwrap_err().to_string();

    assert!(err.contains("could not be read back"), "{err}");
    assert!(state_path(&git_dir).exists());
}

/// A command this loom does not know comes from another loom version, whose
/// undo this one cannot vouch for: ending the rebase, or undoing what it can
/// read, may leave the user worse off than letting that version finish.
#[test]
fn abort_leaves_a_state_naming_an_unknown_command_and_its_rebase_alone() {
    let t = crate::core::test_helpers::TestRepo::new();
    let workdir = t.workdir();
    let git_dir = t.repo.path().to_path_buf();
    start_reorder_rebase(&t);
    let json = r#"{"command":"frobnicate","rollback":{"reset_hard_to":"0"},"context":null}"#;
    std::fs::create_dir_all(git_dir.join("loom")).unwrap();
    std::fs::write(state_path(&git_dir), json).unwrap();

    let err = abort_cmd(&workdir, &git_dir).unwrap_err().to_string();

    assert!(err.contains("`loom frobnicate`"), "{err}");
    assert!(git::rebase_is_in_progress(&git_dir), "{err}");
    assert_eq!(std::fs::read_to_string(state_path(&git_dir)).unwrap(), json);
    git::rebase_abort(&workdir).unwrap();
}
