use std::collections::HashMap;

use super::{
    BranchChanges, CLOSED, Change, STALE, SshProgram, SshServer, branch_change_ids,
    parse_query_output, saved_value,
};
use crate::core::test_helpers::{base_info, branch, commit};

#[test]
fn ssh_urls_name_the_server_and_project() {
    assert_eq!(
        SshServer::parse("ssh://me@codereview.qt-project.org:29418/qt/qtbase.git"),
        Some(SshServer {
            destination: "me@codereview.qt-project.org".into(),
            port: Some("29418".into()),
            project: "qt/qtbase".into(),
        })
    );
    assert_eq!(
        SshServer::parse("review.example.com:tools/repo"),
        Some(SshServer {
            destination: "review.example.com".into(),
            port: None,
            project: "tools/repo".into(),
        })
    );
    assert_eq!(
        SshServer::parse("ssh://host/a.git.git").map(|s| s.project),
        Some("a.git".into())
    );
    assert_eq!(
        SshServer::parse("git+ssh://host/p").map(|s| s.project),
        Some("p".into())
    );
    assert_eq!(SshServer::parse("https://review.example.com/repo"), None);
    assert_eq!(SshServer::parse("/srv/git/repo.git"), None);
    assert_eq!(SshServer::parse("C:/git/repo.git"), None);
    assert_eq!(SshServer::parse("./a:b/repo.git"), None);
}

#[test]
fn query_output_gives_open_revisions_and_closed_changes() {
    let stdout = r#"{"id":"Ia","status":"NEW","currentPatchSet":{"number":2,"revision":"aaa"}}
{"id":"Ib","status":"MERGED","currentPatchSet":{"number":1,"revision":"bbb"}}
{"id":"Ic","status":"ABANDONED","currentPatchSet":{"number":1,"revision":"ccc"}}
{"type":"stats","rowCount":3}
"#;
    let changes = parse_query_output(stdout).unwrap();
    assert_eq!(changes.len(), 3);
    assert_eq!(changes["Ia"], Change::Open("aaa".into()));
    assert_eq!(changes["Ib"], Change::Closed);
    assert_eq!(changes["Ic"], Change::Closed);
}

#[test]
fn the_topmost_known_change_decides() {
    let changes = HashMap::from([
        ("Ilow".to_string(), Change::Open("low-rev".into())),
        ("Imerged".to_string(), Change::Closed),
    ]);
    assert_eq!(
        saved_value(&["Inew", "Ilow"], Some("Inew"), &changes),
        Some("low-rev"),
        "a commit never uploaded sits on top"
    );
    assert_eq!(
        saved_value(&["Imerged"], Some("Imerged"), &changes),
        Some(CLOSED)
    );
    assert_eq!(
        saved_value(&["Inew", "Imerged"], Some("Inew"), &changes),
        Some(STALE),
        "a new commit above a merged one is not closed"
    );
    assert_eq!(saved_value(&["Inew"], Some("Inew"), &changes), None);
}

#[test]
fn a_stacked_branch_owns_only_the_changes_above_the_one_below() {
    let with_id = |byte, parent, id: &str| {
        let mut c = commit(byte, "msg", parent);
        c.change_id = Some(id.to_string());
        c
    };
    let mut info = base_info();
    // base 0xAA <- 1 (Ia, lower tip) <- 2 (Ib) <- 3 (Ic, upper tip)
    info.commits = vec![
        with_id(3, Some(2), "Ic"),
        with_id(2, Some(1), "Ib"),
        with_id(1, Some(0xAA), "Ia"),
    ];
    info.branches = vec![branch("upper", 3, None), branch("lower", 1, None)];
    let branches = branch_change_ids(&info);
    let upper = BranchChanges {
        name: "upper",
        tip_id: Some("Ic"),
        ids: vec!["Ic", "Ib"],
    };
    let lower = BranchChanges {
        name: "lower",
        tip_id: Some("Ia"),
        ids: vec!["Ia"],
    };
    assert_eq!(branches, vec![upper, lower]);
}

#[test]
fn only_a_program_named_ssh_counts_as_openssh() {
    for (program, ssh) in [
        (SshProgram::Path("ssh".into()), true),
        (
            SshProgram::Path("C:/Windows/System32/OpenSSH/ssh.exe".into()),
            true,
        ),
        (SshProgram::Shell("/usr/bin/ssh -i key".into()), true),
        (SshProgram::Path("plink.exe".into()), false),
        (SshProgram::Shell("tortoiseplink -batch".into()), false),
    ] {
        assert_eq!(program.is_named_ssh(), ssh, "{program:?}");
    }
}

#[test]
fn the_ssh_program_runs_through_git() {
    let args = |p: SshProgram| {
        let command = p.command();
        assert_eq!(command.get_program(), "git");
        command.get_args().nth(1).unwrap().to_owned()
    };
    assert_eq!(
        args(SshProgram::Shell("ssh -i key".into())),
        "alias.loom-gerrit-ssh=!ssh -i key"
    );
    assert_eq!(
        args(SshProgram::Path("/opt/it's ssh".into())),
        r"alias.loom-gerrit-ssh=!'/opt/it'\''s ssh'"
    );
}

#[test]
fn an_error_row_or_a_truncated_answer_fails_the_query() {
    let error = r#"{"type":"error","message":"Unsupported operator nosuchop:x"}"#;
    let err = parse_query_output(error).unwrap_err();
    assert!(err.to_string().contains("Unsupported operator"), "{err}");

    let truncated = r#"{"id":"Ia","status":"NEW","currentPatchSet":{"revision":"aaa"}}
{"type":"stats","rowCount":1,"moreChanges":true}"#;
    assert!(parse_query_output(truncated).is_err());
}

#[test]
fn an_answer_without_its_stats_row_fails_the_query() {
    assert!(parse_query_output("").is_err());
    let cut = r#"{"id":"Ia","status":"NEW","currentPatchSet":{"revision":"aaa"}}"#;
    assert!(parse_query_output(cut).is_err());
    let garbled = r#"{"id":"Ia","status":"NEW","curr
{"type":"stats","rowCount":1}"#;
    assert!(parse_query_output(garbled).is_err());
}
