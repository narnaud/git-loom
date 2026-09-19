use super::*;
use crate::core::diff::{BINARY_ENTRY, DELETED_ENTRY, DiffHunk, SUBMODULE_ENTRY};
use crate::tui::hunk_selector::{HunkEntry, HunkOrigin};

fn entry(text: &str) -> HunkEntry {
    HunkEntry {
        hunk: DiffHunk {
            text: text.to_string(),
            modified_lines: vec![],
        },
        selected: false,
        origin: HunkOrigin::Commit,
    }
}

fn file(path: &str, texts: &[&str]) -> FileEntry {
    FileEntry {
        path: path.to_string(),
        hunks: texts.iter().map(|t| entry(t)).collect(),
        index_status: if texts == [DELETED_ENTRY] { 'D' } else { 'M' },
        worktree_status: ' ',
        binary: texts == [BINARY_ENTRY] || texts == [SUBMODULE_ENTRY],
    }
}

fn sample() -> Vec<FileEntry> {
    vec![
        file(
            "src/a.rs",
            &[
                "@@ -1,3 +1,4 @@ fn a\n+one\n",
                "@@ -20,2 +21,3 @@ fn b\n+two\n",
            ],
        ),
        file("logo.png", &[BINARY_ENTRY]),
    ]
}

const OID: &str = "1111111111111111111111111111111111111111";

fn picker(ids: &[&str], from: Option<&str>, whole_files: bool) -> Picker {
    Picker {
        hunks: HunkArgs::new(
            ids.iter().map(|s| s.to_string()).collect(),
            from.map(str::to_string),
        ),
        command: "loom split ab -m <message> -p".to_string(),
        whole_files,
        target_hash: None,
    }
}

/// `fold`'s policy: text hunks, submodules and deletions can move.
fn hunks(ids: &[&str], from: &str) -> Picker {
    picker(ids, Some(from), false)
}

#[test]
fn ids_number_from_one_within_each_file() {
    let listed = items(sample(), false);
    let ids: Vec<&str> = listed.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["src/a.rs:1", "src/a.rs:2", "logo.png:1"]);
    assert!(listed[0].diff.starts_with("@@ -1,3 +1,4 @@ fn a"));
}

#[test]
fn a_binary_entry_is_listed_but_not_selectable_for_fold() {
    let listed = items(sample(), false);
    assert!(listed[0].selectable);
    assert!(!listed[2].selectable);
}

/// `fold` moves a deletion as the commit's own whole-file diff (spec 007), so
/// refusing its id would answer the TUI's own listing with a no.
#[test]
fn a_deleted_entry_is_selectable_for_fold() {
    let listed = items(vec![file("gone.txt", &[DELETED_ENTRY])], false);
    assert!(listed[0].selectable);
}

/// `split` stages a binary or deleted file whole (spec 013), so the agent may
/// pick what the interactive picker lets a user pick.
#[test]
fn binary_and_deleted_entries_are_selectable_for_split() {
    let listed = items(sample(), true);
    assert!(listed.iter().all(|i| i.selectable));

    let mut entries = sample();
    let fp = fingerprint(OID, None, &entries);
    apply(OID, &mut entries, &picker(&["logo.png:1"], Some(&fp), true)).unwrap();
    assert!(entries[1].hunks[0].selected);
}

/// A placeholder nobody thought to classify is unmovable by default: `fold`
/// names the ones it can carry, and an empty file is not among them.
#[test]
fn an_unlisted_placeholder_is_not_selectable_for_fold() {
    let listed = items(vec![file("empty.txt", &["(empty file)"])], false);
    assert!(!listed[0].selectable);
    assert!(items(vec![file("empty.txt", &["(empty file)"])], true)[0].selectable);
}

#[test]
fn submodule_entries_stay_selectable_for_fold() {
    let listed = items(vec![file("Data", &[SUBMODULE_ENTRY])], false);
    assert!(listed[0].selectable);
}

#[test]
fn apply_marks_only_the_requested_hunks() {
    let mut entries = sample();
    let fp = fingerprint(OID, None, &entries);
    apply(OID, &mut entries, &hunks(&["src/a.rs:2"], &fp)).unwrap();
    assert!(!entries[0].hunks[0].selected);
    assert!(entries[0].hunks[1].selected);
}

