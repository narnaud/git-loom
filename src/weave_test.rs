use git2::Oid;

use super::*;

// ── Helper: create test OIDs ────────────────────────────────────────────

fn oid(hex: &str) -> Oid {
    // Pad to 40 chars with zeros (hex must be valid hex chars)
    let padded = format!("{:0<40}", hex);
    Oid::from_str(&padded).unwrap()
}

fn make_commit(hex: &str, message: &str) -> CommitEntry {
    CommitEntry {
        oid: oid(hex),
        short_hash: hex[..7.min(hex.len())].to_string(),
        message: message.to_string(),
        command: Command::Pick,
        update_refs: Vec::new(),
    }
}

fn make_commit_with_refs(hex: &str, message: &str, refs: Vec<&str>) -> CommitEntry {
    CommitEntry {
        oid: oid(hex),
        short_hash: hex[..7.min(hex.len())].to_string(),
        message: message.to_string(),
        command: Command::Pick,
        update_refs: refs.into_iter().map(String::from).collect(),
    }
}

// Use valid hex strings for test OIDs
const BASE: &str = "ba5e000";
const OID_A1: &str = "abc1234";
const OID_A2: &str = "def5678";
const OID_B1: &str = "bbb2222";
const OID_INT: &str = "1111111";
const OID_C1: &str = "ccc3333";
const OID_C2: &str = "ddd4444";
const OID_MERGE1: &str = "9999999";
const OID_MERGE2: &str = "8888888";
const OID_FIX: &str = "eee5555";

// ── Serialization tests ─────────────────────────────────────────────────

#[test]
fn serialize_single_branch_section() {
    let graph = Weave {
        base_oid: oid(BASE),

        branch_sections: vec![BranchSection {
            reset_target: "onto".to_string(),
            commits: vec![make_commit(OID_A1, "A1"), make_commit(OID_A2, "A2")],
            label: "feature-a".to_string(),
            branch_names: vec!["feature-a".to_string()],
        }],
        integration_line: vec![
            IntegrationEntry::Pick(make_commit(OID_INT, "Int")),
            IntegrationEntry::Merge {
                original_oid: Some(oid(OID_MERGE1)),
                label: "feature-a".to_string(),
            },
        ],
    };

    let todo = graph.to_todo();
    let lines: Vec<&str> = todo.lines().collect();

    assert_eq!(lines[0], "label onto");
    assert_eq!(lines[1], "");
    assert_eq!(lines[2], "reset onto");
    assert_eq!(lines[3], &format!("pick {} A1", OID_A1));
    assert_eq!(lines[4], &format!("pick {} A2", OID_A2));
    assert_eq!(lines[5], "label feature-a");
    assert_eq!(lines[6], "update-ref refs/heads/feature-a");
    assert_eq!(lines[7], "");
    assert_eq!(lines[8], "reset onto");
    assert_eq!(lines[9], &format!("pick {} Int", OID_INT));
    assert!(lines[10].starts_with(&format!("merge -C {} feature-a", OID_MERGE1)));
}

#[test]
fn serialize_two_branch_sections() {
    let graph = Weave {
        base_oid: oid(BASE),

        branch_sections: vec![
            BranchSection {
                reset_target: "onto".to_string(),
                commits: vec![make_commit(OID_A1, "A1")],
                label: "feature-a".to_string(),
                branch_names: vec!["feature-a".to_string()],
            },
            BranchSection {
                reset_target: "onto".to_string(),
                commits: vec![make_commit(OID_B1, "B1")],
                label: "feature-b".to_string(),
                branch_names: vec!["feature-b".to_string()],
            },
        ],
        integration_line: vec![
            IntegrationEntry::Pick(make_commit(OID_INT, "Int")),
            IntegrationEntry::Merge {
                original_oid: Some(oid(OID_MERGE1)),
                label: "feature-a".to_string(),
            },
            IntegrationEntry::Merge {
                original_oid: Some(oid(OID_MERGE2)),
                label: "feature-b".to_string(),
            },
        ],
    };

    let todo = graph.to_todo();
    assert!(todo.contains("label feature-a\n"));
    assert!(todo.contains("label feature-b\n"));
    assert!(todo.contains(&format!("merge -C {} feature-a", OID_MERGE1)));
    assert!(todo.contains(&format!("merge -C {} feature-b", OID_MERGE2)));
}

