use std::fs::File;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::core::test_helpers::TestRepo;
use crate::core::weave::{self, EmptiedRefs, Weave, run_rebase};
use crate::git::{self, RebaseOutcome};

/// A whole-second mtime an hour ago, exact on every filesystem.
fn an_hour_ago() -> SystemTime {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    UNIX_EPOCH + Duration::from_secs(now - 3600)
}

fn set_mtime(workdir: &Path, name: &str, time: SystemTime) {
    File::options()
        .write(true)
        .open(workdir.join(name))
        .unwrap()
        .set_modified(time)
        .unwrap();
}

fn mtime(workdir: &Path, name: &str) -> SystemTime {
    std::fs::metadata(workdir.join(name))
        .unwrap()
        .modified()
        .unwrap()
}

/// The mtime git recorded for `name` in the index, to the second: the
/// refresh after a restore must have landed for it to equal the file's.
fn index_mtime_secs(workdir: &Path, name: &str) -> SystemTime {
    let debug = git::run_git_stdout(workdir, &["ls-files", "--debug", "--", name]).unwrap();
    let line = debug
        .lines()
        .find_map(|l| l.trim().strip_prefix("mtime: "))
        .unwrap();
    let secs: u64 = line.split(':').next().unwrap().parse().unwrap();
    UNIX_EPOCH + Duration::from_secs(secs)
}

fn identity_todo(t: &TestRepo) -> (String, String) {
    let graph = Weave::from_repo(&t.repo).unwrap();
    (graph.to_todo(), graph.base_oid.to_string())
}

#[test]
fn replay_keeps_mtimes_of_unchanged_files() {
    let t = TestRepo::new_with_remote();
    t.commit("C1", "a.txt");
    t.commit("C2", "é b.txt");
    let workdir = t.workdir();
    let old = an_hour_ago();
    set_mtime(&workdir, "a.txt", old);
    set_mtime(&workdir, "é b.txt", old);

    let (todo, base) = identity_todo(&t);
    let outcome = run_rebase(&workdir, Some(&base), &todo).unwrap();

    assert_eq!(outcome, RebaseOutcome::Completed);
    assert_eq!(mtime(&workdir, "a.txt"), old);
    assert_eq!(mtime(&workdir, "é b.txt"), old);
    assert_eq!(index_mtime_secs(&workdir, "a.txt"), old);
}

#[test]
fn changed_file_gets_a_new_mtime() {
    let t = TestRepo::new_with_remote();
    t.commit("C1", "a.txt");
    let c2 = t.commit("C2", "a.txt");
    t.commit("C3", "b.txt");
    let workdir = t.workdir();
    let old = an_hour_ago();
    set_mtime(&workdir, "a.txt", old);
    set_mtime(&workdir, "b.txt", old);

    let mut graph = Weave::from_repo(&t.repo).unwrap();
    assert!(graph.drop_commit(c2, EmptiedRefs::Park).is_some());
    let outcome = run_rebase(
        &workdir,
        Some(&graph.base_oid.to_string()),
        &graph.to_todo(),
    )
    .unwrap();

    assert_eq!(outcome, RebaseOutcome::Completed);
    assert_eq!(t.read_file("a.txt"), "C1");
    assert!(mtime(&workdir, "a.txt") > old);
    assert_eq!(mtime(&workdir, "b.txt"), old);
}

/// The autostash replay rewrites a dirty file too, bytes unchanged.
#[test]
fn dirty_file_comes_back_with_its_mtime() {
    let t = TestRepo::new_with_remote();
    t.commit("C1", "a.txt");
    t.commit("C2", "b.txt");
    let workdir = t.workdir();
    t.write_file("a.txt", "edited");
    let old = an_hour_ago();
    set_mtime(&workdir, "a.txt", old);

    let (todo, base) = identity_todo(&t);
    let outcome = run_rebase(&workdir, Some(&base), &todo).unwrap();

    assert_eq!(outcome, RebaseOutcome::Completed);
    assert_eq!(t.read_file("a.txt"), "edited");
    assert_eq!(mtime(&workdir, "a.txt"), old);
}

/// `git diff HEAD` does not list a file staged with other bytes and put back
/// by hand, yet the autostash's reset rewrites it.
#[test]
fn staged_only_file_comes_back_with_its_mtime() {
    let t = TestRepo::new_with_remote();
    // s.txt sits below the base, so only the autostash can touch it.
    t.commit("Base", "s.txt");
    t.push_branch_to_remote_main("integration");
    t.commit("C1", "a.txt");
    let workdir = t.workdir();
    t.write_file("s.txt", "staged");
    t.stage_files(&["s.txt"]);
    t.write_file("s.txt", "Base");
    let old = an_hour_ago();
    set_mtime(&workdir, "s.txt", old);

    let (todo, base) = identity_todo(&t);
    let outcome = run_rebase(&workdir, Some(&base), &todo).unwrap();

    assert_eq!(outcome, RebaseOutcome::Completed);
    assert_eq!(t.read_file("s.txt"), "Base");
    assert_eq!(mtime(&workdir, "s.txt"), old);
}

