use super::{
    LeftOut, binary_stamp, collect_commit_hunks, collect_file_entries, filter_paths,
    put_index_back, run_commit_hunk_picker, run_hunk_picker, save_and_unstage_staged,
    selected_paths, still_listed,
};
use crate::core::diff::DiffHunk;
use crate::core::hunk_select::{self, HunkArgs, Picker};
use crate::core::hunks::{FileEntry, HunkEntry, HunkOrigin};
use crate::core::repo;
use crate::core::test_helpers::TestRepo;
use crate::core::trace_log;

/// A repo with changes to `a.rs` and `dir/b.rs`. It needs an upstream because
/// an argument that is not a path falls through to short-ID resolution.
fn repo_with_changes() -> TestRepo {
    let test_repo = TestRepo::new_with_remote();
    test_repo.write_file("a.rs", "one\n");
    std::fs::create_dir(test_repo.workdir().join("dir")).unwrap();
    test_repo.write_file("dir/b.rs", "one\n");
    test_repo.stage_files(&["a.rs", "dir/b.rs"]);
    test_repo.commit_staged("Add files");
    test_repo.write_file("a.rs", "changed\n");
    test_repo.write_file("dir/b.rs", "changed\n");
    test_repo
}

fn filter(test_repo: &TestRepo, args: &[&str]) -> anyhow::Result<Option<Vec<String>>> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    test_repo.in_dir(|| filter_paths(&repo::open_repo().unwrap(), &args))
}

#[test]
fn no_files_means_every_change() {
    assert_eq!(filter(&repo_with_changes(), &[]).unwrap(), None);
}

#[test]
fn zz_means_every_change_even_beside_a_path() {
    let test_repo = repo_with_changes();
    assert_eq!(filter(&test_repo, &["zz"]).unwrap(), None);
    assert_eq!(filter(&test_repo, &["a.rs", "zz"]).unwrap(), None);
}

#[test]
fn paths_resolve_to_themselves_including_below_the_root() {
    let filtered = filter(&repo_with_changes(), &["a.rs", "dir/b.rs"]).unwrap();
    assert_eq!(
        filtered,
        Some(vec!["a.rs".to_string(), "dir/b.rs".to_string()])
    );
}

#[test]
fn an_argument_naming_nothing_is_refused() {
    let err = filter(&repo_with_changes(), &["nope.rs"]).unwrap_err();
    assert!(err.to_string().contains("nope.rs"), "{err}");
}

/// The rationale for resolving once: a short ID names what is changed now.
#[test]
fn a_short_id_resolves_to_its_path() {
    let test_repo = repo_with_changes();
    let short_id = test_repo.in_dir(|| {
        let repo = repo::open_repo().unwrap();
        let info = repo::gather_repo_info(&repo, false, 1).unwrap();
        let allocator = crate::core::shortid::IdAllocator::new(info.collect_entities());
        allocator.get_file("a.rs").to_string()
    });
    assert_ne!(short_id, "a.rs", "the test needs a real short ID");
    assert_eq!(
        filter(&test_repo, &[short_id.as_str()]).unwrap(),
        Some(vec!["a.rs".to_string()])
    );
}

fn hunk(origin: HunkOrigin, selected: bool) -> HunkEntry {
    HunkEntry {
        hunk: DiffHunk {
            text: "@@ -1 +1 @@\n-a\n+b\n".to_string(),
            modified_lines: vec![1],
        },
        selected,
        origin,
    }
}

fn entry(path: &str, hunks: Vec<HunkEntry>) -> FileEntry {
    FileEntry {
        path: path.to_string(),
        hunks,
        index_status: 'M',
        worktree_status: 'M',
        binary: false,
    }
}

#[test]
fn a_file_with_no_selected_hunk_is_not_folded() {
    let entries = vec![entry(
        "a.rs",
        vec![
            hunk(HunkOrigin::Unstaged, false),
            hunk(HunkOrigin::Staged, false),
        ],
    )];
    assert!(selected_paths(&entries).is_empty());
}

