use crate::core::diff;
use crate::core::test_helpers::TestRepo;
use crate::git;
use crate::tui::hunk_selector::{FileEntry, HunkEntry, HunkOrigin};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a test repo with a committed file containing multiple sections,
/// then modify it to produce two distinct hunks in the diff.
fn setup_two_hunk_file() -> TestRepo {
    let repo = TestRepo::new();
    // Initial content with enough context lines to produce separate hunks.
    let initial = "line 1\nline 2\nline 3\nline 4\nline 5\n\
                   line 6\nline 7\nline 8\nline 9\nline 10\n\
                   line 11\nline 12\nline 13\nline 14\nline 15\n";
    repo.write_file("file.txt", initial);
    repo.stage_files(&["file.txt"]);
    repo.commit_staged("initial");

    // Modify two distant regions to produce two separate hunks.
    let modified = "line 1\nMODIFIED TOP\nline 3\nline 4\nline 5\n\
                    line 6\nline 7\nline 8\nline 9\nline 10\n\
                    line 11\nline 12\nline 13\nMODIFIED BOTTOM\nline 15\n";
    repo.write_file("file.txt", modified);
    repo
}

/// Parse hunks from a file's diff against HEAD.
fn get_hunks(repo: &TestRepo, path: &str) -> Vec<diff::DiffHunk> {
    let workdir = repo.workdir();
    let raw = git::diff_head_file(workdir.as_path(), path).unwrap();
    diff::parse_hunks(&raw)
}

/// Build a FileEntry from parsed hunks with all hunks selected (unstaged origin).
fn file_entry_all_selected(path: &str, hunks: Vec<diff::DiffHunk>) -> FileEntry {
    FileEntry {
        path: path.to_string(),
        hunks: hunks
            .into_iter()
            .map(|h| HunkEntry {
                hunk: h,
                selected: true,
                origin: HunkOrigin::Unstaged,
            })
            .collect(),
        index_status: ' ',
        worktree_status: 'M',
        binary: false,
    }
}

