// ── Unit tests for diff parsing ──────────────────────────────────────

/// A CRLF file's `\r` is part of each line: dropped, the hunk's context no
/// longer matches the file and `git apply` refuses it.
#[test]
fn parse_hunks_keeps_crlf() {
    let diff = "--- a/f.txt\n+++ b/f.txt\n@@ -1,2 +1,2 @@\n-old\r\n+new\r\n ctx\r\n";
    let hunks = super::parse_hunks(diff);
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].text, "@@ -1,2 +1,2 @@\n-old\r\n+new\r\n ctx\r\n");
}

#[test]
fn parse_hunks_single_hunk() {
    let diff = "\
--- a/file.txt
+++ b/file.txt
@@ -3,4 +3,4 @@
 context
-old line
+new line
 context
";
    let hunks = super::parse_hunks(diff);
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].modified_lines, vec![4]);
    assert!(hunks[0].text.contains("@@ -3,4 +3,4 @@"));
    assert!(hunks[0].text.contains("-old line"));
}

#[test]
fn parse_hunks_multiple_hunks() {
    let diff = "\
--- a/file.txt
+++ b/file.txt
@@ -3,4 +3,4 @@
 context
-old line 1
+new line 1
 context
@@ -20,4 +20,4 @@
 context
-old line 2
+new line 2
 context
";
    let hunks = super::parse_hunks(diff);
    assert_eq!(hunks.len(), 2);
    assert_eq!(hunks[0].modified_lines, vec![4]);
    assert_eq!(hunks[1].modified_lines, vec![21]);
}

#[test]
fn parse_hunks_pure_addition() {
    let diff = "\
--- a/file.txt
+++ b/file.txt
@@ -3,2 +3,4 @@
 context
+added line 1
+added line 2
 context
";
    let hunks = super::parse_hunks(diff);
    assert_eq!(hunks.len(), 1);
    assert!(
        hunks[0].modified_lines.is_empty(),
        "pure additions should produce no modified original lines"
    );
}

// ── Unit tests for patch construction ────────────────────────────────

#[test]
fn build_hunk_patch_single_hunk() {
    let hunk = super::DiffHunk {
        text: "@@ -3,4 +3,4 @@\n context\n-old line\n+new line\n context\n".to_string(),
        modified_lines: vec![4],
    };
    let patch = super::build_hunk_patch("src/file.txt", &[hunk]);
    assert!(patch.starts_with("--- a/src/file.txt\n+++ b/src/file.txt\n"));
    assert!(patch.contains("@@ -3,4 +3,4 @@"));
    assert!(patch.contains("-old line"));
    assert!(patch.contains("+new line"));
}

#[test]
fn build_hunk_patch_multiple_hunks() {
    let hunk1 = super::DiffHunk {
        text: "@@ -3,4 +3,4 @@\n context\n-old1\n+new1\n context\n".to_string(),
        modified_lines: vec![4],
    };
    let hunk2 = super::DiffHunk {
        text: "@@ -20,4 +20,4 @@\n context\n-old2\n+new2\n context\n".to_string(),
        modified_lines: vec![21],
    };
    let patch = super::build_hunk_patch("src/file.txt", &[hunk1, hunk2]);
    // Should have one file header and both hunks
    assert_eq!(patch.matches("--- a/").count(), 1);
    assert_eq!(patch.matches("@@ -").count(), 2);
}

#[test]
fn parse_hunks_sql_comment_lines() {
    let diff = "\
--- a/query.sql
+++ b/query.sql
@@ -1,3 +1,3 @@
 SELECT *
--- main query
+-- updated query
 FROM users
";
    let hunks = super::parse_hunks(diff);
    assert_eq!(hunks.len(), 1);
    assert_eq!(
        hunks[0].modified_lines,
        vec![2],
        "SQL comment line starting with '-- ' must be tracked as a modified line"
    );
}

/// A placeholder in the body would corrupt its own entry and swallow the next
/// file's header.
#[test]
fn build_hunk_patch_leaves_out_a_placeholder() {
    let hunks = vec![
        super::DiffHunk {
            text: "(file deleted)".to_string(),
            modified_lines: vec![],
        },
        super::DiffHunk {
            text: "@@ -1 +1 @@\n-a\n+b\n".to_string(),
            modified_lines: vec![],
        },
    ];
    assert_eq!(
        super::build_hunk_patch("f.txt", &hunks),
        "--- a/f.txt\n+++ b/f.txt\n@@ -1 +1 @@\n-a\n+b\n"
    );
}

/// Headers with no body are still non-empty, so a caller testing
/// `!patch.is_empty()` would hand `git apply` a fragment it rejects.
#[test]
fn build_hunk_patch_is_empty_when_nothing_survives() {
    let hunks = vec![super::DiffHunk {
        text: "(file deleted)".to_string(),
        modified_lines: vec![],
    }];
    assert_eq!(super::build_hunk_patch("f.txt", &hunks), "");
}

#[test]
fn added_lines_count_the_new_side() {
    let hunk = |text: &str| super::DiffHunk {
        text: text.to_string(),
        modified_lines: vec![],
    };
    assert_eq!(hunk("@@ -3 +3 @@\n-3\n+333\n").added_lines(), [3]);
    assert_eq!(
        hunk(
            "@@ -9,4 +10,5 @@ fn a\n ctx\n-old\n+new\n+more\n ctx\n\\ No newline at end of file\n"
        )
        .added_lines(),
        [11, 12]
    );
    assert_eq!(hunk("@@ -0,0 +1 @@\n+only\n").added_lines(), [1]);
    assert_eq!(
        hunk("@@ -5,2 +4,0 @@\n-a\n-b\n").added_lines(),
        Vec::<usize>::new()
    );
    assert!(hunk(super::BINARY_ENTRY).added_lines().is_empty());
}