/// A staged hunk left selected is a choice the picker offered and the user kept,
/// so its path belongs in the fold.
#[test]
fn a_kept_staged_hunk_is_folded() {
    let entries = vec![entry("a.rs", vec![hunk(HunkOrigin::Staged, true)])];
    assert_eq!(selected_paths(&entries), vec!["a.rs".to_string()]);
}

/// The whole point of the fix: a staged change the picker has no hunk for — a
/// mode-only one — never reaches the listing, so nobody can pick it and the
/// fold cannot name it (Spec 007).
#[cfg(unix)]
#[test]
fn a_mode_only_staged_change_is_not_listed() {
    use std::os::unix::fs::PermissionsExt;

    let test_repo = repo_with_changes();
    let script = test_repo.workdir().join("m.sh");
    std::fs::write(&script, "#!/bin/sh\n").unwrap();
    test_repo.stage_files(&["m.sh"]);
    test_repo.commit_staged("Add a script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    test_repo.stage_files(&["m.sh"]);

    let workdir = test_repo.workdir().to_path_buf();
    let entries = test_repo
        .in_dir(|| super::collect_file_entries(&test_repo.repo, &workdir, None))
        .unwrap();

    let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
    assert!(!paths.contains(&"m.sh"), "{paths:?}");
    assert!(
        paths.contains(&"a.rs"),
        "the other changes are still listed"
    );
}

#[test]
fn only_the_picked_files_are_folded_and_order_is_kept() {
    let entries = vec![
        entry("a.rs", vec![hunk(HunkOrigin::Unstaged, true)]),
        entry("b.rs", vec![hunk(HunkOrigin::Unstaged, false)]),
        entry(
            "c.rs",
            vec![
                hunk(HunkOrigin::Staged, false),
                hunk(HunkOrigin::Unstaged, true),
            ],
        ),
    ];
    assert_eq!(
        selected_paths(&entries),
        vec!["a.rs".to_string(), "c.rs".to_string()]
    );
}

fn interactive_picker() -> Picker {
    hunk_select::worktree_picker(HunkArgs::default(), "loom add".to_string(), None, &[])
}

#[test]
fn what_the_selector_picks_is_staged() {
    let test_repo = repo_with_changes();
    let workdir = test_repo.workdir();
    let picked = test_repo
        .in_dir(|| {
            run_hunk_picker(
                &test_repo.repo,
                &workdir,
                None,
                &interactive_picker(),
                LeftOut::Unstaged,
                |mut files| {
                    let a = files.iter_mut().find(|f| f.path == "a.rs").unwrap();
                    a.hunks.iter_mut().for_each(|h| h.selected = true);
                    Ok(Some(files))
                },
            )
        })
        .unwrap()
        .expect("the selector confirmed");

    assert_eq!(picked.paths, vec!["a.rs".to_string()]);
    let status = test_repo.status_porcelain();
    assert!(status.contains("M  a.rs"), "{status}");
    assert!(status.contains(" M dir/b.rs"), "{status}");
}

#[test]
fn a_cancelled_selector_leaves_the_index_alone() {
    let test_repo = repo_with_changes();
    test_repo.stage_files(&["dir/b.rs"]);
    let before = test_repo.status_porcelain();

    let workdir = test_repo.workdir();
    let picked = test_repo
        .in_dir(|| {
            run_hunk_picker(
                &test_repo.repo,
                &workdir,
                None,
                &interactive_picker(),
                LeftOut::KeptStaged,
                |_| Ok(None),
            )
        })
        .unwrap();

    assert!(picked.is_none());
    assert_eq!(test_repo.status_porcelain(), before);
}

#[test]
fn the_commit_selector_is_offered_the_commit_hunks_unselected() {
    let test_repo = repo_with_changes();
    let head = test_repo.head_oid().to_string();
    let workdir = test_repo.workdir();

    let offered = run_commit_hunk_picker(&workdir, &head, &[], &interactive_picker(), |files| {
        Ok(Some(files))
    })
    .unwrap()
    .expect("the selector confirmed");

    let paths: Vec<&str> = offered.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["a.rs", "dir/b.rs"]);
    assert!(offered.iter().flat_map(|f| &f.hunks).all(|h| !h.selected));
}

