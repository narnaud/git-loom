use crate::core::test_helpers::TestRepo;

// ── Integration tests ────────────────────────────────────────────────

#[test]
fn absorb_single_file() {
    let test_repo = TestRepo::new_with_remote();

    test_repo.commit("Add file1", "file1.txt");

    test_repo.write_file("file1.txt", "modified content");

    test_repo.in_dir(|| {
        let result = super::run(false, vec![]);
        assert!(result.is_ok(), "absorb failed: {:?}", result);

        test_repo.assert_working_tree_clean();

        assert_eq!(test_repo.get_message(0), "Add file1");
    });
}

#[test]
fn absorb_multiple_files_different_commits() {
    let test_repo = TestRepo::new_with_remote();

    test_repo.commit("Add file1", "file1.txt");
    test_repo.commit("Add file2", "file2.txt");

    test_repo.write_file("file1.txt", "modified file1");
    test_repo.write_file("file2.txt", "modified file2");

    test_repo.in_dir(|| {
        let result = super::run(false, vec![]);
        assert!(result.is_ok(), "absorb failed: {:?}", result);

        test_repo.assert_working_tree_clean();

        assert_eq!(test_repo.get_message(0), "Add file2");
        assert_eq!(test_repo.get_message(1), "Add file1");
    });
}

#[test]
fn absorb_skips_new_file() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Add file1", "file1.txt");

    test_repo.write_file("file1.txt", "modified");
    test_repo.write_file("new_file.txt", "brand new");

    test_repo.in_dir(|| {
        // diff HEAD --name-only only shows tracked changes, so new_file.txt
        // won't even be in the list unless user passes it explicitly.
        // Let's pass both explicitly to test the skip path.
        let result = super::run(
            false,
            vec!["file1.txt".to_string(), "new_file.txt".to_string()],
        );
        assert!(result.is_ok(), "absorb failed: {:?}", result);

        assert_eq!(test_repo.read_file("new_file.txt"), "brand new");
    });
}

#[test]
fn absorb_skips_pure_addition() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.write_file("file1.txt", "line1\nline2\n");
    test_repo.stage_files(&["file1.txt"]);
    test_repo.commit_staged("Add file1");

    test_repo.write_file("file1.txt", "line1\nline2\nnew line3\n");

    test_repo.in_dir(|| {
        let result = super::run(true, vec![]);
        // Should succeed as dry-run but skip the file (pure addition)
        // The error "No files could be absorbed" is expected when all are skipped
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("No files could be absorbed"),
            "Expected 'No files could be absorbed' error, got: {}",
            err_msg
        );
    });
}

#[test]
fn absorb_dry_run() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Add file1", "file1.txt");

    let original_head = test_repo.head_oid();
    test_repo.write_file("file1.txt", "modified content");

    test_repo.in_dir(|| {
        let result = super::run(true, vec![]);
        assert!(result.is_ok(), "dry-run absorb failed: {:?}", result);

        // HEAD should NOT have changed (dry-run)
        assert_eq!(
            test_repo.head_oid(),
            original_head,
            "dry-run should not modify HEAD"
        );

        assert_eq!(test_repo.read_file("file1.txt"), "modified content");
    });
}

#[test]
fn absorb_with_file_filter() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Add file1", "file1.txt");
    test_repo.commit("Add file2", "file2.txt");

    test_repo.write_file("file1.txt", "modified file1");
    test_repo.write_file("file2.txt", "modified file2");

    test_repo.in_dir(|| {
        let result = super::run(false, vec!["file1.txt".to_string()]);
        assert!(result.is_ok(), "absorb failed: {:?}", result);

        assert_eq!(
            test_repo.read_file("file2.txt"),
            "modified file2",
            "file2 should still have working tree changes"
        );
    });
}

#[test]
fn absorb_no_changes_error() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Add file1", "file1.txt");

    test_repo.in_dir(|| {
        let result = super::run(false, vec![]);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Nothing to absorb"),
            "Expected 'Nothing to absorb' error, got: {}",
            err
        );
    });
}

#[test]
fn absorb_preserves_skipped_changes() {
    let test_repo = TestRepo::new_with_remote();

    test_repo.commit("Add tracked", "tracked.txt");

    test_repo.write_file("tracked.txt", "modified tracked");
    test_repo.write_file("untracked.txt", "new content");

    test_repo.in_dir(|| {
        let result = super::run(
            false,
            vec!["tracked.txt".to_string(), "untracked.txt".to_string()],
        );
        assert!(result.is_ok(), "absorb failed: {:?}", result);

        assert_eq!(test_repo.read_file("untracked.txt"), "new content");
    });
}

