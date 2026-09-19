use super::*;
use crate::core::test_helpers::TestRepo;
use std::io::Write;
use std::process::{Command, Stdio};

const ID: &str = "I0123456789abcdef0123456789abcdef01234567";

#[test]
fn normalize_accepts_any_case_and_rejects_bad_shapes() {
    assert_eq!(normalize(ID).as_deref(), Some(ID));
    assert_eq!(normalize(&ID.to_uppercase()).as_deref(), Some(ID));
    assert_eq!(
        normalize(" I0123456789abcdef0123456789abcdef01234567\n").as_deref(),
        Some(ID)
    );
    assert!(normalize(&ID[1..]).is_none(), "missing I");
    assert!(normalize(&ID[..40]).is_none(), "39 hex digits");
    assert!(normalize("I0123456789abcdef0123456789abcdef0123456g").is_none());
}

#[test]
fn from_message_reads_change_id_trailer() {
    let msg = format!("Subject\n\nBody.\n\nSigned-off-by: A <a@x>\nChange-Id: {ID}\n");
    assert_eq!(from_message(&msg).as_deref(), Some(ID));
}

#[test]
fn from_message_reads_link_form() {
    let msg = format!("Subject\n\nLink: https://review.example.com/id/{ID}\n");
    assert_eq!(from_message(&msg).as_deref(), Some(ID));
}

#[test]
fn from_message_last_trailer_wins() {
    let other = "Iffffffffffffffffffffffffffffffffffffffff";
    let msg = format!("Subject\n\nChange-Id: {ID}\nChange-Id: {other}\n");
    assert_eq!(from_message(&msg).as_deref(), Some(other));
}

#[test]
fn from_message_ignores_mentions_outside_the_trailer_block() {
    let msg = format!("Subject\n\nSee Change-Id: {ID} in the body.\n\nMore text here.\n");
    assert!(from_message(&msg).is_none());
    assert!(from_message("Subject only\n").is_none());
}