/// The guard is what every `?` between the unstaging and the end of an
/// operation relies on, so its two states are pinned here rather than only
/// through the commands that hold it.
#[test]
fn the_set_aside_staging_comes_back_when_the_guard_drops() {
    let test_repo = TestRepo::new();
    test_repo.commit("A1", "a1.txt");
    test_repo.write_file("kept.txt", "staged");
    test_repo.stage_files(&["kept.txt"]);

    let workdir = test_repo.repo.workdir().unwrap().to_path_buf();
    {
        let guard = save_and_unstage_staged(&test_repo.repo, &workdir).unwrap();
        assert!(!guard.patch().is_empty());
        let status = test_repo.status_porcelain();
        assert!(!status.contains("A  kept.txt"), "{status}");
    }

    let status = test_repo.status_porcelain();
    assert!(status.contains("A  kept.txt"), "{status}");
}

#[test]
fn a_released_guard_leaves_the_index_to_its_new_owner() {
    let test_repo = TestRepo::new();
    test_repo.commit("A1", "a1.txt");
    test_repo.write_file("kept.txt", "staged");
    test_repo.stage_files(&["kept.txt"]);

    let workdir = test_repo.repo.workdir().unwrap().to_path_buf();
    let patch = {
        let guard = save_and_unstage_staged(&test_repo.repo, &workdir).unwrap();
        guard.release()
    };

    let status = test_repo.status_porcelain();
    assert!(!status.contains("A  kept.txt"), "{status}");
    assert!(patch.contains("kept.txt"), "{patch}");
}

#[test]
fn a_handed_over_guard_leaves_the_index_to_its_new_owner() {
    let test_repo = TestRepo::new();
    test_repo.commit("A1", "a1.txt");
    test_repo.write_file("kept.txt", "staged");
    test_repo.stage_files(&["kept.txt"]);

    let workdir = test_repo.repo.workdir().unwrap().to_path_buf();
    save_and_unstage_staged(&test_repo.repo, &workdir)
        .unwrap()
        .handed_over();

    let status = test_repo.status_porcelain();
    assert!(!status.contains("A  kept.txt"), "{status}");
}

/// The handover a failed abort never reaches. `rebase_abort_then_cleanup`
/// skips its closure when the abort fails, so disarming before the call
/// dropped the patch on the one path that cannot get it back: the `-p` folds
/// save no `LoomState` for `loom abort` to read it out of, and the snapshot
/// that owns it only replays from inside that closure.
#[test]
fn a_handover_the_cleanup_never_reached_parks_the_patch() {
    let test_repo = TestRepo::new();
    test_repo.commit("A1", "a1.txt");
    test_repo.write_file("kept.txt", "staged");
    test_repo.stage_files(&["kept.txt"]);

    let workdir = test_repo.repo.workdir().unwrap().to_path_buf();
    let guard = save_and_unstage_staged(&test_repo.repo, &workdir).unwrap();
    // A rebase dir with no rebase under it: the abort git tries here fails.
    std::fs::create_dir_all(test_repo.repo.path().join("rebase-merge")).unwrap();

    let err =
        crate::git::rebase_abort_then_cleanup(&workdir, anyhow::anyhow!("the fold failed"), || {
            guard.handed_over()
        });
    assert!(err.to_string().contains("The abort failed"), "{err}");

    drop(guard);

    let parked = crate::git::git_path(&workdir, "loom").unwrap();
    assert!(
        std::fs::read_dir(&parked)
            .expect("the patch is parked under the git dir")
            .filter_map(|e| e.ok())
            .any(|e| e
                .file_name()
                .to_string_lossy()
                .starts_with("unrestored-staged")),
        "a cleanup that never ran leaves the guard to hand the patch over"
    );
}

#[test]
fn a_submodule_is_stamped_with_its_checked_out_commit() {
    let test_repo = TestRepo::new_with_remote();
    let (first, second) = test_repo.add_submodule("sub");
    test_repo.commit_staged("Add sub");
    test_repo.checkout_submodule("sub", second);

    let workdir = test_repo.workdir();
    let entries =
        collect_file_entries(&test_repo.repo, &workdir, Some(&["sub".to_string()])).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        binary_stamp(&workdir, &entries),
        vec![Some(second), Some(first)]
    );
}