#[test]
fn serialize_new_merge_without_oid() {
    let graph = Weave {
        base_oid: oid(BASE),

        branch_sections: vec![BranchSection {
            reset_target: "onto".to_string(),
            commits: vec![make_commit(OID_A1, "A1")],
            label: "feature-a".to_string(),
            branch_names: vec!["feature-a".to_string()],
        }],
        integration_line: vec![IntegrationEntry::Merge {
            original_oid: None,
            label: "feature-a".to_string(),
        }],
    };

    let todo = graph.to_todo();
    assert!(todo.contains("merge feature-a # Merge branch 'feature-a'"));
}

#[test]
fn serialize_colocated_branches() {
    let graph = Weave {
        base_oid: oid(BASE),

        branch_sections: vec![BranchSection {
            reset_target: "onto".to_string(),
            commits: vec![make_commit(OID_A1, "A1")],
            label: "feature-a".to_string(),
            branch_names: vec!["feature-a".to_string(), "feature-b".to_string()],
        }],
        integration_line: vec![IntegrationEntry::Merge {
            original_oid: Some(oid(OID_MERGE1)),
            label: "feature-a".to_string(),
        }],
    };

    let todo = graph.to_todo();
    assert!(todo.contains("update-ref refs/heads/feature-a\n"));
    assert!(todo.contains("update-ref refs/heads/feature-b\n"));
}

#[test]
fn serialize_update_refs_on_integration_line() {
    let graph = Weave {
        base_oid: oid(BASE),

        branch_sections: vec![],
        integration_line: vec![IntegrationEntry::Pick(make_commit_with_refs(
            OID_C1,
            "C1",
            vec!["non-woven"],
        ))],
    };

    let todo = graph.to_todo();
    assert!(todo.contains(&format!("pick {} C1\n", OID_C1)));
    assert!(todo.contains("update-ref refs/heads/non-woven\n"));
}

#[test]
fn serialize_empty_graph() {
    let graph = Weave {
        base_oid: oid(BASE),

        branch_sections: vec![],
        integration_line: vec![],
    };

    let todo = graph.to_todo();
    assert_eq!(todo, "label onto\n\nreset onto\n");
}

// ── Mutation tests ──────────────────────────────────────────────────────

#[test]
fn drop_commit_from_branch_section() {
    let mut graph = Weave {
        base_oid: oid(BASE),

        branch_sections: vec![BranchSection {
            reset_target: "onto".to_string(),
            commits: vec![make_commit(OID_A1, "A1"), make_commit(OID_A2, "A2")],
            label: "feature-a".to_string(),
            branch_names: vec!["feature-a".to_string()],
        }],
        integration_line: vec![IntegrationEntry::Merge {
            original_oid: Some(oid(OID_MERGE1)),
            label: "feature-a".to_string(),
        }],
    };

    graph.drop_commit(oid(OID_A1));

    assert_eq!(graph.branch_sections.len(), 1);
    assert_eq!(graph.branch_sections[0].commits.len(), 1);
    assert_eq!(graph.branch_sections[0].commits[0].message, "A2");
}

#[test]
fn drop_last_commit_removes_section_and_merge() {
    let mut graph = Weave {
        base_oid: oid(BASE),

        branch_sections: vec![BranchSection {
            reset_target: "onto".to_string(),
            commits: vec![make_commit(OID_A1, "A1")],
            label: "feature-a".to_string(),
            branch_names: vec!["feature-a".to_string()],
        }],
        integration_line: vec![
            IntegrationEntry::Pick(make_commit(OID_INT, "Int")),
            IntegrationEntry::Merge {
                original_oid: Some(oid(OID_MERGE1)),
                label: "feature-a".to_string(),
            },
        ],
    };

    graph.drop_commit(oid(OID_A1));

    assert!(graph.branch_sections.is_empty());
    assert_eq!(graph.integration_line.len(), 1); // Only "Int" pick remains
}

