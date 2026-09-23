use serde_json::Value;

use crate::core::graph;
use crate::core::repo::{ContextCommit, FileChange, RemoteStatus, RepoInfo};
use crate::core::shortid::IdAllocator;
use crate::core::status_json::{self, StackEdge};
use crate::core::test_helpers::{
    base_info, branch, commit, stack_edges, status_graph_in as build, strip_ansi,
};

/// Build and round-trip through serde so tests assert the wire shape, not the
/// Rust one.
fn json(info: RepoInfo) -> Value {
    serde_json::to_value(build(info, "")).unwrap()
}

/// Every group's `stacked_on` must be exactly what a stacked push follows.
fn assert_matches_stack_parent(info: &RepoInfo, j: &Value) {
    for group in j["branches"].as_array().unwrap() {
        let canonical = group["names"].as_array().unwrap().last().unwrap()["name"]
            .as_str()
            .unwrap();
        let expected = graph::stack_parent(info, canonical);
        assert_eq!(
            group["stacked_on"].as_str().map(str::to_string),
            expected,
            "group {canonical}"
        );
    }
}

#[test]
fn empty_repo_has_every_top_level_key() {
    let j = json(base_info());
    assert_eq!(j["schema"], 1);
    assert_eq!(j["integration_branch"], "integration");
    assert_eq!(j["cwd_prefix"], "");
    assert_eq!(j["local_changes"]["id"], "zz");
    assert_eq!(j["local_changes"]["files"].as_array().unwrap().len(), 0);
    assert_eq!(j["branches"].as_array().unwrap().len(), 0);
    assert_eq!(j["loose_commits"].as_array().unwrap().len(), 0);
    assert_eq!(j["context_commits"].as_array().unwrap().len(), 0);
    assert_eq!(j["upstream"]["label"], "origin/main");
    assert_eq!(j["upstream"]["base_hash"], "aaa0000");
    assert_eq!(j["upstream"]["base_subject"], "Initial commit");
    assert_eq!(j["upstream"]["base_date"], "2025-07-06");
    assert_eq!(j["upstream"]["commits_ahead"], 0);
}