#[test]
fn a_binary_file_staged_anew_since_the_pick_is_no_longer_listed() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    let logo = workdir.join("logo.bin");
    std::fs::write(&logo, b"\0one").unwrap();
    test_repo.stage_files(&["logo.bin"]);
    test_repo.commit_staged("Add logo");
    std::fs::write(&logo, b"\0two").unwrap();
    test_repo.stage_files(&["logo.bin"]);
    std::fs::write(&logo, b"\0three").unwrap();
    let picked = collect_file_entries(&test_repo.repo, &workdir, None).unwrap();
    let stamp = binary_stamp(&workdir, &picked);
    assert!(still_listed(&test_repo.repo, &workdir, &picked, &stamp).unwrap());

    // Another version staged, the working tree written back as it was.
    std::fs::write(&logo, b"\0four").unwrap();
    test_repo.stage_files(&["logo.bin"]);
    std::fs::write(&logo, b"\0three").unwrap();

    assert!(!still_listed(&test_repo.repo, &workdir, &picked, &stamp).unwrap());
}

const TEN: &str = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";

/// Text edits staged and not, CRLF and Latin-1 edits, added, empty, deleted,
/// renamed, binary and untracked files and a submodule, on paths git quotes
/// (`é.txt`) or that split ambiguously in a `diff --git` header (`x b/y.txt`).
fn repo_with_mixed_changes() -> TestRepo {
    let t = TestRepo::new_with_remote();
    let workdir = t.workdir();
    let write = |path: &str, bytes: &[u8]| std::fs::write(workdir.join(path), bytes).unwrap();
    std::fs::create_dir(workdir.join("x b")).unwrap();
    let texts = [
        "a.txt",
        "with space.txt",
        "x b/y.txt",
        "é.txt",
        "gone.txt",
        "lost.txt",
        "old name.txt",
    ];
    for path in texts {
        t.write_file(path, TEN);
    }
    write("crlf.txt", b"one\r\ntwo\r\n");
    write("latin1.txt", b"caf\xe9\n");
    write("logo.bin", b"\0one");
    let (_, second) = t.add_submodule("sub");
    t.stage_files(&texts);
    t.stage_files(&["crlf.txt", "latin1.txt", "logo.bin"]);
    t.commit_staged("Base");

    t.write_file("a.txt", &TEN.replace("1\n", "one\n"));
    t.stage_files(&["a.txt"]);
    t.write_file(
        "a.txt",
        &TEN.replace("1\n", "one\n").replace("10\n", "ten\n"),
    );
    for path in ["with space.txt", "x b/y.txt", "é.txt"] {
        t.write_file(path, &TEN.replace("5\n", "five\n"));
    }
    write("crlf.txt", b"one\r\nTWO\r\n");
    write("latin1.txt", b"caf\xe9!\n");
    write("logo.bin", b"\0two");
    t.stage_files(&["logo.bin"]);
    write("logo.bin", b"\0three");
    t.write_file("new.txt", "new\n");
    t.write_file("empty.txt", "");
    t.stage_files(&["new.txt", "empty.txt"]);
    crate::git::run_git(&workdir, &["rm", "-q", "gone.txt"]).unwrap();
    crate::git::run_git(&workdir, &["mv", "old name.txt", "new name.txt"]).unwrap();
    std::fs::remove_file(workdir.join("lost.txt")).unwrap();
    t.write_file("untracked.txt", "u\n");
    t.write_file("untracked-empty.txt", "");
    write("untracked.bin", b"\0u");
    t.checkout_submodule("sub", second);
    t
}

/// One line per hunk offered: its file's state, its own, and its text.
fn render(entries: &[FileEntry]) -> Vec<String> {
    entries
        .iter()
        .flat_map(|f| {
            f.hunks.iter().map(move |h| {
                format!(
                    "{} {}{}{} {:?}{} {:?}",
                    f.path,
                    f.index_status,
                    f.worktree_status,
                    if f.binary { " binary" } else { "" },
                    h.origin,
                    if h.selected { "+" } else { "" },
                    h.hunk.text,
                )
            })
        })
        .collect()
}

