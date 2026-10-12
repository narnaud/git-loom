use crate::core::repo;
use crate::core::test_helpers::TestRepo;

/// Unmerging a woven branch removes it from integration but keeps the ref.
#[test]
fn unmerge_removes_branch_from_integration() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");
    test_repo.commit("A2", "a2.txt");
    let a2_oid = test_repo.head_oid();

    // Create and weave feature-a
    test_repo
        .in_dir(|| crate::branch::run(Some("feature-a".to_string()), Some(a2_oid.to_string())))
        .unwrap();

    // Verify it's woven (HEAD is a merge commit)
    assert_eq!(test_repo.head_commit().parent_count(), 2);

    // Unmerge it
    test_repo
        .in_dir(|| super::run(Some("feature-a".to_string())))
        .unwrap();

    // Branch ref should still exist
    assert!(test_repo.branch_exists("feature-a"));

    // feature-a should not be in the woven branches list
    let info = repo::gather_repo_info(&test_repo.repo, false, 1).unwrap();
    let branch_names: Vec<&str> = info.branches.iter().map(|b| b.name.as_str()).collect();
    assert!(
        !branch_names.contains(&"feature-a"),
        "feature-a should not be in integration branches after unmerge, got: {:?}",
        branch_names
    );
}

/// When replaying integration commits on top of the merge-base after removing a
/// woven branch, a conflict can occur if those commits depended on the branch's
/// content.  The rebase must be aborted automatically and the error propagated.
///
/// Conflict setup: feature-a creates `shared.txt`; integration commit B modifies
/// it.  Unmerging feature-a requires replaying B on the merge-base (which has no
/// `shared.txt`) → delete/modify conflict.
#[test]
fn unmerge_conflict_aborts() {
    let test_repo = TestRepo::new_with_remote();

    // feature-a creates shared.txt
    let a_oid = test_repo.commit("from-a", "shared.txt");
    test_repo
        .in_dir(|| crate::branch::run(Some("feature-a".to_string()), Some(a_oid.to_string())))
        .unwrap();

    // Integration commit B modifies shared.txt — its diff expects "from-a" as
    // context, which will be absent when replayed on the merge-base.
    test_repo.write_file("shared.txt", "from-b");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Commit B");

    let result = test_repo.in_dir(|| super::run(Some("feature-a".to_string())));

    assert!(
        result.is_err(),
        "unmerge should fail when the rebase conflicts"
    );

    // The rebase must be fully aborted — no stale git rebase state.
    assert!(
        !crate::git::rebase_is_in_progress(test_repo.repo.path()),
        "rebase should be aborted, not left paused"
    );
    // No loom state file should be left behind.
    assert!(
        !test_repo
            .repo
            .path()
            .join("loom")
            .join("state.json")
            .exists(),
        "no loom state should be written for a non-resumable command"
    );
}

/// Two refs at one woven tip share a section, which takes both out: the
/// question and the message used to name one.
#[test]
fn unmerge_names_the_co_located_branches_leaving_with_it() {
    use crate::core::ui::{self, Answer, PromptKind, Request};

    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");
    let a2_oid = test_repo.commit("A2", "a2.txt");
    test_repo
        .in_dir(|| crate::branch::run(Some("feature-a".to_string()), Some(a2_oid.to_string())))
        .unwrap();
    let tip = test_repo
        .repo
        .find_branch("feature-a", git2::BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();
    test_repo.create_branch_at("feature-b", &tip.to_string());

    // Under `ui`, a branch passed in is confirmed: the question names both.
    let (question, success) = test_repo.in_dir(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            ui::install(tx);
            let result = super::run(Some("feature-b".to_string()));
            ui::uninstall();
            result
        });
        let (mut question, mut success) = (String::new(), String::new());
        for request in rx {
            match request {
                Request::Prompt {
                    kind: PromptKind::Confirm,
                    prompt,
                    reply,
                    ..
                } => {
                    question = prompt;
                    reply.send(Some(Answer::Bool(true))).unwrap();
                }
                Request::Message { text, .. } => success = text,
                _ => {}
            }
        }
        worker.join().unwrap().unwrap();
        (question, success)
    });

    assert_eq!(
        question,
        "Unmerge `feature-b` and `feature-a`?\n\
         They are at the same commit, so they leave together\n\
         Their commits leave the integration branch; the branches keep them, so merging them again brings them back"
    );
    assert_eq!(
        success,
        "Unmerged `feature-b` and `feature-a` from integration branch"
    );
    assert!(test_repo.branch_exists("feature-a"));
    assert!(test_repo.branch_exists("feature-b"));
    let info = repo::gather_repo_info(&test_repo.repo, false, 1).unwrap();
    assert!(info.branches.is_empty());
}