/// Reword's `edit` stop is driven by the same invocation, so the record
/// outlives that pause.
#[test]
fn edit_stop_driven_in_process_keeps_mtimes() {
    let t = TestRepo::new_with_remote();
    let c1 = t.commit("C1", "a.txt");
    t.commit("C2", "b.txt");
    let workdir = t.workdir();
    let old = an_hour_ago();
    set_mtime(&workdir, "a.txt", old);
    set_mtime(&workdir, "b.txt", old);

    weave::start_edit_rebase(&t.repo, &workdir, c1).unwrap();
    let outcome = git::continue_rebase(&workdir).unwrap();

    assert_eq!(outcome, RebaseOutcome::Completed);
    assert_eq!(mtime(&workdir, "a.txt"), old);
    assert_eq!(mtime(&workdir, "b.txt"), old);
}

/// Reporting a pause hands the tree to the user, who may build it before
/// going on: an older mtime put back afterwards would pass that build off as
/// current.
#[test]
fn a_reported_pause_drops_the_record() {
    let t = TestRepo::new_with_remote();
    let c1 = t.commit("C1", "a.txt");
    t.commit("C2", "a.txt");
    t.commit("C3", "b.txt");
    let workdir = t.workdir();
    let old = an_hour_ago();
    set_mtime(&workdir, "b.txt", old);

    // Dropping the commit that created a.txt makes C2's edit of it conflict.
    let mut graph = Weave::from_repo(&t.repo).unwrap();
    assert!(graph.drop_commit(c1, EmptiedRefs::Park).is_some());
    let outcome = run_rebase(
        &workdir,
        Some(&graph.base_oid.to_string()),
        &graph.to_todo(),
    )
    .unwrap();
    assert_eq!(outcome, RebaseOutcome::Stopped);
    crate::core::transaction::warn_paused(&workdir, "drop");

    t.write_file("a.txt", "C2");
    t.stage_files(&["a.txt"]);
    let outcome = git::continue_rebase(&workdir).unwrap();

    assert_eq!(outcome, RebaseOutcome::Completed);
    assert_eq!(t.read_file("b.txt"), "C3");
    assert!(mtime(&workdir, "b.txt") > old);
}

/// Nobody built in between, so an abort the invocation runs itself restores.
#[test]
fn in_process_abort_after_a_stop_restores_mtimes() {
    let t = TestRepo::new_with_remote();
    let c1 = t.commit("C1", "a.txt");
    t.commit("C2", "a.txt");
    let workdir = t.workdir();
    let old = an_hour_ago();
    set_mtime(&workdir, "a.txt", old);

    let mut graph = Weave::from_repo(&t.repo).unwrap();
    assert!(graph.drop_commit(c1, EmptiedRefs::Park).is_some());
    let outcome = run_rebase(
        &workdir,
        Some(&graph.base_oid.to_string()),
        &graph.to_todo(),
    )
    .unwrap();
    assert_eq!(outcome, RebaseOutcome::Stopped);

    git::rebase_abort(&workdir).unwrap();

    assert_eq!(t.read_file("a.txt"), "C2");
    assert_eq!(mtime(&workdir, "a.txt"), old);
}

/// `update`'s plain pull-rebase records and restores like a weave rebase.
#[test]
fn plain_rebase_keeps_mtimes_of_unchanged_files() {
    let t = TestRepo::new_with_remote();
    t.commit("C1", "a.txt");
    t.add_remote_commits(&["Upstream"]);
    t.fetch_remote();
    let workdir = t.workdir();
    let git_dir = t.repo.path().to_path_buf();
    let old = an_hour_ago();
    set_mtime(&workdir, "a.txt", old);

    let outcome = git::rebase(&git_dir, &workdir, "origin/main").unwrap();

    assert_eq!(outcome, RebaseOutcome::Completed);
    assert_eq!(mtime(&workdir, "a.txt"), old);
}

/// The record and the rebase state both live in the worktree's own git dir.
#[test]
fn replay_in_a_linked_worktree_keeps_mtimes() {
    let t = TestRepo::new_with_remote();
    let base = t.find_remote_branch_target("origin/main").to_string();
    let wt = t.add_worktree("wt", &["-b", "wt-int", &base]);
    std::fs::write(wt.join("a.txt"), "A").unwrap();
    git::run_git(&wt, &["add", "a.txt"]).unwrap();
    git::run_git(&wt, &["commit", "-q", "-m", "A"]).unwrap();
    let old = an_hour_ago();
    set_mtime(&wt, "a.txt", old);

    let todo = format!("pick {}\n", git::rev_parse(&wt, "HEAD").unwrap());
    let outcome = run_rebase(&wt, Some(&base), &todo).unwrap();

    assert_eq!(outcome, RebaseOutcome::Completed);
    assert_eq!(mtime(&wt, "a.txt"), old);
}

/// An empty replay under `--empty=stop` is a stop the invocation carries past
/// with `--skip` itself; the record must survive it.
#[test]
fn skipped_empty_replay_keeps_mtimes() {
    let t = TestRepo::new_with_remote();
    let c1 = t.commit("C1", "a.txt");
    let c2 = t.commit("C2", "b.txt");
    let workdir = t.workdir();
    let base = t.find_remote_branch_target("origin/main").to_string();
    let old = an_hour_ago();
    set_mtime(&workdir, "a.txt", old);
    set_mtime(&workdir, "b.txt", old);

    let todo = format!(
        "pick {c1}
pick {c1}
pick {c2}
"
    );
    let outcome =
        weave::run_rebase_protecting(&workdir, Some(&base), &todo, git::Protected::named(&[]))
            .unwrap();

    assert_eq!(outcome, RebaseOutcome::Completed);
    assert_eq!(mtime(&workdir, "a.txt"), old);
    assert_eq!(mtime(&workdir, "b.txt"), old);
}