#[test]
fn mixed_working_changes_are_listed() {
    let t = repo_with_mixed_changes();
    let entries = collect_file_entries(&t.repo, &t.workdir(), None).unwrap();
    assert_eq!(
        render(&entries),
        [
            r#"a.txt MM Staged+ "@@ -1,4 +1,4 @@\n-1\n+one\n 2\n 3\n 4\n""#,
            r#"a.txt MM Unstaged "@@ -7,4 +7,4 @@ one\n 7\n 8\n 9\n-10\n+ten\n""#,
            r#"crlf.txt  M Unstaged "@@ -1,2 +1,2 @@\n one\r\n-two\r\n+TWO\r\n""#,
            r#"empty.txt A  Staged+ "(empty file)""#,
            r#"gone.txt D  Staged+ "(file deleted)""#,
            r#"latin1.txt  M Unstaged "@@ -1 +1 @@\n-caf\u{10ffe9}\n+caf\u{10ffe9}!\n""#,
            r#"logo.bin MM binary Staged+ "(binary file)""#,
            r#"logo.bin MM binary Unstaged "(binary file)""#,
            r#"lost.txt  D Unstaged "(file deleted)""#,
            r#"new name.txt A  Staged+ "@@ -0,0 +1,10 @@\n+1\n+2\n+3\n+4\n+5\n+6\n+7\n+8\n+9\n+10\n""#,
            r#"new.txt A  Staged+ "@@ -0,0 +1 @@\n+new\n""#,
            r#"old name.txt D  Staged+ "(file deleted)""#,
            r#"sub  M binary Unstaged "(submodule)""#,
            r#"untracked-empty.txt ?? Unstaged "(empty file)""#,
            r#"untracked.bin ?? binary Unstaged "(binary file)""#,
            r#"untracked.txt ?? Unstaged "@@ -0,0 +1,1 @@\n+u\n""#,
            r#"with space.txt  M Unstaged "@@ -2,7 +2,7 @@\n 2\n 3\n 4\n-5\n+five\n 6\n 7\n 8\n""#,
            r#"x b/y.txt  M Unstaged "@@ -2,7 +2,7 @@\n 2\n 3\n 4\n-5\n+five\n 6\n 7\n 8\n""#,
            r#"é.txt  M Unstaged "@@ -2,7 +2,7 @@\n 2\n 3\n 4\n-5\n+five\n 6\n 7\n 8\n""#,
        ]
    );
}

/// Config that reshapes a whole-tree diff: copy detection pairs files no
/// per-file diff would, and unquoted paths reach the parser raw.
#[test]
fn diff_config_does_not_change_the_listing() {
    let t = repo_with_mixed_changes();
    let workdir = t.workdir();
    let before = collect_file_entries(&t.repo, &workdir, None).unwrap();
    t.set_config("diff.renames", "copies");
    t.set_config("core.quotePath", "false");
    t.set_config("diff.noprefix", "true");
    t.set_config("diff.context", "0");
    t.set_config("color.diff", "always");
    t.set_config("diff.external", "false");

    let after = collect_file_entries(&t.repo, &workdir, None).unwrap();

    assert_eq!(render(&after), render(&before));
}

#[test]
fn a_filtered_listing_is_the_full_one_narrowed() {
    let t = repo_with_mixed_changes();
    let workdir = t.workdir();
    let all = collect_file_entries(&t.repo, &workdir, None).unwrap();
    let paths = ["x b/y.txt", "logo.bin", "é.txt"].map(String::from);

    let some = collect_file_entries(&t.repo, &workdir, Some(&paths)).unwrap();

    let narrowed: Vec<&FileEntry> = all.iter().filter(|f| paths.contains(&f.path)).collect();
    assert_eq!(some.iter().collect::<Vec<_>>(), narrowed);
}