/// Picking `feature-a` in the menu says nothing of `feature-b`, which leaves
/// with it.
#[test]
fn a_picked_branch_taking_others_with_it_is_confirmed() {
    use crate::core::ui::{self, Answer, PromptKind, Request};

    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");
    let a2_oid = test_repo.commit("A2", "a2.txt");
    test_repo
        .in_dir(|| crate::branch::run(Some("feature-a".to_string()), Some(a2_oid.to_string())))
        .unwrap();
    let tip = test_repo
        .repo
        .find_branch("feature-a", git2::BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();
    test_repo.create_branch_at("feature-b", &tip.to_string());

    let kinds = test_repo.in_dir(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            ui::install(tx);
            let result = super::run(None);
            ui::uninstall();
            result
        });
        let mut kinds = Vec::new();
        for request in rx {
            if let Request::Prompt { kind, reply, .. } = request {
                let answer = match kind {
                    PromptKind::Confirm => Answer::Bool(true),
                    _ => Answer::Text("feature-a".into()),
                };
                kinds.push(kind);
                reply.send(Some(answer)).unwrap();
            }
        }
        worker.join().unwrap().unwrap();
        kinds
    });

    assert!(
        matches!(
            kinds.as_slice(),
            [PromptKind::Select { .. }, PromptKind::Confirm]
        ),
        "{kinds:?}"
    );
    let info = repo::gather_repo_info(&test_repo.repo, false, 1).unwrap();
    assert!(info.branches.is_empty());
}

#[test]
fn name_list_joins_with_and() {
    let names = |n: &[&str]| super::name_list(&n.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(names(&["a"]), "`a`");
    assert_eq!(names(&["a", "b"]), "`a` and `b`");
    assert_eq!(names(&["a", "b", "c"]), "`a`, `b` and `c`");
}

/// `lo`'s commits are `hi`'s too: they can only leave once `hi` has. `lo2`
/// shares `lo`'s tip, which must not read as co-located with it.
#[test]
fn unmerge_refuses_a_stacked_branch_until_its_outer_one_is_out() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.create_branch_at("hi", &base_oid.to_string());
    test_repo.switch_branch("hi");
    let lo_tip = test_repo.commit("L1", "l1.txt");
    test_repo.commit("H1", "h1.txt");
    test_repo.switch_branch("integration");
    test_repo.create_branch_at("lo", &lo_tip.to_string());
    test_repo.create_branch_at("lo2", &lo_tip.to_string());
    test_repo
        .in_dir(|| crate::merge::run(Some("hi".to_string()), false))
        .unwrap();

    for name in ["lo", "lo2"] {
        let err = test_repo
            .in_dir(|| super::run(Some(name.to_string())))
            .unwrap_err();
        assert!(err.to_string().contains("stacked under `hi`"), "{err}");
    }

    test_repo
        .in_dir(|| super::run(Some("hi".to_string())))
        .unwrap();
    let info = repo::gather_repo_info(&test_repo.repo, false, 1).unwrap();
    let mut names: Vec<&str> = info.branches.iter().map(|b| b.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["lo", "lo2"]);
}

#[test]
fn the_picker_leaves_out_stacked_branches() {
    use crate::core::ui::{self, PromptKind, Request};

    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.create_branch_at("hi", &base_oid.to_string());
    test_repo.switch_branch("hi");
    let lo_tip = test_repo.commit("L1", "l1.txt");
    test_repo.commit("H1", "h1.txt");
    test_repo.switch_branch("integration");
    test_repo.create_branch_at("lo", &lo_tip.to_string());
    test_repo
        .in_dir(|| crate::merge::run(Some("hi".to_string()), false))
        .unwrap();

    let kinds = test_repo.in_dir(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            ui::install(tx);
            let result = super::run(None);
            ui::uninstall();
            result
        });
        let mut kinds = Vec::new();
        for request in rx {
            if let Request::Prompt { kind, reply, .. } = request {
                kinds.push(kind);
                reply.send(None).unwrap();
            }
        }
        assert!(worker.join().unwrap().is_err(), "cancelled");
        kinds
    });

    assert_eq!(
        kinds,
        [PromptKind::Select {
            items: vec!["hi".to_string()],
            allow_other: false
        }]
    );
}

/// Unmerging a branch that isn't woven should error.
#[test]
fn unmerge_non_woven_branch_errors() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");

    // Create a branch at merge-base (not woven, just a ref)
    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.create_branch_at("feature-a", &base_oid.to_string());

    let result = test_repo.in_dir(|| super::run(Some("feature-a".to_string())));

    assert!(result.is_err());
}

/// Picking in unmerge's own menu is the choice: a pick that takes no other
/// branch with it is not confirmed again.
#[test]
fn a_branch_picked_in_the_menu_is_not_confirmed_again() {
    use crate::core::ui::{self, Answer, PromptKind, Request};

    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");
    let a2_oid = test_repo.commit("A2", "a2.txt");
    test_repo
        .in_dir(|| crate::branch::run(Some("feature-a".to_string()), Some(a2_oid.to_string())))
        .unwrap();

    let kinds = test_repo.in_dir(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            ui::install(tx);
            let result = super::run(None);
            ui::uninstall();
            result
        });
        let mut kinds = Vec::new();
        for request in rx {
            if let Request::Prompt { kind, reply, .. } = request {
                kinds.push(kind);
                reply.send(Some(Answer::Text("feature-a".into()))).unwrap();
            }
        }
        worker.join().unwrap().unwrap();
        kinds
    });

    assert!(
        matches!(kinds.as_slice(), [PromptKind::Select { .. }]),
        "{kinds:?}"
    );
    let info = repo::gather_repo_info(&test_repo.repo, false, 1).unwrap();
    assert!(info.branches.is_empty());
}
