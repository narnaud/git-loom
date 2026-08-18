use crate::core::repo;
use crate::core::shortid::IdAllocator;
use crate::core::test_helpers::TestRepo;

/// Disable the pager so `git show` doesn't block or pollute test output.
fn no_pager() {
    // SAFETY: tests are serialized via `in_dir`'s global mutex, so no concurrent
    // env mutation occurs.
    unsafe { std::env::set_var("GIT_PAGER", "cat") };
}

/// Feature branches `featA` (A1, A2) and `featB` (B1, B2) stacked on top of it,
/// both merged into `integration`. Returns (a1, a2, b1, b2).
fn stacked_repo(test_repo: &TestRepo) -> (git2::Oid, git2::Oid, git2::Oid, git2::Oid) {
    let base = test_repo.head_oid();

    test_repo.create_branch_at_commit("featA", base);
    test_repo.switch_branch("featA");
    let a1 = test_repo.commit_empty("A1");
    let a2 = test_repo.commit_empty("A2");

    test_repo.create_branch_at_commit("featB", a2);
    test_repo.switch_branch("featB");
    let b1 = test_repo.commit_empty("B1");
    let b2 = test_repo.commit_empty("B2");

    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("featA");
    test_repo.merge_no_ff("featB");

    (a1, a2, b1, b2)
}

fn revs(test_repo: &TestRepo, branch: &str) -> Vec<String> {
    super::branch_revs(&test_repo.repo, branch).unwrap()
}

fn hashes(oids: &[git2::Oid]) -> Vec<String> {
    oids.iter().map(|o| o.to_string()).collect()
}

#[test]
fn show_commit_by_hash() {
    no_pager();
    let test_repo = TestRepo::new();
    let oid = test_repo.commit("Test commit", "file.txt");

    let result = test_repo.in_dir(|| super::run(Some(oid.to_string())));
    assert!(
        result.is_ok(),
        "show should succeed for a valid commit hash"
    );
}

#[test]
fn show_no_target_uses_head() {
    no_pager();
    let test_repo = TestRepo::new();
    test_repo.commit("On main", "file.txt");

    let result = test_repo.in_dir(|| super::run(None));
    assert!(
        result.is_ok(),
        "show with no target should show the last commit"
    );
}

#[test]
fn show_branch_succeeds() {
    no_pager();
    let test_repo = TestRepo::new();
    test_repo.commit("On main", "file.txt");

    let head = test_repo.repo.head().unwrap();
    let branch_name = head.shorthand().unwrap().to_string();

    let result = test_repo.in_dir(|| super::run(Some(branch_name.clone())));
    assert!(result.is_ok(), "show should succeed for a branch name");
}

#[test]
fn show_invalid_target_fails() {
    let test_repo = TestRepo::new();

    let result = test_repo.in_dir(|| super::run(Some("nonexistent_target_xyz".to_string())));
    assert!(result.is_err(), "show should fail for invalid target");
}

#[test]
fn branch_revs_covers_all_commits_in_branch() {
    let test_repo = TestRepo::new_with_remote();
    let base = test_repo.head_oid();

    test_repo.create_branch_at_commit("feature", base);
    test_repo.switch_branch("feature");
    let f1 = test_repo.commit_empty("F1");
    let f2 = test_repo.commit_empty("F2");

    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature");

    assert_eq!(revs(&test_repo, "feature"), hashes(&[f2, f1]));
}

#[test]
fn branch_revs_stops_at_the_branch_below_in_the_stack() {
    let test_repo = TestRepo::new_with_remote();
    let (a1, a2, b1, b2) = stacked_repo(&test_repo);

    assert_eq!(
        revs(&test_repo, "featB"),
        hashes(&[b2, b1]),
        "featB must not claim the commits of featA below it"
    );
    assert_eq!(revs(&test_repo, "featA"), hashes(&[a2, a1]));
}

#[test]
fn branch_revs_on_integration_shows_loose_commits_only() {
    let test_repo = TestRepo::new_with_remote();
    let base = test_repo.head_oid();

    test_repo.create_branch_at_commit("feature", base);
    test_repo.switch_branch("feature");
    test_repo.commit_empty("F1");

    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature");
    let loose = test_repo.commit_empty("Loose");

    assert_eq!(
        revs(&test_repo, "integration"),
        hashes(&[loose]),
        "the integration branch owns neither the feature commits nor the merge"
    );
}