#[test]
fn mixed_changes_in_a_commit_are_listed() {
    let t = repo_with_mixed_changes();
    crate::git::stage_all(&t.workdir()).unwrap();
    t.commit_staged("Everything");

    let entries = collect_commit_hunks(&t.workdir(), "HEAD", &[]).unwrap();

    // An empty file has no hunk to pick in a commit, unlike in the index.
    assert_eq!(
        render(&entries),
        [
            r#"a.txt M  Commit "@@ -1,4 +1,4 @@\n-1\n+one\n 2\n 3\n 4\n""#,
            r#"a.txt M  Commit "@@ -7,4 +7,4 @@\n 7\n 8\n 9\n-10\n+ten\n""#,
            r#"crlf.txt M  Commit "@@ -1,2 +1,2 @@\n one\r\n-two\r\n+TWO\r\n""#,
            r#"gone.txt D  Commit "(file deleted)""#,
            r#"latin1.txt M  Commit "@@ -1 +1 @@\n-caf\u{10ffe9}\n+caf\u{10ffe9}!\n""#,
            r#"logo.bin M  binary Commit "(binary file)""#,
            r#"lost.txt D  Commit "(file deleted)""#,
            r#"new name.txt A  Commit "@@ -0,0 +1,10 @@\n+1\n+2\n+3\n+4\n+5\n+6\n+7\n+8\n+9\n+10\n""#,
            r#"new.txt A  Commit "@@ -0,0 +1 @@\n+new\n""#,
            r#"old name.txt D  Commit "(file deleted)""#,
            r#"sub M  binary Commit "(submodule)""#,
            r#"untracked.bin A  binary Commit "(binary file)""#,
            r#"untracked.txt A  Commit "@@ -0,0 +1 @@\n+u\n""#,
            r#"with space.txt M  Commit "@@ -2,7 +2,7 @@\n 2\n 3\n 4\n-5\n+five\n 6\n 7\n 8\n""#,
            r#"x b/y.txt M  Commit "@@ -2,7 +2,7 @@\n 2\n 3\n 4\n-5\n+five\n 6\n 7\n 8\n""#,
            r#"é.txt M  Commit "@@ -2,7 +2,7 @@\n 2\n 3\n 4\n-5\n+five\n 6\n 7\n 8\n""#,
        ]
    );
}

#[test]
fn a_filtered_commit_listing_is_the_full_one_narrowed() {
    let t = repo_with_mixed_changes();
    let workdir = t.workdir();
    crate::git::stage_all(&workdir).unwrap();
    t.commit_staged("Everything");
    let all = collect_commit_hunks(&workdir, "HEAD", &[]).unwrap();
    let paths = ["x b/y.txt", "logo.bin", "é.txt"].map(String::from);

    let some = collect_commit_hunks(&workdir, "HEAD", &paths).unwrap();

    let narrowed: Vec<&FileEntry> = all.iter().filter(|f| paths.contains(&f.path)).collect();
    assert_eq!(some.iter().collect::<Vec<_>>(), narrowed);
}

/// `c.txt` conflicts. The staged side prints `* Unmerged path c.txt` right
/// after `b.txt`'s hunk, which would read as one more context line of it.
#[test]
fn a_conflicted_path_stays_out_of_its_neighbors_hunks() {
    let t = TestRepo::new();
    let workdir = t.workdir();
    let git = |args: &[&str]| crate::git::run_git(&workdir, args);
    for path in ["a.txt", "b.txt", "c.txt"] {
        t.write_file(path, "base\n");
    }
    t.stage_files(&["a.txt", "b.txt", "c.txt"]);
    t.commit_staged("Base");
    git(&["checkout", "-q", "-b", "other"]).unwrap();
    t.write_file("c.txt", "other\n");
    t.stage_files(&["c.txt"]);
    t.commit_staged("Other");
    git(&["checkout", "-q", "-"]).unwrap();
    t.write_file("c.txt", "mine\n");
    t.stage_files(&["c.txt"]);
    t.commit_staged("Mine");
    assert!(git(&["merge", "-q", "other"]).is_err());
    t.write_file("b.txt", "staged\n");
    t.stage_files(&["b.txt"]);
    t.write_file("a.txt", "unstaged\n");

    let entries = collect_file_entries(&t.repo, &workdir, None).unwrap();

    assert_eq!(
        render(&entries),
        [
            r#"a.txt  M Unstaged "@@ -1 +1 @@\n-base\n+unstaged\n""#,
            r#"b.txt M  Staged+ "@@ -1 +1 @@\n-base\n+staged\n""#,
        ]
    );
}