#[test]
fn absorb_skips_multiple_sources() {
    let test_repo = TestRepo::new_with_remote();
    // Create two commits each introducing content in the same file.
    // Use manual staging to control exact file content per commit.
    test_repo.write_file("shared.txt", "line1 from c1\n");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Commit 1");

    test_repo.write_file("shared.txt", "line1 from c1\nline2 from c2\n");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Commit 2");

    // Modify lines from both commits
    test_repo.write_file("shared.txt", "MODIFIED line1\nMODIFIED line2\n");

    test_repo.in_dir(|| {
        let result = super::run(true, vec![]);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("No files could be absorbed"),
            "Expected 'No files could be absorbed', got: {}",
            err_msg
        );
    });
}

#[test]
fn absorb_skips_out_of_scope() {
    let test_repo = TestRepo::new_with_remote();

    test_repo.commit("In-scope commit", "in_scope.txt");

    test_repo.write_file("in_scope.txt", "modified");

    test_repo.in_dir(|| {
        let result = super::run(false, vec![]);
        assert!(result.is_ok(), "absorb should succeed for in-scope file");
    });
}

#[test]
fn absorb_with_woven_branches() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("feat1", &base_oid.to_string());
    test_repo.switch_branch("feat1");
    test_repo.write_file("feature.txt", "initial feature content");
    test_repo.stage_files(&["feature.txt"]);
    test_repo.commit_staged("Feature 1");

    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feat1");

    test_repo.write_file("feature.txt", "updated feature content");

    test_repo.in_dir(|| {
        let result = super::run(false, vec![]);
        assert!(
            result.is_ok(),
            "absorb with woven branches failed: {:?}",
            result
        );

        test_repo.assert_working_tree_clean();
    });
}

#[test]
fn absorb_split_hunks_to_different_commits() {
    let test_repo = TestRepo::new_with_remote();

    test_repo.write_file(
        "shared.txt",
        "line1 from c1\nline2 from c1\nline3 from c1\n\
         pad1\npad2\npad3\npad4\npad5\npad6\npad7\npad8\n",
    );
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Commit 1");

    test_repo.write_file(
        "shared.txt",
        "line1 from c1\nline2 from c1\nline3 from c1\n\
         pad1\npad2\npad3\npad4\npad5\npad6\npad7\npad8\n\
         line12 from c2\nline13 from c2\nline14 from c2\n",
    );
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Commit 2");

    test_repo.write_file(
        "shared.txt",
        "MODIFIED line1\nline2 from c1\nline3 from c1\n\
         pad1\npad2\npad3\npad4\npad5\npad6\npad7\npad8\n\
         MODIFIED line12\nline13 from c2\nline14 from c2\n",
    );

    test_repo.in_dir(|| {
        let result = super::run(false, vec![]);
        assert!(
            result.is_ok(),
            "hunk-level absorb should succeed: {:?}",
            result
        );

        test_repo.assert_working_tree_clean();

        assert_eq!(test_repo.get_message(0), "Commit 2");
        assert_eq!(test_repo.get_message(1), "Commit 1");
    });
}

#[test]
fn absorb_split_with_pure_addition_hunk() {
    let test_repo = TestRepo::new_with_remote();

    test_repo.write_file("file.txt", "line1\nline2\nline3\n");
    test_repo.stage_files(&["file.txt"]);
    test_repo.commit_staged("Add file");

    test_repo.write_file(
        "file.txt",
        "MODIFIED line1\nline2\nline3\nnew line4\nnew line5\n",
    );

    test_repo.in_dir(|| {
        let result = super::run(false, vec![]);
        assert!(
            result.is_ok(),
            "mixed split absorb should succeed: {:?}",
            result
        );

        let content = test_repo.read_file("file.txt");
        assert!(
            content.contains("new line4"),
            "pure addition hunk should remain in working tree"
        );
    });
}

#[test]
fn absorb_skipped_patch_only_contains_skipped_files() {
    let test_repo = TestRepo::new_with_remote();

    test_repo.write_file("absorbable.txt", "original content\n");
    test_repo.stage_files(&["absorbable.txt"]);
    test_repo.commit_staged("Add absorbable");

    test_repo.write_file("skippable.txt", "line1\nline2\n");
    test_repo.stage_files(&["skippable.txt"]);
    test_repo.commit_staged("Add skippable");

    test_repo.write_file("absorbable.txt", "modified content\n");

    test_repo.write_file("skippable.txt", "line1\nline2\nnew line3\n");

    test_repo.in_dir(|| {
        let result = super::run(
            false,
            vec!["absorbable.txt".to_string(), "skippable.txt".to_string()],
        );
        assert!(result.is_ok(), "absorb failed: {:?}", result);

        let abs_diff =
            crate::git::diff_head_file(&std::path::PathBuf::from("."), "absorbable.txt").unwrap();
        assert!(
            abs_diff.is_empty(),
            "absorbable.txt should be clean after absorb, but has diff:\n{}",
            abs_diff
        );

        let content = test_repo.read_file("skippable.txt");
        assert!(
            content.contains("new line3"),
            "skippable.txt should retain its skipped changes"
        );
    });
}