/// The hook pipes `git var GIT_COMMITTER_IDENT`, the refhash, and the message
/// file into `git hash-object --stdin`; the same bytes must hash the same.
#[test]
fn generate_from_matches_git_hash_object() {
    let repo = TestRepo::new();
    let ident = "Test <test@test.com> 1700000000 +0100";
    let refhash = "9f070cd9f070cd9f070cd9f070cd9f070cd9f070";
    let message = "Fix bug\n\nLonger text.\n";
    let mut child = Command::new("git")
        .args(["hash-object", "--stdin"])
        .current_dir(repo.workdir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("{ident}\n{refhash}\n{message}").as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let expected = format!("I{}", String::from_utf8_lossy(&out.stdout).trim());
    assert_eq!(generate_from(ident, refhash, message), expected);
    // `-m` messages have no trailing newline; the message file always does.
    assert_eq!(generate_from(ident, refhash, message.trim_end()), expected);
}

#[test]
fn generate_is_well_formed_and_message_dependent() {
    let repo = TestRepo::new();
    let a = generate(&repo.repo, &repo.workdir(), "one").unwrap();
    let b = generate(&repo.repo, &repo.workdir(), "two").unwrap();
    assert_eq!(normalize(&a).as_deref(), Some(a.as_str()));
    assert_ne!(a, b);
}

#[test]
fn generate_works_on_an_unborn_branch() {
    let repo = TestRepo::new_empty();
    let id = generate(&repo.repo, &repo.workdir(), "first").unwrap();
    assert!(normalize(&id).is_some());
}

#[test]
fn stamp_adds_a_paragraph_when_there_is_no_trailer_block() {
    assert_eq!(
        stamp("Fix bug", ID, None),
        format!("Fix bug\n\nChange-Id: {ID}\n")
    );
    // A `key: value` subject alone is a title, not a trailer block.
    assert_eq!(
        stamp("fix: bug", ID, None),
        format!("fix: bug\n\nChange-Id: {ID}\n")
    );
    assert_eq!(
        stamp("Fix: bug\n\nBody text.\n\n", ID, None),
        format!("Fix: bug\n\nBody text.\n\nChange-Id: {ID}\n")
    );
}

#[test]
fn stamp_joins_an_existing_trailer_block() {
    assert_eq!(
        stamp("Fix bug\n\nSigned-off-by: A <a@x>\n", ID, None),
        format!("Fix bug\n\nSigned-off-by: A <a@x>\nChange-Id: {ID}\n")
    );
}

#[test]
fn stamp_writes_link_form_when_asked() {
    let out = stamp("Fix bug", ID, Some("https://r.example.com"));
    assert_eq!(
        out,
        format!("Fix bug\n\nLink: https://r.example.com/id/{ID}\n")
    );
    assert_eq!(from_message(&out).as_deref(), Some(ID));
}

#[test]
fn enabled_honors_loom_and_gerrit_opt_outs() {
    let repo = TestRepo::new();
    assert!(enabled(&repo.repo));
    repo.set_config("gerrit.createChangeId", "false");
    assert!(!enabled(&repo.repo));
    repo.set_config("gerrit.createChangeId", "always");
    assert!(enabled(&repo.repo));
    repo.set_config("loom.changeId", "false");
    assert!(!enabled(&repo.repo));
}

#[test]
fn link_base_trims_trailing_slash() {
    let repo = TestRepo::new();
    assert!(link_base(&repo.repo).is_none());
    repo.set_config("gerrit.reviewUrl", "https://r.example.com/");
    assert_eq!(
        link_base(&repo.repo).as_deref(),
        Some("https://r.example.com")
    );
}

#[test]
fn for_message_stamps_once() {
    let repo = TestRepo::new();
    let wd = repo.workdir();
    let stamped = for_message(&repo.repo, &wd, "Fix bug", None).unwrap();
    assert!(from_message(&stamped).is_some());
    assert_eq!(
        for_message(&repo.repo, &wd, &stamped, None).unwrap(),
        stamped
    );
}

#[test]
fn for_message_leaves_autosquash_and_disabled_alone() {
    let repo = TestRepo::new();
    let wd = repo.workdir();
    assert_eq!(
        for_message(&repo.repo, &wd, "fixup! Fix bug", None).unwrap(),
        "fixup! Fix bug"
    );
    assert_eq!(
        for_message(&repo.repo, &wd, "squash! Fix bug", None).unwrap(),
        "squash! Fix bug"
    );
    // The hook's rule is any lowercase word: `amend!` skips, `Fixup!` does not.
    assert_eq!(
        for_message(&repo.repo, &wd, "amend! Fix bug", None).unwrap(),
        "amend! Fix bug"
    );
    assert!(
        for_message(&repo.repo, &wd, "Fixup! Fix bug", None)
            .unwrap()
            .starts_with("Fixup! Fix bug\n\nChange-Id: I")
    );
    // Left empty so git keeps rejecting an empty message.
    assert_eq!(for_message(&repo.repo, &wd, "  \n", None).unwrap(), "  \n");
    repo.set_config("loom.changeId", "false");
    assert_eq!(
        for_message(&repo.repo, &wd, "Fix bug", None).unwrap(),
        "Fix bug"
    );
}

#[test]
fn for_message_keeps_a_given_id_and_writes_link_form() {
    let repo = TestRepo::new();
    let wd = repo.workdir();
    let kept = for_message(&repo.repo, &wd, "Reworded", Some(ID)).unwrap();
    assert_eq!(from_message(&kept).as_deref(), Some(ID));
    repo.set_config("loom.changeId", "false");
    assert_eq!(
        for_message(&repo.repo, &wd, "Reworded", Some(ID)).unwrap(),
        kept
    );
    repo.set_config("gerrit.reviewUrl", "https://r.example.com");
    let linked = for_message(&repo.repo, &wd, "Reworded", Some(ID)).unwrap();
    assert!(
        linked.contains(&format!("Link: https://r.example.com/id/{ID}")),
        "{linked}"
    );
}

#[test]
fn ensure_on_head_amends_the_message_only() {
    let repo = TestRepo::new();
    repo.commit("Plain commit", "a.txt");
    let before = repo.head_commit();
    let tree = before.tree_id();
    repo.write_file("b.txt", "staged");
    repo.stage_files(&["b.txt"]);

    ensure_on_head(&repo.repo, &repo.workdir(), None).unwrap();

    let after = repo.head_commit();
    assert_ne!(after.id(), before.id());
    assert_eq!(after.tree_id(), tree, "staged file must not be folded in");
    let msg = after.message().unwrap();
    assert!(msg.starts_with("Plain commit\n\nChange-Id: I"), "{msg}");
    assert!(repo.status_porcelain().contains("A  b.txt"));
}

#[test]
fn ensure_on_head_keeps_the_requested_id() {
    let repo = TestRepo::new();
    repo.commit("Reworded", "a.txt");
    ensure_on_head(&repo.repo, &repo.workdir(), Some(ID)).unwrap();
    assert_eq!(
        from_message(repo.head_commit().message().unwrap()).as_deref(),
        Some(ID)
    );
}

#[test]
fn ensure_on_head_is_a_no_op_when_present_or_disabled() {
    let repo = TestRepo::new();
    repo.commit(&format!("Has one\n\nChange-Id: {ID}\n"), "a.txt");
    let oid = repo.head_oid();
    ensure_on_head(&repo.repo, &repo.workdir(), None).unwrap();
    assert_eq!(repo.head_oid(), oid);

    repo.commit("Plain", "b.txt");
    let oid = repo.head_oid();
    repo.set_config("loom.changeId", "false");
    ensure_on_head(&repo.repo, &repo.workdir(), None).unwrap();
    assert_eq!(repo.head_oid(), oid);
    // A kept id is never dropped, even with generation disabled.
    ensure_on_head(&repo.repo, &repo.workdir(), Some(ID)).unwrap();
    assert_eq!(
        from_message(repo.head_commit().message().unwrap()).as_deref(),
        Some(ID)
    );
}