#[test]
fn a_pick_is_still_listed_until_one_of_its_own_files_changes() {
    let t = repo_with_mixed_changes();
    let workdir = t.workdir();
    let picked: Vec<FileEntry> = collect_file_entries(&t.repo, &workdir, None)
        .unwrap()
        .into_iter()
        .filter(|f| f.path == "a.txt" || f.path == "x b/y.txt")
        .collect();
    let stamp = binary_stamp(&workdir, &picked);

    t.write_file("with space.txt", "elsewhere\n");
    assert!(still_listed(&t.repo, &workdir, &picked, &stamp).unwrap());

    t.write_file("x b/y.txt", "edited\n");
    assert!(!still_listed(&t.repo, &workdir, &picked, &stamp).unwrap());
}

/// A lock another git holds is theirs: the restore misses rather than race it.
#[test]
fn putting_the_index_back_goes_through_its_lock() {
    let dir = tempfile::tempdir().unwrap();
    let index = dir.path().join("index");
    let lock = dir.path().join("index.lock");
    std::fs::write(&index, b"now").unwrap();

    assert!(put_index_back(&index, Some(b"before")));
    assert_eq!(std::fs::read(&index).unwrap(), b"before");
    assert!(!lock.exists());

    std::fs::write(&lock, b"theirs").unwrap();
    assert!(!put_index_back(&index, Some(b"again")));
    assert_eq!(std::fs::read(&index).unwrap(), b"before");
    assert_eq!(std::fs::read(&lock).unwrap(), b"theirs");
    std::fs::remove_file(&lock).unwrap();

    assert!(put_index_back(&index, None));
    assert!(!index.exists() && !lock.exists());
    assert!(
        put_index_back(&index, None),
        "no index to remove is no miss"
    );
}