#[test]
fn drop_commit_from_integration_line() {
    let mut graph = Weave {
        base_oid: oid(BASE),

        branch_sections: vec![],
        integration_line: vec![
            IntegrationEntry::Pick(make_commit(OID_C1, "C1")),
            IntegrationEntry::Pick(make_commit(OID_C2, "C2")),
        ],
    };

    graph.drop_commit(oid(OID_C1));

    assert_eq!(graph.integration_line.len(), 1);
}

#[test]
fn drop_branch_removes_section_and_merge() {
    let mut graph = Weave {
        base_oid: oid(BASE),

        branch_sections: vec![
            BranchSection {
                reset_target: "onto".to_string(),
                commits: vec![make_commit(OID_A1, "A1")],
                label: "feature-a".to_string(),
                branch_names: vec!["feature-a".to_string()],
            },
            BranchSection {
                reset_target: "onto".to_string(),
                commits: vec![make_commit(OID_B1, "B1")],
                label: "feature-b".to_string(),
                branch_names: vec!["feature-b".to_string()],
            },
        ],
        integration_line: vec![
            IntegrationEntry::Merge {
                original_oid: Some(oid(OID_MERGE1)),
                label: "feature-a".to_string(),
            },
            IntegrationEntry::Merge {
                original_oid: Some(oid(OID_MERGE2)),
                label: "feature-b".to_string(),
            },
        ],
    };

    graph.drop_branch("feature-a");

    assert_eq!(graph.branch_sections.len(), 1);
    assert_eq!(graph.branch_sections[0].label, "feature-b");
    assert_eq!(graph.integration_line.len(), 1);
}

#[test]
fn move_commit_to_branch() {
    let mut graph = Weave {
        base_oid: oid(BASE),

        branch_sections: vec![BranchSection {
            reset_target: "onto".to_string(),
            commits: vec![make_commit(OID_A1, "A1")],
            label: "feature-a".to_string(),
            branch_names: vec!["feature-a".to_string()],
        }],
        integration_line: vec![
            IntegrationEntry::Pick(make_commit(OID_C1, "C1")),
            IntegrationEntry::Merge {
                original_oid: Some(oid(OID_MERGE1)),
                label: "feature-a".to_string(),
            },
        ],
    };

    graph.move_commit(oid(OID_C1), "feature-a");

    // C1 should now be at the end of feature-a's section
    assert_eq!(graph.branch_sections[0].commits.len(), 2);
    assert_eq!(graph.branch_sections[0].commits[1].message, "C1");
    // C1 should be gone from integration line
    assert_eq!(graph.integration_line.len(), 1);
}