/// The whole point of the fingerprint: ids are positional, so a selection
/// listed against a different diff must not move whatever now sits there.
#[test]
fn apply_refuses_a_stale_fingerprint() {
    let mut entries = sample();
    let err = apply(OID, &mut entries, &hunks(&["src/a.rs:1"], "deadbeef")).unwrap_err();
    assert!(err.to_string().contains("fingerprinted deadbeef"), "{err}");
    assert!(!entries[0].hunks[0].selected);
}

#[test]
fn apply_requires_a_fingerprint() {
    let mut entries = sample();
    let err = apply(OID, &mut entries, &picker(&["src/a.rs:1"], None, false)).unwrap_err();
    assert!(err.to_string().contains("--hunks-from"));
}

#[test]
fn fingerprint_changes_when_a_hunk_changes() {
    let before = fingerprint(OID, None, &sample());
    let mut after = sample();
    after[0].hunks[0].hunk.text.push_str("+extra\n");
    assert_ne!(before, fingerprint(OID, None, &after));
}

/// An unselectable entry still counts in the digest, so dropping it shifts the
/// numbering of nothing but must still invalidate the listing.
#[test]
fn fingerprint_covers_unselectable_entries() {
    let before = fingerprint(OID, None, &sample());
    let after = vec![sample().remove(0)];
    assert_ne!(before, fingerprint(OID, None, &after));
}

#[test]
fn apply_rejects_unknown_and_malformed_ids() {
    let mut entries = sample();
    let fp = fingerprint(OID, None, &entries);

    for id in ["src/a.rs:9", "nope.rs:1"] {
        let err = apply(OID, &mut entries, &hunks(&[id], &fp)).unwrap_err();
        assert!(err.to_string().contains("No hunk"), "{id}");
    }

    // Ids loom never emits are malformed, not missing: `:0` because they count
    // from 1, `:+1` and `: 1` because the number is plain digits.
    for id in [
        "src/a.rs",
        "src/a.rs:0",
        "src/a.rs:+1",
        "src/a.rs: 1",
        "src/a.rs:",
    ] {
        let err = apply(OID, &mut entries, &hunks(&[id], &fp)).unwrap_err();
        assert!(err.to_string().contains("Invalid hunk id"), "{id}: {err}");
    }
}

#[test]
fn apply_rejects_a_binary_id() {
    let mut entries = sample();
    let fp = fingerprint(OID, None, &entries);
    let err = apply(OID, &mut entries, &hunks(&["logo.png:1"], &fp)).unwrap_err();
    assert!(
        err.to_string().contains("cannot move `logo.png:1`"),
        "{err}"
    );
}

/// A listing from one commit must not validate against another with the same
/// diff: the ids would be replayed onto a commit they were never numbered for.
#[test]
fn fingerprint_covers_the_source_commit() {
    let other = "2222222222222222222222222222222222222222";
    assert_ne!(
        fingerprint(OID, None, &sample()),
        fingerprint(other, None, &sample())
    );
}

/// One `--hunks` per id, so a comma in a path is just part of the id.
#[test]
fn a_comma_in_a_path_is_part_of_its_id() {
    let mut entries = vec![file("a,b.txt", &["@@ -1 +1 @@\n-x\n+y\n"])];
    let fp = fingerprint(OID, None, &entries);
    apply(OID, &mut entries, &hunks(&["a,b.txt:1"], &fp)).unwrap();
    assert!(entries[0].hunks[0].selected);
}

/// `fold -p <source> <target>` re-resolves the target on the replay, so a
/// revspec that has come to name a different commit must invalidate the ids
/// rather than amend them into whatever now sits there.
#[test]
fn fingerprint_covers_the_target_commit() {
    let entries = sample();
    assert_ne!(
        fingerprint(OID, Some("aaaa"), &entries),
        fingerprint(OID, Some("bbbb"), &entries)
    );
    assert_ne!(
        fingerprint(OID, None, &entries),
        fingerprint(OID, Some("aaaa"), &entries)
    );
}

#[test]
fn has_selectable_follows_the_command_policy() {
    let binary_only = vec![file("logo.png", &[BINARY_ENTRY])];
    assert!(!has_selectable(&binary_only, false));
    assert!(has_selectable(&binary_only, true));
    assert!(has_selectable(&sample(), false));
}