#[test]
fn branch_revs_falls_back_to_tip_for_unrelated_history() {
    let test_repo = TestRepo::new_with_remote();

    // A root commit on its own branch: no merge base with the integration line.
    let sig = test_repo.repo.signature().unwrap();
    let tree = {
        let mut index = test_repo.repo.index().unwrap();
        let tree_id = index.write_tree().unwrap();
        test_repo.repo.find_tree(tree_id).unwrap()
    };
    test_repo
        .repo
        .commit(Some("refs/heads/orphan"), &sig, &sig, "Orphan", &tree, &[])
        .unwrap();

    assert_eq!(
        revs(&test_repo, "orphan"),
        vec!["orphan"],
        "a branch outside the stack must not expand to its whole history"
    );
}

#[test]
fn branch_revs_fails_when_branch_has_no_commits() {
    let test_repo = TestRepo::new_with_remote();
    let base = test_repo.head_oid();
    test_repo.create_branch_at_commit("empty", base);

    let err = super::branch_revs(&test_repo.repo, "empty").unwrap_err();
    assert!(
        err.to_string().contains("no commits of its own"),
        "unexpected error: {err}"
    );
}

#[test]
fn branch_revs_falls_back_to_tip_without_upstream() {
    let test_repo = TestRepo::new();
    test_repo.commit("On main", "file.txt");

    let head = test_repo.repo.head().unwrap();
    let branch_name = head.shorthand().unwrap().to_string();

    assert_eq!(revs(&test_repo, &branch_name), vec![branch_name.clone()]);
}

#[test]
fn branch_revs_falls_back_to_tip_on_detached_head() {
    let test_repo = TestRepo::new_with_remote();
    let (_a1, _a2, _b1, _b2) = stacked_repo(&test_repo);
    test_repo.set_detached_head(test_repo.head_oid());

    assert_eq!(revs(&test_repo, "featB"), vec!["featB"]);
}

#[test]
fn show_revs_resolves_a_branch_short_id() {
    let test_repo = TestRepo::new_with_remote();
    let (_a1, _a2, b1, b2) = stacked_repo(&test_repo);

    let info = repo::gather_repo_info(&test_repo.repo, false, 1).unwrap();
    let short_id = IdAllocator::new(info.collect_entities())
        .get_branch("featB")
        .to_string();

    let resolved = test_repo
        .in_dir(|| super::show_revs(&test_repo.repo, Some(short_id)))
        .unwrap();
    assert_eq!(resolved, hashes(&[b2, b1]));
}

#[test]
fn show_revs_passes_a_commit_through() {
    let test_repo = TestRepo::new_with_remote();
    let oid = test_repo.commit("Test commit", "file.txt");

    let resolved = test_repo
        .in_dir(|| super::show_revs(&test_repo.repo, Some(oid.to_string())))
        .unwrap();
    assert_eq!(resolved, vec![oid.to_string()]);
}

#[test]
fn branch_revs_on_integration_skips_hidden_branches() {
    let test_repo = TestRepo::new_with_remote();
    let base = test_repo.head_oid();

    test_repo.create_branch_at_commit("local-scratch", base);
    test_repo.switch_branch("local-scratch");
    test_repo.commit_empty("Scratch");

    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("local-scratch");
    let loose = test_repo.commit_empty("Loose");

    assert_eq!(
        revs(&test_repo, "integration"),
        hashes(&[loose]),
        "commits of a hidden branch stay hidden on the integration line"
    );
}

#[test]
fn branch_revs_shows_a_hidden_branch_when_named() {
    let test_repo = TestRepo::new_with_remote();
    let base = test_repo.head_oid();

    test_repo.create_branch_at_commit("local-scratch", base);
    test_repo.switch_branch("local-scratch");
    let scratch = test_repo.commit_empty("Scratch");

    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("local-scratch");

    assert_eq!(
        revs(&test_repo, "local-scratch"),
        hashes(&[scratch]),
        "naming a hidden branch shows it, unlike status"
    );
}

#[test]
fn branch_revs_handles_co_located_branches() {
    let test_repo = TestRepo::new_with_remote();
    let base = test_repo.head_oid();

    test_repo.create_branch_at_commit("feature", base);
    test_repo.switch_branch("feature");
    let f1 = test_repo.commit_empty("F1");
    let f2 = test_repo.commit_empty("F2");
    test_repo.create_branch_at_commit("alias", f2);

    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature");

    let expected = hashes(&[f2, f1]);
    assert_eq!(revs(&test_repo, "feature"), expected);
    assert_eq!(
        revs(&test_repo, "alias"),
        expected,
        "both names at the same tip own the same commits"
    );
}