#[test]
fn move_commit_to_colocated_branch_splits_section() {
    // Setup: feature-a and feature-b are co-located (same section).
    // Moving a commit to feature-b should split the section so the commit
    // only appears on feature-b, not on feature-a.
    let mut graph = Weave {
        base_oid: oid(BASE),

        branch_sections: vec![
            BranchSection {
                reset_target: "onto".to_string(),
                commits: vec![make_commit(OID_A1, "A1")],
                label: "feature-a".to_string(),
                branch_names: vec!["feature-a".to_string(), "feature-b".to_string()],
            },
            BranchSection {
                reset_target: "onto".to_string(),
                commits: vec![make_commit(OID_B1, "B1"), make_commit(OID_C1, "C1")],
                label: "feature-c".to_string(),
                branch_names: vec!["feature-c".to_string()],
            },
        ],
        integration_line: vec![
            IntegrationEntry::Merge {
                original_oid: Some(oid(OID_MERGE1)),
                label: "feature-a".to_string(),
            },
            IntegrationEntry::Merge {
                original_oid: Some(oid(OID_MERGE2)),
                label: "feature-c".to_string(),
            },
        ],
    };

    graph.move_commit(oid(OID_C1), "feature-b");

    // Should now have 3 sections: feature-a, feature-b (stacked on a), feature-c
    assert_eq!(graph.branch_sections.len(), 3);

    // feature-a keeps its original commits and only has feature-a in branch_names
    assert_eq!(graph.branch_sections[0].label, "feature-a");
    assert_eq!(graph.branch_sections[0].commits.len(), 1);
    assert_eq!(graph.branch_sections[0].commits[0].message, "A1");
    assert_eq!(
        graph.branch_sections[0].branch_names,
        vec!["feature-a".to_string()]
    );

    // feature-b is stacked on feature-a with the moved commit
    assert_eq!(graph.branch_sections[1].label, "feature-b");
    assert_eq!(graph.branch_sections[1].reset_target, "feature-a");
    assert_eq!(graph.branch_sections[1].commits.len(), 1);
    assert_eq!(graph.branch_sections[1].commits[0].message, "C1");
    assert_eq!(
        graph.branch_sections[1].branch_names,
        vec!["feature-b".to_string()]
    );

    // feature-c lost one commit
    assert_eq!(graph.branch_sections[2].label, "feature-c");
    assert_eq!(graph.branch_sections[2].commits.len(), 1);

    // The merge entry that was "feature-a" should now reference "feature-b" (outermost)
    if let IntegrationEntry::Merge { label, .. } = &graph.integration_line[0] {
        assert_eq!(label, "feature-b");
    } else {
        panic!("Expected Merge entry");
    }

    // Verify serialized todo: feature-b section resets to feature-a
    let todo = graph.to_todo();
    assert!(todo.contains("label feature-a\nupdate-ref refs/heads/feature-a\n"));
    assert!(todo.contains("reset feature-a\n"));
    assert!(todo.contains("label feature-b\nupdate-ref refs/heads/feature-b\n"));
}

#[test]
fn move_commit_to_colocated_branch_when_target_is_label() {
    // Setup: section label IS the target branch.
    // Moving a commit to the label branch should rename the base section to
    // a remaining branch and create a stacked section for the target.
    let mut graph = Weave {
        base_oid: oid(BASE),

        branch_sections: vec![BranchSection {
            reset_target: "onto".to_string(),
            commits: vec![make_commit(OID_A1, "A1")],
            label: "feature-a".to_string(),
            branch_names: vec!["feature-a".to_string(), "feature-b".to_string()],
        }],
        integration_line: vec![
            IntegrationEntry::Pick(make_commit(OID_C1, "C1")),
            IntegrationEntry::Merge {
                original_oid: Some(oid(OID_MERGE1)),
                label: "feature-a".to_string(),
            },
        ],
    };

    graph.move_commit(oid(OID_C1), "feature-a");

    // Should have 2 sections now
    assert_eq!(graph.branch_sections.len(), 2);

    // Base section is renamed to feature-b
    assert_eq!(graph.branch_sections[0].label, "feature-b");
    assert_eq!(
        graph.branch_sections[0].branch_names,
        vec!["feature-b".to_string()]
    );

    // Stacked section for feature-a
    assert_eq!(graph.branch_sections[1].label, "feature-a");
    assert_eq!(graph.branch_sections[1].reset_target, "feature-b");
    assert_eq!(graph.branch_sections[1].commits.len(), 1);
    assert_eq!(graph.branch_sections[1].commits[0].message, "C1");

    // C1 was removed from integration line, only the merge remains
    assert_eq!(graph.integration_line.len(), 1);

    // Merge entry should reference feature-a (the outermost)
    if let IntegrationEntry::Merge { label, .. } = &graph.integration_line[0] {
        assert_eq!(label, "feature-a");
    } else {
        panic!("Expected Merge entry");
    }
}