#[test]
fn absorb_file_with_sql_comment_lines() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.write_file("query.sql", "SELECT *\n-- main query\nFROM users\n");
    test_repo.stage_files(&["query.sql"]);
    test_repo.commit_staged("Add SQL query");

    test_repo.write_file("query.sql", "SELECT *\n-- updated query\nFROM users\n");

    test_repo.in_dir(|| {
        let result = super::run(false, vec![]);
        assert!(
            result.is_ok(),
            "absorb should handle -- lines: {:?}",
            result.err()
        );
        test_repo.assert_working_tree_clean();
        let content = test_repo.read_file("query.sql");
        assert!(
            content.contains("-- updated query"),
            "absorbed content should contain the updated SQL comment"
        );
    });
}

#[test]
fn absorb_staged_only_changes() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Add file", "target.txt");

    test_repo.write_file("target.txt", "staged content\n");
    test_repo.stage_files(&["target.txt"]);

    test_repo.in_dir(|| {
        let result = super::run(false, vec![]);
        assert!(
            result.is_ok(),
            "absorb should handle staged-only changes: {:?}",
            result.err()
        );
        test_repo.assert_working_tree_clean();
    });
}

/// Regression: loom abort for absorb must restore the pre-absorb HEAD
/// (`reset_hard_to`) and re-apply saved staged/worktree patches.
///
/// Absorb creates fixup commits before the rebase, advancing HEAD. If the
/// rebase is aborted, `git rebase --abort` only restores HEAD to the
/// post-fixup state, not the original pre-absorb HEAD. The `reset_hard_to`
/// rollback field corrects this.
///
/// This test simulates absorb's pre-rebase state directly (saves patches,
/// clears working changes, creates a fixup commit, injects LoomState) and
/// then calls abort_cmd to verify the full rollback path.
#[test]
fn absorb_abort_preserves_working_state() {
    let test_repo = TestRepo::new_with_remote();

    test_repo.write_file("feature.txt", "original\n");
    test_repo.write_file("other-staged.txt", "original-staged\n");
    test_repo.write_file("other-unstaged.txt", "original-unstaged\n");
    test_repo.stage_files(&["feature.txt", "other-staged.txt", "other-unstaged.txt"]);
    test_repo.commit_staged("Initial setup");

    let pre_absorb_oid = test_repo.head_oid();

    test_repo.write_file("other-staged.txt", "staged-content");
    test_repo.stage_files(&["other-staged.txt"]);
    test_repo.write_file("other-unstaged.txt", "unstaged-content");
    test_repo.write_file("new-file.txt", "new-content"); // new untracked file

    test_repo.write_file("feature.txt", "modified\n");

    let workdir = test_repo.workdir();
    let git_dir = test_repo.repo.path().to_path_buf();

    let saved_staged = crate::git::diff_cached_files(&workdir, &["other-staged.txt"]).unwrap();
    crate::git::run_git(&workdir, &["restore", "--staged", "."]).unwrap();
    let saved_worktree = crate::git::diff_head(&workdir).unwrap();
    crate::git::run_git(&workdir, &["restore", "--staged", "--worktree", "."]).unwrap();

    test_repo.write_file("feature.txt", "modified\n");
    test_repo.stage_files(&["feature.txt"]);
    test_repo.commit_staged("fixup! Initial setup");

    let state = crate::core::transaction::LoomState {
        command: "absorb".to_string(),
        rollback: crate::core::transaction::Rollback {
            reset_hard_to: pre_absorb_oid.to_string(),
            saved_staged_patch: saved_staged,
            saved_worktree_patch: saved_worktree,
            ..Default::default()
        },
        context: serde_json::json!({ "dry_run": false }),
        protect: Vec::new(),
        targets: Vec::new(),
    };
    crate::core::transaction::save(&git_dir, &state).unwrap();

    crate::core::transaction::abort_cmd(&workdir, &git_dir).unwrap();

    assert_eq!(
        test_repo.head_oid(),
        pre_absorb_oid,
        "HEAD must be restored to pre-absorb state"
    );

    assert_eq!(test_repo.read_file("other-staged.txt"), "staged-content");
    assert_eq!(
        test_repo.read_file("other-unstaged.txt"),
        "unstaged-content"
    );
    assert!(
        workdir.join("new-file.txt").exists(),
        "new untracked file must survive abort"
    );
    assert_eq!(test_repo.read_file("new-file.txt"), "new-content");
}