/// Check if the staged diff contains a specific string.
fn staged_diff_contains(repo: &TestRepo, path: &str, needle: &str) -> bool {
    let workdir = repo.workdir();
    let cached = git::diff_cached_files(workdir.as_path(), &[path]).unwrap();
    cached.contains(needle)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Build a patch from one of two hunks, apply it, verify only that hunk is staged.
#[test]
fn patch_build_single_hunk() {
    let repo = setup_two_hunk_file();
    let hunks = repo.in_dir(|| get_hunks(&repo, "file.txt"));
    assert!(hunks.len() >= 2, "expected 2 hunks, got {}", hunks.len());

    // Select only the first hunk.
    let entry = FileEntry {
        path: "file.txt".to_string(),
        hunks: hunks
            .into_iter()
            .enumerate()
            .map(|(i, h)| HunkEntry {
                hunk: h,
                selected: i == 0,
                origin: HunkOrigin::Unstaged,
            })
            .collect(),
        index_status: ' ',
        worktree_status: 'M',
        binary: false,
    };

    // Build and apply patch.
    let selected: Vec<_> = entry
        .hunks
        .iter()
        .filter(|h| h.selected)
        .map(|h| &h.hunk)
        .collect::<Vec<_>>();
    let patch = diff::build_hunk_patch("file.txt", &selected);

    repo.in_dir(|| {
        git::apply_cached_patch(repo.workdir().as_path(), &patch).unwrap();
    });

    // First hunk should be staged, second should not.
    let has_top = repo.in_dir(|| staged_diff_contains(&repo, "file.txt", "MODIFIED TOP"));
    let has_bottom = repo.in_dir(|| staged_diff_contains(&repo, "file.txt", "MODIFIED BOTTOM"));
    assert!(has_top, "first hunk should be staged");
    assert!(!has_bottom, "second hunk should not be staged");
}

/// Two files changed, select hunks from both, apply patch, verify both partially staged.
#[test]
fn patch_build_multi_file() {
    let repo = TestRepo::new();
    // Create two files.
    let content_a = "a1\na2\na3\na4\na5\na6\na7\na8\na9\na10\n";
    let content_b = "b1\nb2\nb3\nb4\nb5\nb6\nb7\nb8\nb9\nb10\n";
    repo.write_file("a.txt", content_a);
    repo.write_file("b.txt", content_b);
    repo.stage_files(&["a.txt", "b.txt"]);
    repo.commit_staged("initial two files");

    // Modify both.
    let modified_a = "CHANGED_A\na2\na3\na4\na5\na6\na7\na8\na9\na10\n";
    let modified_b = "CHANGED_B\nb2\nb3\nb4\nb5\nb6\nb7\nb8\nb9\nb10\n";
    repo.write_file("a.txt", modified_a);
    repo.write_file("b.txt", modified_b);

    let hunks_a = repo.in_dir(|| get_hunks(&repo, "a.txt"));
    let hunks_b = repo.in_dir(|| get_hunks(&repo, "b.txt"));

    let entry_a = file_entry_all_selected("a.txt", hunks_a);
    let entry_b = file_entry_all_selected("b.txt", hunks_b);

    // Build combined patch.
    let mut combined = String::new();
    for entry in [&entry_a, &entry_b] {
        let hunks: Vec<_> = entry.hunks.iter().map(|h| &h.hunk).collect();
        combined.push_str(&diff::build_hunk_patch(&entry.path, &hunks));
    }

    repo.in_dir(|| {
        git::apply_cached_patch(repo.workdir().as_path(), &combined).unwrap();
    });

    let has_a = repo.in_dir(|| staged_diff_contains(&repo, "a.txt", "CHANGED_A"));
    let has_b = repo.in_dir(|| staged_diff_contains(&repo, "b.txt", "CHANGED_B"));
    assert!(has_a, "a.txt change should be staged");
    assert!(has_b, "b.txt change should be staged");
}

/// All hunks deselected — no patch should be generated or applied.
#[test]
fn patch_empty_selection() {
    let repo = setup_two_hunk_file();
    let hunks = repo.in_dir(|| get_hunks(&repo, "file.txt"));

    let entry = FileEntry {
        path: "file.txt".to_string(),
        hunks: hunks
            .into_iter()
            .map(|h| HunkEntry {
                hunk: h,
                selected: false, // all deselected
                origin: HunkOrigin::Unstaged,
            })
            .collect(),
        index_status: ' ',
        worktree_status: 'M',
        binary: false,
    };

    let selected: Vec<_> = entry.hunks.iter().filter(|h| h.selected).collect();
    assert!(selected.is_empty(), "no hunks should be selected");

    // No patch to apply — verify nothing is staged.
    let status = repo.status_porcelain();
    assert!(
        !status.contains("M  file.txt") && !status.starts_with("M "),
        "nothing should be staged; status: {}",
        status
    );
}

/// All hunks selected — full diff should be staged.
#[test]
fn patch_all_hunks_selected() {
    let repo = setup_two_hunk_file();
    let hunks = repo.in_dir(|| get_hunks(&repo, "file.txt"));
    assert!(hunks.len() >= 2);

    let entry = file_entry_all_selected("file.txt", hunks);

    let hunks: Vec<_> = entry.hunks.iter().map(|h| &h.hunk).collect();
    let patch = diff::build_hunk_patch("file.txt", &hunks);

    repo.in_dir(|| {
        git::apply_cached_patch(repo.workdir().as_path(), &patch).unwrap();
    });

    let has_top = repo.in_dir(|| staged_diff_contains(&repo, "file.txt", "MODIFIED TOP"));
    let has_bottom = repo.in_dir(|| staged_diff_contains(&repo, "file.txt", "MODIFIED BOTTOM"));
    assert!(has_top, "top hunk should be staged");
    assert!(has_bottom, "bottom hunk should be staged");
}

/// Binary file changes should be detected and excluded.
#[test]
fn binary_file_detected() {
    let repo = TestRepo::new();
    // Create a text file and commit.
    repo.write_file("img.bin", "initial");
    repo.stage_files(&["img.bin"]);
    repo.commit_staged("add binary");

    // Write actual binary content (null bytes make git treat it as binary).
    let binary_content = "PNG\x00\x01\x02\x03\x7f\x7e\x00binary data";
    repo.write_file("img.bin", binary_content);

    let is_binary =
        repo.in_dir(|| git::diff_head_file_is_binary(repo.workdir().as_path(), "img.bin").unwrap());
    assert!(
        is_binary,
        "file with null bytes should be detected as binary"
    );
}
