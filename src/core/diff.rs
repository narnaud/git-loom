/// A single hunk extracted from a unified diff.
#[derive(Clone, Debug)]
pub(crate) struct DiffHunk {
    /// The raw diff text for this hunk (starting with the @@ header, ending before the next hunk
    /// or EOF).
    pub text: String,
    /// Original (pre-image) line numbers of modified/deleted lines in this hunk.
    pub modified_lines: Vec<usize>,
}

/// Parse a unified diff into individual hunks.
///
/// Each hunk starts at an `@@ -start,count +start,count @@` header and extends
/// until the next hunk header or end of input. The file headers (`--- a/` / `+++ b/`)
/// are excluded from hunk text.
pub(crate) fn parse_hunks(diff: &str) -> Vec<DiffHunk> {
    let mut hunks: Vec<DiffHunk> = Vec::new();
    let mut current_text = String::new();
    let mut current_modified: Vec<usize> = Vec::new();
    let mut current_orig_line: usize = 0;
    let mut in_hunk = false;

    for line in diff.lines() {
        if line.starts_with("@@ -") {
            // Save previous hunk if any
            if in_hunk {
                hunks.push(DiffHunk {
                    text: std::mem::take(&mut current_text),
                    modified_lines: std::mem::take(&mut current_modified),
                });
            }
            // Start new hunk
            current_text = format!("{}\n", line);
            current_modified = Vec::new();
            current_orig_line = parse_hunk_start(line).unwrap_or(1);
            in_hunk = true;
        } else if !in_hunk {
            // File header lines (--- a/, +++ b/, diff --git, etc.) — skip
            continue;
        } else if line.starts_with('-') {
            current_text.push_str(line);
            current_text.push('\n');
            current_modified.push(current_orig_line);
            current_orig_line += 1;
        } else if line.starts_with('+') {
            current_text.push_str(line);
            current_text.push('\n');
            // Added line — doesn't consume an original line number
        } else if line.starts_with('\\') {
            current_text.push_str(line);
            current_text.push('\n');
            // "\ No newline at end of file" — no line number change
        } else {
            // Context line
            current_text.push_str(line);
            current_text.push('\n');
            current_orig_line += 1;
        }
    }

    // Save last hunk
    if in_hunk {
        hunks.push(DiffHunk {
            text: current_text,
            modified_lines: current_modified,
        });
    }

    hunks
}

/// Parse a hunk header to extract the starting line number of the original side.
pub(crate) fn parse_hunk_start(line: &str) -> Option<usize> {
    let line = line.strip_prefix("@@ -")?;
    let end = line.find([',', ' '])?;
    line[..end].parse().ok()
}

/// Build a valid unified patch for `git apply` from selected hunks of a single file.
///
/// Produces a patch with one file header (`--- a/` / `+++ b/`) followed by
/// the raw text of each hunk (which includes the `@@` header).
///
/// Accepts both `&[DiffHunk]` and `&[&DiffHunk]` via `Borrow`.
pub(crate) fn build_hunk_patch(path: &str, hunks: &[impl std::borrow::Borrow<DiffHunk>]) -> String {
    let mut patch = String::new();
    patch.push_str(&format!("--- a/{}\n", path));
    patch.push_str(&format!("+++ b/{}\n", path));
    for hunk in hunks {
        patch.push_str(&hunk.borrow().text);
    }
    patch
}

#[cfg(test)]
#[path = "diff_test.rs"]
mod tests;