/// Absorb makes its `fixup!` commits before the rebase, so a rebase that
/// refuses to start — a branch it would move is checked out in another
/// worktree — still has them to take back, along with the working tree they
/// consumed. The state file holding that undo goes here, so nothing later can.
#[test]
fn absorb_rolls_back_when_the_rebase_refuses_to_start() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    let base = test_repo
        .find_remote_branch_target("origin/main")
        .to_string();

    test_repo.create_branch_at("feature", &base);
    test_repo.switch_branch("feature");
    test_repo.commit("A1", "a1.txt");
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature");

    let wt = workdir.parent().unwrap().join("wt");
    crate::git::run_git(
        &workdir,
        &["worktree", "add", wt.to_str().unwrap(), "feature"],
    )
    .unwrap();

    let head_before = test_repo.head_oid();
    test_repo.write_file(
        "a1.txt",
        "the change to absorb
",
    );
    let worktree_before = test_repo.read_file("a1.txt");

    let result = test_repo.in_dir(|| super::run(false, vec![]));

    assert!(result.is_err(), "the rebase cannot start, so absorb fails");
    assert_eq!(
        test_repo.head_oid(),
        head_before,
        "the `fixup!` commits must be gone"
    );
    assert_eq!(
        test_repo.read_file("a1.txt"),
        worktree_before,
        "the change absorb took out of the working tree comes back"
    );
    assert!(
        !test_repo.repo.path().join("loom/state.json").exists(),
        "the state file goes with the rollback"
    );
}

// ── TUI confirmation ─────────────────────────────────────────────────────

/// Runs `absorb` with a TUI sink installed, answers its one prompt with
/// `yes`, and returns the prompt text with the command's result.
fn run_in_tui(test_repo: &TestRepo, yes: bool) -> (String, anyhow::Result<()>) {
    use crate::core::ui;
    test_repo.in_dir(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            ui::install(tx);
            let result = super::run(false, vec![]);
            ui::uninstall();
            result
        });
        let prompt = loop {
            match rx.recv().unwrap() {
                ui::Request::Prompt { prompt, reply, .. } => {
                    reply.send(Some(ui::Answer::Bool(yes))).unwrap();
                    break prompt;
                }
                _ => continue,
            }
        };
        (prompt, worker.join().unwrap())
    })
}

#[test]
fn tui_absorb_shows_the_plan_and_declining_touches_nothing() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Add file1", "file1.txt");
    let original_head = test_repo.head_oid();
    test_repo.write_file("file1.txt", "modified content");

    let (prompt, result) = run_in_tui(&test_repo, false);

    let (question, detail) = prompt.split_once('\n').unwrap();
    assert_eq!(
        question,
        "Absorb 1 hunk(s) from 1 file(s) into 1 commit(s)?"
    );
    assert!(
        detail.starts_with("file1.txt -> ") && detail.contains("\"Add file1\""),
        "plan: {detail}"
    );
    let err = result.unwrap_err();
    assert!(err.downcast_ref::<crate::core::ui::Cancelled>().is_some());
    assert_eq!(test_repo.head_oid(), original_head);
    assert_eq!(test_repo.read_file("file1.txt"), "modified content");
}

#[test]
fn tui_absorb_runs_once_confirmed() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Add file1", "file1.txt");
    test_repo.write_file("file1.txt", "modified content");

    let (_, result) = run_in_tui(&test_repo, true);

    assert!(result.is_ok(), "absorb failed: {:?}", result);
    test_repo.assert_working_tree_clean();
    assert_eq!(test_repo.get_message(0), "Add file1");
}

/// With no stdout to print on, the skip reasons travel in the error.
#[test]
fn tui_absorb_with_nothing_to_absorb_says_why_without_asking() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.write_file("file1.txt", "line1\n");
    test_repo.stage_files(&["file1.txt"]);
    test_repo.commit_staged("Add file1");
    test_repo.write_file("file1.txt", "line1\nline2\n");

    let result = test_repo.in_dir(|| {
        let (tx, _rx) = std::sync::mpsc::channel();
        crate::core::ui::install(tx);
        let result = super::run(false, vec![]);
        crate::core::ui::uninstall();
        result
    });

    assert_eq!(
        result.unwrap_err().to_string(),
        "No files could be absorbed\nfile1.txt -- skipped (pure addition)"
    );
}