/// feature-ui stacks on feature-api: the JSON must name the edge, and each
/// branch must own only the commits above the one below it (spec 001).
#[test]
fn stacked_branches_name_the_branch_below() {
    let mut info = base_info();
    info.commits = vec![
        commit(0x03, "ui: panel", Some(0x02)),
        commit(0x02, "ui: route", Some(0x01)),
        commit(0x01, "api: endpoint", Some(0xAA)),
    ];
    info.branches = vec![
        branch("feature-api", 0x01, Some(RemoteStatus::Synced)),
        branch("feature-ui", 0x03, None),
    ];

    let j = json(info);
    let groups = j["branches"].as_array().unwrap();
    assert_eq!(groups.len(), 2);

    assert_eq!(groups[0]["names"][0]["name"], "feature-ui");
    assert_eq!(groups[0]["names"][0]["remote"], Value::Null);
    assert_eq!(groups[0]["stacked_on"], "feature-api");
    let ui: Vec<&str> = groups[0]["commits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["subject"].as_str().unwrap())
        .collect();
    assert_eq!(ui, ["ui: panel", "ui: route"]);

    assert_eq!(groups[1]["names"][0]["name"], "feature-api");
    assert_eq!(groups[1]["names"][0]["remote"], "synced");
    assert_eq!(groups[1]["stacked_on"], Value::Null);
    assert_eq!(groups[1]["commits"].as_array().unwrap().len(), 1);
    assert_eq!(groups[1]["commits"][0]["subject"], "api: endpoint");
}

/// Two branches forking from the base independently are not a stack.
#[test]
fn parallel_branches_are_not_stacked() {
    let mut info = base_info();
    info.commits = vec![commit(0x02, "B", Some(0xAA)), commit(0x01, "A", Some(0xAA))];
    info.branches = vec![
        branch("feature-a", 0x01, None),
        branch("feature-b", 0x02, None),
    ];

    let j = serde_json::to_value(build(info.clone(), "")).unwrap();
    for group in j["branches"].as_array().unwrap() {
        assert_eq!(group["stacked_on"], Value::Null);
    }
    assert_matches_stack_parent(&info, &j);
}

#[test]
fn colocated_branches_share_one_group() {
    let mut info = base_info();
    info.commits = vec![commit(0x01, "A", Some(0xAA))];
    info.branches = vec![
        branch("feature-a", 0x01, None),
        branch("feature-a-v2", 0x01, Some(RemoteStatus::Gone)),
    ];

    let j = json(info);
    let groups = j["branches"].as_array().unwrap();
    assert_eq!(groups.len(), 1);
    let names: Vec<&str> = groups[0]["names"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["name"].as_str().unwrap())
        .collect();
    // Alphabetically last first, as the tree renders them.
    assert_eq!(names, ["feature-a-v2", "feature-a"]);
    assert_eq!(groups[0]["names"][0]["remote"], "gone");
    assert_eq!(groups[0]["commits"].as_array().unwrap().len(), 1);
}

/// Stacking onto a co-located pair: `stacked_on` must name the edge the way a
/// stacked push does, so the two surfaces agree on which ref is below.
#[test]
fn stacked_on_names_a_colocated_group_as_a_stacked_push_does() {
    let mut info = base_info();
    info.commits = vec![
        commit(0x02, "ui", Some(0x01)),
        commit(0x01, "api", Some(0xAA)),
    ];
    info.branches = vec![
        branch("feature-api", 0x01, None),
        branch("feature-api-v2", 0x01, None),
        branch("feature-ui", 0x02, None),
    ];

    let j = serde_json::to_value(build(info.clone(), "")).unwrap();
    let groups = j["branches"].as_array().unwrap();
    assert_eq!(groups[0]["names"][0]["name"], "feature-ui");
    assert_eq!(groups[0]["stacked_on"], "feature-api");
    assert_matches_stack_parent(&info, &j);
}

/// A branch sitting at the merge base owns no commits, so it renders as an
/// empty section far from the branch above it and no `││` is drawn — but a
/// stacked push still walks the edge, so `stacked_on` must report it.
#[test]
fn stacked_on_reports_a_branch_below_that_owns_no_commits() {
    let mut info = base_info();
    info.commits = vec![commit(0x01, "A", Some(0xAA))];
    info.branches = vec![
        branch("feature-a", 0x01, None),
        branch("feature-new", 0xAA, None),
    ];

    let j = serde_json::to_value(build(info.clone(), "")).unwrap();
    let groups = j["branches"].as_array().unwrap();
    assert_eq!(groups[0]["names"][0]["name"], "feature-new");
    assert_eq!(groups[0]["stacked_on"], Value::Null);
    assert_eq!(groups[1]["names"][0]["name"], "feature-a");
    assert_eq!(groups[1]["stacked_on"], "feature-new");
    assert_matches_stack_parent(&info, &j);
}

/// A conflicted add reports `?` on the worktree side (`repo.rs` gather), and
/// the file must still land in the conflicted group rather than between them.
#[test]
fn a_conflict_marked_on_one_side_only_is_still_conflicted() {
    let mut info = base_info();
    info.working_changes = vec![FileChange {
        path: "src/both_added.rs".to_string(),
        index: '!',
        worktree: '?',
    }];

    let j = json(info);
    let files = j["local_changes"]["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["state"], "conflicted");
}

/// Every remote state reaches the wire under its documented spelling.
#[test]
fn remote_states_serialize_by_name() {
    let mut info = base_info();
    info.commits = vec![
        commit(0x03, "C", Some(0x02)),
        commit(0x02, "B", Some(0x01)),
        commit(0x01, "A", Some(0xAA)),
    ];
    info.branches = vec![
        branch("feature-a", 0x01, Some(RemoteStatus::Different)),
        branch("feature-b", 0x02, Some(RemoteStatus::Synced)),
        branch("feature-c", 0x03, Some(RemoteStatus::Gone)),
    ];

    let j = json(info);
    let remote_of = |name: &str| -> String {
        j["branches"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["names"][0]["name"] == name)
            .unwrap_or_else(|| panic!("missing {name}"))["names"][0]["remote"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(remote_of("feature-a"), "different");
    assert_eq!(remote_of("feature-b"), "synced");
    assert_eq!(remote_of("feature-c"), "gone");
}

/// A branch sitting at the base owns nothing and still appears, first.
#[test]
fn empty_branch_is_a_group_with_no_commits() {
    let mut info = base_info();
    info.commits = vec![commit(0x01, "A", Some(0xAA))];
    info.branches = vec![
        branch("feature-a", 0x01, None),
        branch("feature-new", 0xAA, None),
    ];

    let j = json(info);
    let groups = j["branches"].as_array().unwrap();
    assert_eq!(groups[0]["names"][0]["name"], "feature-new");
    assert_eq!(groups[0]["commits"].as_array().unwrap().len(), 0);
    assert_eq!(groups[1]["names"][0]["name"], "feature-a");
}

#[test]
fn commits_owned_by_no_branch_are_loose() {
    let mut info = base_info();
    info.commits = vec![
        commit(0x02, "loose", Some(0x01)),
        commit(0x01, "owned", Some(0xAA)),
    ];
    info.branches = vec![branch("feature-a", 0x01, None)];

    let j = json(info);
    let loose = j["loose_commits"].as_array().unwrap();
    assert_eq!(loose.len(), 1);
    assert_eq!(loose[0]["subject"], "loose");
    assert_eq!(j["branches"][0]["commits"][0]["subject"], "owned");
}

#[test]
fn working_files_carry_their_state_and_raw_status() {
    let mut info = base_info();
    info.working_changes = vec![
        FileChange {
            path: "src/conflict.rs".to_string(),
            index: '!',
            worktree: '!',
        },
        FileChange {
            path: "src/staged.rs".to_string(),
            index: 'M',
            worktree: ' ',
        },
        FileChange {
            path: "src/new.rs".to_string(),
            index: '?',
            worktree: '?',
        },
    ];

    let j = json(info);
    let files = j["local_changes"]["files"].as_array().unwrap();
    let by_path = |p: &str| {
        files
            .iter()
            .find(|f| f["path"] == p)
            .unwrap_or_else(|| panic!("missing {p}"))
    };
    assert_eq!(by_path("src/conflict.rs")["state"], "conflicted");
    assert_eq!(by_path("src/staged.rs")["state"], "tracked");
    assert_eq!(by_path("src/staged.rs")["index"], "M");
    assert_eq!(by_path("src/staged.rs")["worktree"], " ");
    assert_eq!(by_path("src/new.rs")["state"], "untracked");
    // Every file gets a short ID the agent can pass back.
    assert!(files.iter().all(|f| !f["id"].as_str().unwrap().is_empty()));
}

/// Commit file ids are `<commit id>:<n>` counting from 0, matching the tree.
#[test]
fn commit_files_are_numbered_from_zero() {
    let mut info = base_info();
    let mut c = commit(0x01, "A", Some(0xAA));
    c.files = vec![
        FileChange {
            path: "a.rs".to_string(),
            index: 'M',
            worktree: ' ',
        },
        FileChange {
            path: "b.rs".to_string(),
            index: 'A',
            worktree: ' ',
        },
    ];
    info.commits = vec![c];
    info.branches = vec![branch("feature-a", 0x01, None)];

    let j = json(info);
    let commit = &j["branches"][0]["commits"][0];
    let sid = commit["id"].as_str().unwrap();
    let files = commit["files"].as_array().unwrap();
    assert_eq!(files[0]["id"], format!("{sid}:0"));
    assert_eq!(files[1]["id"], format!("{sid}:1"));
    assert_eq!(files[0]["path"], "a.rs");
}

#[test]
fn commit_files_absent_without_the_files_flag() {
    let mut info = base_info();
    info.commits = vec![commit(0x01, "A", Some(0xAA))];
    info.branches = vec![branch("feature-a", 0x01, None)];

    let j = json(info);
    assert_eq!(
        j["branches"][0]["commits"][0]["files"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn change_id_is_exposed_when_present() {
    let mut info = base_info();
    let mut c = commit(0x01, "A", Some(0xAA));
    c.change_id = Some("I3a7b0cf5827e615d05929a80db355b2cc190f765".to_string());
    info.commits = vec![c];
    info.branches = vec![branch("feature-a", 0x01, None)];

    let j = json(info);
    let commit = &j["branches"][0]["commits"][0];
    assert_eq!(
        commit["change_id"],
        "I3a7b0cf5827e615d05929a80db355b2cc190f765"
    );
    assert_eq!(commit["hash"], "0000001");
    assert_eq!(commit["oid"].as_str().unwrap().len(), 40);
    // A Change-Id earns a persistent letter ID (spec 002).
    assert!(
        commit["id"]
            .as_str()
            .unwrap()
            .chars()
            .all(|c| c.is_ascii_lowercase())
    );
}

#[test]
fn commit_without_change_id_serializes_null() {
    let mut info = base_info();
    info.commits = vec![commit(0x01, "A", Some(0xAA))];
    info.branches = vec![branch("feature-a", 0x01, None)];

    let j = json(info);
    assert_eq!(j["branches"][0]["commits"][0]["change_id"], Value::Null);
}

#[test]
fn upstream_ahead_reports_its_count() {
    let mut info = base_info();
    info.upstream.commits_ahead = 3;
    let j = json(info);
    assert_eq!(j["upstream"]["commits_ahead"], 3);
}

#[test]
fn context_commits_are_listed() {
    let mut info = base_info();
    info.context_commits = vec![ContextCommit {
        short_hash: "9f2e1a0".to_string(),
        message: "older".to_string(),
        date: "2025-06-01".to_string(),
    }];

    let j = json(info);
    let ctx = j["context_commits"].as_array().unwrap();
    assert_eq!(ctx.len(), 1);
    assert_eq!(ctx[0]["hash"], "9f2e1a0");
    assert_eq!(ctx[0]["date"], "2025-06-01");
    assert_eq!(ctx[0]["subject"], "older");
}

/// Paths are relative to the cwd, as every other loom surface prints them.
#[test]
fn paths_follow_the_cwd_prefix() {
    let mut info = base_info();
    info.working_changes = vec![FileChange {
        path: "src/core/msg.rs".to_string(),
        index: 'M',
        worktree: ' ',
    }];
    let j = serde_json::to_value(build(info, "src")).unwrap();

    assert_eq!(j["cwd_prefix"], "src");
    assert_eq!(j["local_changes"]["files"][0]["path"], "core/msg.rs");
}

/// The whole object must survive `serde_json::to_string` as one line: it is
/// the last line of stdout (spec 019).
#[test]
fn serializes_to_a_single_line() {
    let mut info = base_info();
    info.commits = vec![commit(0x01, "subject with spaces", Some(0xAA))];
    info.branches = vec![branch("feature-a", 0x01, None)];
    let s = serde_json::to_string(&build(info, "")).unwrap();
    assert!(!s.contains('\n'));
}

/// Hiding cuts the edge to a hidden branch below, yet `push` still refuses the
/// branch above it, so the JSON must say why rather than report no stack.
#[test]
fn stacked_on_hidden_survives_the_hidden_branch_leaving_the_graph() {
    let mut info = base_info();
    info.commits = vec![commit(0x02, "a", Some(0x01))];
    info.branches = vec![branch("feature-a", 0x02, None)];
    let stacks = [(
        "feature-a".to_string(),
        StackEdge {
            below: None,
            below_hidden: true,
        },
    )]
    .into_iter()
    .collect();
    let ids = IdAllocator::new(info.collect_entities());
    let j = serde_json::to_value(status_json::build(
        &graph::build_sections(info),
        "integration",
        &stacks,
        &ids,
        "",
    ))
    .unwrap();
    let group = &j["branches"][0];
    assert_eq!(group["stacked_on"], Value::Null);
    assert_eq!(group["stacked_on_hidden"], true);
}

/// The thesis of the design: the tree and the JSON are two renderings of one
/// section list, so every ID the JSON hands an agent must be an ID the tree
/// shows the person next to them. Guards the claim the prose makes.
#[test]
fn every_json_id_appears_in_the_rendered_tree() {
    let mut info = base_info();
    let mut c = commit(0x02, "ui: panel", Some(0x01));
    c.files = vec![FileChange {
        path: "src/ui.rs".to_string(),
        index: 'M',
        worktree: ' ',
    }];
    info.commits = vec![c, commit(0x01, "api: endpoint", Some(0xAA))];
    info.branches = vec![
        branch("feature-api", 0x01, None),
        branch("feature-ui", 0x02, None),
    ];
    info.working_changes = vec![FileChange {
        path: "src/main.rs".to_string(),
        index: 'M',
        worktree: ' ',
    }];

    let ids = IdAllocator::new(info.collect_entities());
    let stacks = stack_edges(&info);
    let sections = graph::build_sections(info);
    let j = serde_json::to_value(status_json::build(
        &sections,
        "integration",
        &stacks,
        &ids,
        "",
    ))
    .unwrap();
    let tree = strip_ansi(&graph::render_sections(
        &sections,
        &ids,
        &graph::RenderOpts {
            terminal_width: None,
            theme: graph::Theme::dark(),
            cwd_prefix: String::new(),
        },
    ));

    // Whitespace-delimited, so a two-letter ID cannot pass by matching inside
    // a branch name or a path.
    let tokens: Vec<&str> = tree.split_whitespace().collect();
    let mut checked = 0;
    let mut check = |id: &Value| {
        let id = id.as_str().unwrap();
        assert!(
            tokens.contains(&id),
            "tree has no `{id}` token:
{tree}"
        );
        checked += 1;
    };
    check(&j["local_changes"]["id"]);
    for f in j["local_changes"]["files"].as_array().unwrap() {
        check(&f["id"]);
    }
    for group in j["branches"].as_array().unwrap() {
        for n in group["names"].as_array().unwrap() {
            check(&n["id"]);
        }
        for c in group["commits"].as_array().unwrap() {
            check(&c["id"]);
            check(&c["hash"]);
            for f in c["files"].as_array().unwrap() {
                check(&f["id"]);
            }
        }
    }
    assert!(checked >= 7, "only {checked} ids checked");
}