/// When even the reset fails there is nothing safe to replay onto, so both
/// halves of the snapshot are parked on disk instead of applied blind.
#[test]
fn rollback_to_snapshot_parks_both_patches_when_the_reset_fails() {
    let test_repo = TestRepo::new();
    test_repo.commit("A commit", "file1.txt");
    let workdir = test_repo.workdir();
    let snapshot = super::WorktreeSnapshot {
        worktree: "worktree half".to_string(),
        staged: "staged half".to_string(),
    };

    super::rollback_to_snapshot(
        &workdir,
        "0123456789abcdef0123456789abcdef01234567",
        None,
        &snapshot,
    );

    let loom_dir = crate::git::git_path(&workdir, "loom").unwrap();
    let saved: Vec<String> = std::fs::read_dir(&loom_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert!(
        saved.contains(&"unrestored-0.patch".to_string()),
        "{saved:?}"
    );
    assert!(
        saved.contains(&"unrestored-staged-0.patch".to_string()),
        "{saved:?}"
    );
    assert_eq!(
        std::fs::read_to_string(loom_dir.join("unrestored-0.patch")).unwrap(),
        "worktree half"
    );
    assert_eq!(
        std::fs::read_to_string(loom_dir.join("unrestored-staged-0.patch")).unwrap(),
        "staged half"
    );
}

/// Each half is replayed on its own: a half that fails to apply is parked
/// while the other still lands, so a bad worktree patch cannot take the
/// staged one with it, nor the reverse.
#[test]
fn rollback_to_snapshot_parks_only_the_half_that_fails_to_apply() {
    let test_repo = TestRepo::new();
    test_repo.commit("A commit", "file1.txt");
    let workdir = test_repo.workdir();
    let head = test_repo.head_oid().to_string();
    test_repo.write_file(
        "file1.txt",
        "changed
",
    );
    let good = crate::git::diff_head(&workdir).unwrap();
    test_repo.reset_hard(test_repo.head_oid());
    let loom_dir = crate::git::git_path(&workdir, "loom").unwrap();

    super::rollback_to_snapshot(
        &workdir,
        &head,
        None,
        &super::WorktreeSnapshot {
            worktree: good.clone(),
            staged: "not a patch".to_string(),
        },
    );
    assert_eq!(
        test_repo.read_file("file1.txt"),
        "changed
"
    );
    assert_eq!(crate::git::diff_cached(&workdir).unwrap(), "");
    assert_eq!(
        std::fs::read_to_string(loom_dir.join("unrestored-staged-0.patch")).unwrap(),
        "not a patch"
    );
    assert!(!loom_dir.join("unrestored-0.patch").exists());

    super::rollback_to_snapshot(
        &workdir,
        &head,
        None,
        &super::WorktreeSnapshot {
            worktree: "not a patch".to_string(),
            staged: good.clone(),
        },
    );
    assert_eq!(crate::git::diff_cached(&workdir).unwrap(), good);
    assert_eq!(
        std::fs::read_to_string(loom_dir.join("unrestored-0.patch")).unwrap(),
        "not a patch"
    );
    assert!(!loom_dir.join("unrestored-staged-1.patch").exists());
}

/// The rollback's reset would overwrite the file, and neither patch holds its
/// content: the snapshot refuses it, before the operation touches anything.
#[test]
fn a_snapshot_refuses_a_file_removed_from_the_index_but_kept_on_disk() {
    let test_repo = TestRepo::new();
    test_repo.commit("A commit", "foo.txt");
    let workdir = test_repo.workdir();
    crate::git::run_git(&workdir, &["rm", "-q", "--cached", "foo.txt"]).unwrap();
    test_repo.write_file("foo.txt", "my local edits\n");

    let err = super::WorktreeSnapshot::take(&workdir)
        .err()
        .expect("refused")
        .to_string();

    assert!(err.contains("`foo.txt`"), "{err}");
}

/// Two names that are not UTF-8 and read as the same lossy one, which no git
/// command takes back: neither is listed.
#[test]
fn a_path_that_is_not_utf8_is_not_listed() {
    let t = TestRepo::new();
    let repo = &t.repo;
    let tree_of = |content: &[u8]| {
        let blob = repo.blob(content).unwrap();
        let mut builder = repo.treebuilder(None).unwrap();
        for name in [&b"caf\xe9.txt"[..], b"caf\xe8.txt", b"ok.txt"] {
            builder.insert(name, blob, 0o100644).unwrap();
        }
        repo.find_tree(builder.write().unwrap()).unwrap()
    };
    let sig = git2::Signature::now("Test", "test@test.com").unwrap();
    let base = repo
        .commit(
            None,
            &sig,
            &sig,
            "Base",
            &tree_of(b"one\n"),
            &[&t.head_commit()],
        )
        .unwrap();
    let base = repo.find_commit(base).unwrap();
    let edit = repo
        .commit(None, &sig, &sig, "Edit", &tree_of(b"two\n"), &[&base])
        .unwrap();

    let entries = collect_commit_hunks(&t.workdir(), &edit.to_string(), &[]).unwrap();

    assert_eq!(
        render(&entries),
        [r#"ok.txt M  Commit "@@ -1 +1 @@\n-one\n+two\n""#]
    );
}

/// The `git diff` runs a listing traces.
fn diffs_run(t: &TestRepo, list: impl FnOnce()) -> usize {
    let _ = trace_log::finalize();
    trace_log::init(t.repo.path(), "git loom add -p", false);
    list();
    let log = std::fs::read_to_string(trace_log::finalize().unwrap()).unwrap();
    log.matches("[git] diff ").count()
}

/// What a listing spawns must not grow with the files it lists.
#[test]
fn each_side_is_read_with_two_diffs_whatever_the_file_count() {
    let t = repo_with_mixed_changes();
    let workdir = t.workdir();
    let working = diffs_run(&t, || {
        collect_file_entries(&t.repo, &workdir, None).unwrap();
    });
    crate::git::stage_all(&workdir).unwrap();
    t.commit_staged("Everything");
    let commit = diffs_run(&t, || {
        collect_commit_hunks(&workdir, "HEAD", &[]).unwrap();
    });

    assert_eq!(working, 4, "staged and unstaged sides");
    assert_eq!(commit, 3, "name-status, then the commit's side");
}

#[test]
fn a_side_with_no_file_to_diff_runs_no_git() {
    let t = TestRepo::new();
    t.write_file("untracked.txt", "u\n");
    let workdir = t.workdir();

    let runs = diffs_run(&t, || {
        collect_file_entries(&t.repo, &workdir, None).unwrap();
    });

    assert_eq!(runs, 0);
}