#[test]
fn fixup_commit_moves_and_changes_command() {
    let mut graph = Weave {
        base_oid: oid(BASE),

        branch_sections: vec![],
        integration_line: vec![
            IntegrationEntry::Pick(make_commit(OID_C1, "C1")),
            IntegrationEntry::Pick(make_commit(OID_C2, "C2")),
            IntegrationEntry::Pick(make_commit(OID_FIX, "Fixup for C1")),
        ],
    };

    graph.fixup_commit(oid(OID_FIX), oid(OID_C1));

    assert_eq!(graph.integration_line.len(), 3);

    // C1 at index 0, fixup at index 1, C2 at index 2
    if let IntegrationEntry::Pick(c) = &graph.integration_line[0] {
        assert_eq!(c.message, "C1");
    }
    if let IntegrationEntry::Pick(c) = &graph.integration_line[1] {
        assert_eq!(c.message, "Fixup for C1");
        assert_eq!(c.command, Command::Fixup);
    }
    if let IntegrationEntry::Pick(c) = &graph.integration_line[2] {
        assert_eq!(c.message, "C2");
    }
}

#[test]
fn edit_commit_changes_command() {
    let mut graph = Weave {
        base_oid: oid(BASE),

        branch_sections: vec![],
        integration_line: vec![IntegrationEntry::Pick(make_commit(OID_C1, "C1"))],
    };

    graph.edit_commit(oid(OID_C1));

    if let IntegrationEntry::Pick(c) = &graph.integration_line[0] {
        assert_eq!(c.command, Command::Edit);
    }
}

#[test]
fn add_branch_section_and_merge() {
    let mut graph = Weave {
        base_oid: oid(BASE),

        branch_sections: vec![],
        integration_line: vec![IntegrationEntry::Pick(make_commit(OID_C1, "C1"))],
    };

    graph.add_branch_section(
        "new-branch".to_string(),
        vec!["new-branch".to_string()],
        vec![make_commit(OID_B1, "N1")],
        "onto".to_string(),
    );

    graph.add_merge("new-branch".to_string(), None, None);

    assert_eq!(graph.branch_sections.len(), 1);
    assert_eq!(graph.branch_sections[0].label, "new-branch");
    assert_eq!(graph.integration_line.len(), 2);
}

#[test]
fn reassign_branch_renames_section() {
    let mut graph = Weave {
        base_oid: oid(BASE),

        branch_sections: vec![BranchSection {
            reset_target: "onto".to_string(),
            commits: vec![make_commit(OID_A1, "A1")],
            label: "feature-a".to_string(),
            branch_names: vec!["feature-a".to_string(), "feature-b".to_string()],
        }],
        integration_line: vec![IntegrationEntry::Merge {
            original_oid: Some(oid(OID_MERGE1)),
            label: "feature-a".to_string(),
        }],
    };

    graph.reassign_branch("feature-a", "feature-b");

    assert_eq!(graph.branch_sections[0].label, "feature-b");
    assert!(
        !graph.branch_sections[0]
            .branch_names
            .contains(&"feature-a".to_string())
    );
    assert!(
        graph.branch_sections[0]
            .branch_names
            .contains(&"feature-b".to_string())
    );

    if let IntegrationEntry::Merge { label, .. } = &graph.integration_line[0] {
        assert_eq!(label, "feature-b");
    }
}

// ── Integration test: from_repo ─────────────────────────────────────────

#[test]
fn from_repo_linear_integration() {
    use crate::test_helpers::TestRepo;

    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("C1", "c1.txt");
    test_repo.commit("C2", "c2.txt");

    let graph = Weave::from_repo(&test_repo.repo).unwrap();

    assert!(graph.branch_sections.is_empty());
    assert_eq!(graph.integration_line.len(), 2);

    // Oldest first
    if let IntegrationEntry::Pick(c) = &graph.integration_line[0] {
        assert_eq!(c.message, "C1");
    } else {
        panic!("Expected Pick entry");
    }
    if let IntegrationEntry::Pick(c) = &graph.integration_line[1] {
        assert_eq!(c.message, "C2");
    } else {
        panic!("Expected Pick entry");
    }
}

