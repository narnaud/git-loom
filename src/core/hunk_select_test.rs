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
        git_args: String::new(),
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

/// A staged working-tree hunk starts selected, and `--hunks` is the whole
/// answer, so an id left out of it comes back out of the index.
#[test]
fn apply_deselects_what_the_ids_leave_out() {
    let mut entries = sample();
    entries[0].hunks[0].selected = true;
    let fp = fingerprint(OID, None, &entries);
    apply(OID, &mut entries, &picker(&["src/a.rs:2"], Some(&fp), true)).unwrap();
    assert!(!entries[0].hunks[0].selected);
    assert!(entries[0].hunks[1].selected);
}

/// A refused selection leaves the entries alone: the caller stages from them.
#[test]
fn a_refused_selection_deselects_nothing() {
    let mut entries = sample();
    entries[0].hunks[0].selected = true;
    let fp = fingerprint(OID, None, &entries);
    apply(OID, &mut entries, &picker(&["src/a.rs:9"], Some(&fp), true)).unwrap_err();
    assert!(entries[0].hunks[0].selected);
}

#[test]
fn items_mark_an_already_staged_entry() {
    let mut entries = sample();
    entries[0].hunks[1].selected = true;
    let listed = items(entries, true);
    assert!(!listed[0].staged);
    assert!(listed[1].staged);
}

#[test]
fn a_worktree_picker_fingerprints_the_commit_it_lands_in() {
    let with_target = worktree_picker(HunkArgs::default(), String::new(), Some("aaaa"), &[]);
    let without = worktree_picker(HunkArgs::default(), String::new(), None, &[]);
    assert_ne!(
        fingerprint("", with_target.target_hash.as_deref(), &sample()),
        fingerprint("", without.target_hash.as_deref(), &sample())
    );
}

/// Built the way `collect_unstaged_hunks` builds it, so a change to the
/// synthesized header shows up here rather than silently stopping the summary.
fn whole_file_hunk(lines: usize) -> String {
    let mut text = format!("{}+1,{lines} @@\n", crate::core::diff::NEW_FILE_HEADER);
    for n in 0..lines {
        text.push_str(&format!("+line {n}\n"));
    }
    text
}

fn with_origin(mut entry: FileEntry, origin: HunkOrigin) -> FileEntry {
    for hunk in &mut entry.hunks {
        hunk.origin = origin;
    }
    entry
}

/// An untracked file's entry: the picker synthesizes its whole content so it
/// can preview it.
fn untracked(path: &str, lines: usize) -> FileEntry {
    let mut entry = file(path, &[whole_file_hunk(lines).as_str()]);
    entry.index_status = '?';
    entry.worktree_status = '?';
    with_origin(entry, HunkOrigin::Unstaged)
}

/// The same file once staged: `git diff --cached` gives its whole content back.
fn staged_new(path: &str, lines: usize) -> FileEntry {
    let mut entry = file(path, &[whole_file_hunk(lines).as_str()]);
    entry.index_status = 'A';
    with_origin(entry, HunkOrigin::Staged)
}

/// `git add -N`: in the index as an empty blob, so the whole content is one
/// unstaged hunk against it.
fn intent_to_add(path: &str, lines: usize) -> FileEntry {
    let mut entry = file(path, &[whole_file_hunk(lines).as_str()]);
    entry.index_status = 'A';
    entry.worktree_status = 'M';
    with_origin(entry, HunkOrigin::Unstaged)
}

