use crate::core::repo::gather_repo_info;
use crate::core::shortid::IdAllocator;
use crate::core::test_helpers::{TestRepo, hash_shaped_ids};

use super::resolve_commit_filter;

// ── resolve_commit_filter tests ─────────────────────────────────────────────

#[test]
fn filter_by_full_git_hash_shows_only_that_commit() {
    let test_repo = TestRepo::new_with_remote();
    let c1_oid = test_repo.commit("C1", "file1.txt");
    test_repo.commit("C2", "file2.txt");

    let info = gather_repo_info(&test_repo.repo, true, 1).unwrap();
    assert_eq!(info.commits.len(), 2);
    assert!(info.commits.iter().all(|c| !c.files.is_empty()));

    let allocator = IdAllocator::new(info.collect_entities());
    let filter = resolve_commit_filter(&test_repo.repo, &[c1_oid.to_string()], &info, &allocator);
    assert!(filter.contains(&c1_oid));
    assert_eq!(filter.len(), 1);
}

#[test]
fn filter_by_loom_short_id_shows_only_that_commit() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("C1", "file1.txt");
    let c2_oid = test_repo.commit("C2", "file2.txt");

    let info = gather_repo_info(&test_repo.repo, true, 1).unwrap();
    let allocator = IdAllocator::new(info.collect_entities());
    let c2_sid = allocator.get_commit(c2_oid).to_string();

    let filter = resolve_commit_filter(&test_repo.repo, &[c2_sid], &info, &allocator);
    assert!(filter.contains(&c2_oid));
    assert_eq!(filter.len(), 1);
}

#[test]
fn filter_unknown_id_silently_skipped() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("C1", "file1.txt");

    let info = gather_repo_info(&test_repo.repo, true, 1).unwrap();
    let allocator = IdAllocator::new(info.collect_entities());
    let filter = resolve_commit_filter(
        &test_repo.repo,
        &["nonexistent_id".to_string()],
        &info,
        &allocator,
    );
    assert!(filter.is_empty());
}

/// The point of persistent IDs: a rebase changes the hash, not the ID.
#[test]
fn commit_id_survives_an_update_rebase() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.write_file("a.txt", "a");
    test_repo
        .in_dir(|| {
            crate::commit::run(
                Some("feature-a".to_string()),
                false,
                Some("Add a".to_string()),
                None,
                vec!["a.txt".to_string()],
                vec![],
                &crate::core::graph::Theme::dark(),
            )
        })
        .unwrap();
    let tip_before = test_repo.get_branch_target("feature-a");
    let id_before = {
        let info = gather_repo_info(&test_repo.repo, false, 1).unwrap();
        IdAllocator::new(info.collect_entities())
            .persistent_commit(tip_before)
            .expect("a persistent ID")
            .to_string()
    };
    assert_eq!(id_before.len(), 3, "{id_before}");

    test_repo.add_remote_commits(&["Upstream change"]);
    test_repo.in_dir(|| crate::update::run(None, true)).unwrap();

    let tip_after = test_repo.get_branch_target("feature-a");
    assert_ne!(tip_after, tip_before, "the rebase rewrote the commit");
    let reopened = git2::Repository::open(test_repo.workdir()).unwrap();
    let info = gather_repo_info(&reopened, false, 1).unwrap();
    let ids = IdAllocator::new(info.collect_entities());
    assert_eq!(ids.get_commit(tip_after), id_before);
}

#[test]
fn filter_by_persistent_prefix_skips_ambiguous_ones() {
    let test_repo = TestRepo::new_with_remote();
    let a = test_repo.commit(
        "A\n\nChange-Id: I00000000000000000000000000000000000001c3\n",
        "a.txt",
    );
    test_repo.commit(
        "B\n\nChange-Id: I0000000000000000000000000000000000000510\n",
        "b.txt",
    );

    let info = gather_repo_info(&test_repo.repo, true, 1).unwrap();
    let allocator = IdAllocator::new(info.collect_entities());
    let filter =
        |arg: &str| resolve_commit_filter(&test_repo.repo, &[arg.to_string()], &info, &allocator);
    assert_eq!(filter("sunac").into_iter().collect::<Vec<_>>(), vec![a]);
    assert_eq!(
        filter("I00000000000000000000000000000000000001c3")
            .into_iter()
            .collect::<Vec<_>>(),
        vec![a]
    );
    assert!(filter("sun").is_empty(), "ambiguous prefix is skipped");
}

#[test]
fn filter_skips_a_hash_shaped_persistent_id_that_is_also_a_hash_prefix() {
    let test_repo = TestRepo::new_with_remote();
    let ids = hash_shaped_ids(&test_repo);

    let info = gather_repo_info(&test_repo.repo, true, 1).unwrap();
    let allocator = IdAllocator::new(info.collect_entities());
    let filter =
        |arg: &str| resolve_commit_filter(&test_repo.repo, &[arg.to_string()], &info, &allocator);
    assert!(
        filter(&ids.shadowed).is_empty(),
        "shadowed by a hash prefix"
    );
    assert_eq!(
        filter(&ids.sibling).into_iter().collect::<Vec<_>>(),
        vec![ids.sibling_oid]
    );
}