#[test]
fn from_repo_with_woven_branch() {
    use crate::git_commands::{git_branch, git_merge};
    use crate::test_helpers::TestRepo;

    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create feature-a at merge-base
    git_branch::create(workdir.as_path(), "feature-a", &base_oid.to_string()).unwrap();

    // Switch to feature-a and add commits
    test_repo.switch_branch("feature-a");
    test_repo.commit("A1", "a1.txt");
    test_repo.commit("A2", "a2.txt");

    // Switch back to integration
    test_repo.switch_branch("integration");

    // Add a commit on integration before merging
    test_repo.commit("Int", "int.txt");

    // Merge feature-a
    git_merge::merge_no_ff(workdir.as_path(), "feature-a").unwrap();

    let graph = Weave::from_repo(&test_repo.repo).unwrap();

    // Should have one branch section for feature-a
    assert_eq!(graph.branch_sections.len(), 1);
    assert_eq!(graph.branch_sections[0].label, "feature-a");
    assert_eq!(graph.branch_sections[0].commits.len(), 2);
    assert_eq!(graph.branch_sections[0].commits[0].message, "A1");
    assert_eq!(graph.branch_sections[0].commits[1].message, "A2");

    // Integration line should have Int + merge
    assert_eq!(graph.integration_line.len(), 2);
    if let IntegrationEntry::Pick(c) = &graph.integration_line[0] {
        assert_eq!(c.message, "Int");
    } else {
        panic!("Expected Pick entry for Int");
    }
    if let IntegrationEntry::Merge {
        label,
        original_oid,
    } = &graph.integration_line[1]
    {
        assert_eq!(label, "feature-a");
        assert!(original_oid.is_some());
    } else {
        panic!("Expected Merge entry for feature-a");
    }
}

#[test]
fn from_repo_with_non_woven_branch() {
    use crate::git_commands::git_branch;
    use crate::test_helpers::TestRepo;

    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();

    // Add commits on integration
    test_repo.commit("C1", "c1.txt");
    let c1_oid = test_repo.head_oid();

    // Create feature-a at C1 (non-woven, just a branch pointing at a commit)
    git_branch::create(workdir.as_path(), "feature-a", &c1_oid.to_string()).unwrap();

    test_repo.commit("C2", "c2.txt");

    let graph = Weave::from_repo(&test_repo.repo).unwrap();

    // No branch sections (not woven)
    assert!(graph.branch_sections.is_empty());

    // Integration line should have C1 with update-ref + C2
    assert_eq!(graph.integration_line.len(), 2);
    if let IntegrationEntry::Pick(c) = &graph.integration_line[0] {
        assert_eq!(c.message, "C1");
        assert!(c.update_refs.contains(&"feature-a".to_string()));
    } else {
        panic!("Expected Pick entry for C1");
    }
}

#[test]
fn from_repo_round_trip_preserves_identity() {
    use crate::git_commands::{git_branch, git_merge};
    use crate::test_helpers::TestRepo;

    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Build a non-trivial graph
    git_branch::create(workdir.as_path(), "feature-a", &base_oid.to_string()).unwrap();
    test_repo.switch_branch("feature-a");
    test_repo.commit("A1", "a1.txt");
    test_repo.switch_branch("integration");
    test_repo.commit("Int", "int.txt");
    git_merge::merge_no_ff(workdir.as_path(), "feature-a").unwrap();

    // Record state before round-trip
    let messages_before: Vec<String> = {
        let info = git::gather_repo_info(&test_repo.repo).unwrap();
        info.commits.iter().map(|c| c.message.clone()).collect()
    };

    // Build graph, serialize, and run through rebase
    let graph = Weave::from_repo(&test_repo.repo).unwrap();
    let todo = graph.to_todo();

    // Pass the merge-base OID directly as upstream (not merge_base^)
    run_rebase(workdir.as_path(), Some(&graph.base_oid.to_string()), &todo).unwrap();

    // Verify identity: same commits after round-trip
    let messages_after: Vec<String> = {
        let info = git::gather_repo_info(&test_repo.repo).unwrap();
        info.commits.iter().map(|c| c.message.clone()).collect()
    };

    assert_eq!(messages_before, messages_after);
}