#[test]
fn an_untracked_file_is_listed_by_size_not_by_content() {
    let listed = items(vec![untracked("new.rs", 3)], true);
    assert_eq!(listed[0].diff, "(new file, 3 line(s))");
    assert!(listed[0].selectable);

    // The fingerprint still digests the content the listing left out, so
    // editing the file invalidates the ids it was numbered for — including an
    // edit the summary cannot see, which hashing the summary would miss.
    let mut edited = untracked("new.rs", 3);
    edited.hunks[0].hunk.text = edited.hunks[0].hunk.text.replace("+line 0", "+edited");
    assert_ne!(
        fingerprint("", None, &[untracked("new.rs", 3)]),
        fingerprint("", None, std::slice::from_ref(&edited))
    );
    // Same line count, so the summary is identical: the fingerprint is what
    // notices, not the listing.
    assert_eq!(items(vec![edited], true)[0].diff, "(new file, 3 line(s))");
}

/// Once the file is in the index, its text is git's diff of the *indexed*
/// content. A clean or eol filter makes that something else than the file on
/// disk — an LFS pointer for a huge file — so there is no summary to trust.
#[test]
fn an_intent_to_add_file_is_listed_verbatim() {
    let listed = items(vec![intent_to_add("new.rs", 3)], true);
    assert!(listed[0].diff.starts_with("@@ -0,0"));
}

#[test]
fn a_staged_new_file_is_listed_verbatim() {
    let listed = items(vec![staged_new("new.rs", 3)], true);
    assert!(listed[0].diff.starts_with("@@ -0,0"));
}

/// A tracked file that was empty is filled by a `@@ -0,0` hunk too, but it is
/// not new: its content is a change to list, not a file to read.
#[test]
fn filling_a_tracked_empty_file_is_listed_verbatim() {
    let mut entry = file("empty.rs", &[whole_file_hunk(3).as_str()]);
    entry.index_status = ' ';
    entry.worktree_status = 'M';
    let entry = with_origin(entry, HunkOrigin::Unstaged);
    assert!(items(vec![entry], true)[0].diff.starts_with("@@ -0,0"));
}

fn unstaged_hunk(text: &str) -> HunkEntry {
    HunkEntry {
        hunk: DiffHunk {
            text: text.to_string(),
            modified_lines: vec![],
        },
        selected: false,
        origin: HunkOrigin::Unstaged,
    }
}

/// A staged new file deleted from the worktree: there is no file to read, so
/// the content the agent decides on stays in the listing.
#[test]
fn a_staged_new_file_gone_from_the_worktree_is_listed_verbatim() {
    let mut entry = staged_new("new.rs", 3);
    entry.worktree_status = 'D';
    entry.hunks.push(unstaged_hunk(DELETED_ENTRY));

    let listed = items(vec![entry], true);
    assert!(listed[0].diff.starts_with("@@ -0,0"));
    assert_eq!(listed[1].diff, DELETED_ENTRY);
}

#[test]
fn a_single_line_new_file_is_still_counted() {
    assert_eq!(
        items(vec![untracked("new.rs", 1)], true)[0].diff,
        "(new file, 1 line(s))"
    );
}

/// The count is the `+` lines, so a `\ No newline` marker is not one of them.
#[test]
fn a_new_file_with_no_trailing_newline_counts_its_lines_only() {
    let mut entry = untracked("new.rs", 3);
    entry.hunks[0]
        .hunk
        .text
        .push_str("\\ No newline at end of file\n");
    assert_eq!(items(vec![entry], true)[0].diff, "(new file, 3 line(s))");
}

/// Nothing about a staged file is summarized, and its worktree hunks are its
/// own choices, so every entry stays verbatim.
#[test]
fn a_staged_new_file_edited_in_the_worktree_lists_both_hunks_verbatim() {
    // An edit that drops a staged line: `line 1` is in the staged entry and
    // nowhere on disk, so a summary would point at a file that has lost it.
    for edit in [
        "@@ -3,1 +3,2 @@\n+tail\n",
        "@@ -1,3 +1,2 @@\n line 0\n-line 1\n line 2\n",
    ] {
        let mut entry = staged_new("new.rs", 3);
        entry.worktree_status = 'M';
        entry.hunks.push(unstaged_hunk(edit));

        let listed = items(vec![entry], true);
        assert!(listed[0].diff.starts_with("@@ -0,0"));
        assert_eq!(listed[1].diff, edit);
    }
}

/// A new file with no hunk to summarize keeps the placeholder that says why.
#[test]
fn an_untracked_binary_or_empty_file_keeps_its_placeholder() {
    for placeholder in [BINARY_ENTRY, "(empty file)"] {
        let mut entry = file("new.bin", &[placeholder]);
        entry.index_status = '?';
        entry.worktree_status = '?';
        let entry = with_origin(entry, HunkOrigin::Unstaged);
        assert_eq!(items(vec![entry], true)[0].diff, placeholder);
    }
}

#[test]
fn a_tracked_hunk_is_still_listed_verbatim() {
    let listed = items(sample(), true);
    assert_eq!(listed[0].diff, "@@ -1,3 +1,4 @@ fn a\n+one\n");
}

/// A commit's entry stands for content that need not be in the worktree at
/// all, so there is no file to read instead and it stays verbatim. Built the
/// way `collect_commit_hunks` builds it: the commit's name-status char, no
/// worktree side.
#[test]
fn a_new_file_in_a_commit_is_listed_verbatim() {
    let mut entry = file("new.rs", &[whole_file_hunk(3).as_str()]);
    entry.index_status = 'A';
    entry.worktree_status = ' ';
    let entry = with_origin(entry, HunkOrigin::Commit);
    assert!(items(vec![entry], false)[0].diff.starts_with("@@ -0,0"));
}

/// A working-tree file with one staged and one unstaged hunk, as listed.
fn staged_then_changed() -> Vec<FileEntry> {
    let mut f = file(
        "f.txt",
        &["@@ -3 +3 @@\n-3\n+333\n", "@@ -3 +3 @@\n-333\n+WT\n"],
    );
    f.hunks[0].origin = HunkOrigin::Staged;
    f.hunks[0].selected = true;
    f.hunks[1].origin = HunkOrigin::Unstaged;
    f.hunks[1].hunk.modified_lines = vec![3];
    vec![f]
}

/// `--hunks` unstages a staged entry left out, so a replay numbered before a
/// `git add -p` would silently undo it.
#[test]
fn fingerprint_changes_when_an_entry_gets_staged() {
    let mut entries = staged_then_changed();
    let before = fingerprint(OID, None, &entries);
    entries[0].hunks[0].origin = HunkOrigin::Unstaged;
    assert_ne!(before, fingerprint(OID, None, &entries));
}

/// Unstaging reverse-applies to the index alone: the staged `333` exists
/// nowhere else once the working tree changed it again.
#[test]
fn refuses_to_unstage_what_only_the_index_holds() {
    let mut entries = staged_then_changed();
    entries[0].hunks[0].selected = false;
    let err = refuse_losing_index_content(&entries)
        .unwrap_err()
        .to_string();
    assert!(err.contains("Keep `f.txt:1` staged"), "{err}");
}

#[test]
fn keeping_a_staged_entry_selected_is_not_refused() {
    refuse_losing_index_content(&staged_then_changed()).unwrap();
}

#[test]
fn unstaging_lines_the_working_tree_left_alone_is_not_refused() {
    let mut entries = staged_then_changed();
    entries[0].hunks[0].selected = false;
    entries[0].hunks[1].hunk = DiffHunk {
        text: "@@ -37 +37 @@\n-37\n+WT\n".to_string(),
        modified_lines: vec![37],
    };
    refuse_losing_index_content(&entries).unwrap();
}

#[test]
fn unstaging_a_staged_binary_the_working_tree_changed_is_refused() {
    let mut f = file("logo.png", &[BINARY_ENTRY, BINARY_ENTRY]);
    f.hunks[0].origin = HunkOrigin::Staged;
    f.hunks[1].origin = HunkOrigin::Unstaged;
    assert!(refuse_losing_index_content(&[f]).is_err());
}
