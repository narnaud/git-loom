use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::Result;

use super::{literal_pathspec, literal_pathspecs, run_git_stdout, run_git_stdout_raw};

/// Flags forced on every diff loom reads back, for display or otherwise.
///
/// `--no-color` rather than `-c color.ui=false`: an explicit `color.diff=always`
/// beats `color.ui`. An external diff driver prints whatever it likes, which is
/// not a patch.
const DISPLAY: &[&str] = &["--no-color", "--no-ext-diff"];

/// Flags forced on every diff loom parses or hands back to `git apply`.
///
/// A textconv filter rewrites a binary file's patch into text `git apply`
/// cannot apply, `diff.context=0` produces hunks it refuses without
/// `--unidiff-zero`, and `diff.submodule=diff` inlines a submodule's patch.
/// `--full-index` spells out the blob ids a three-way apply needs to look up,
/// and the ones a binary diff without `--binary` is applied from (Spec 007).
const REPLAY: &[&str] = &[
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--unified=3",
    "--submodule=short",
    "--full-index",
];

/// Run `git diff` with the flags that keep its output replayable, and return it
/// decoded byte for byte ([`super::raw_text`]): it goes back to `git apply`.
fn diff_stdout(workdir: &Path, args: &[&str]) -> Result<String> {
    let mut full = vec!["diff"];
    full.extend(REPLAY);
    full.extend(args);
    run_git_stdout_raw(workdir, &full)
}

/// [`diff_stdout`] for output that is no patch — names, numstat — decoded
/// lossily like every other path loom reads: escapes would leak into argv and
/// compare unequal to the same path read elsewhere.
fn meta_stdout(workdir: &Path, args: &[&str]) -> Result<String> {
    let mut full = vec!["diff"];
    full.extend(REPLAY);
    full.extend(args);
    run_git_stdout(workdir, &full)
}

/// Run `git diff` the way [`diff_stdout`] does, plus `--binary`, and return it.
///
/// A diff saved only to be restored later needs it: without `--binary` a
/// changed binary file prints as `Binary files a/x and b/x differ`, which
/// `git apply` refuses unless both blobs are in the object store — never so for
/// uncommitted content — and since it applies a patch all-or-nothing, one such
/// file would sink the whole restore.
fn restore_stdout(workdir: &Path, args: &[&str]) -> Result<String> {
    let mut full = vec!["--binary"];
    full.extend(args);
    diff_stdout(workdir, &full)
}

fn patch_stdout(workdir: &Path, command: &str, flags: &[&str], args: &[&str]) -> Result<String> {
    let mut full = vec![command];
    full.extend(flags);
    full.extend(args);
    run_git_stdout(workdir, &full)
}

/// Get the diff for a single commit (`git diff <oid>^..<oid>`).
pub fn diff_commit(workdir: &Path, oid: &str) -> Result<String> {
    diff_stdout(workdir, &[&format!("{}^..{}", oid, oid)])
}

/// Get the diff for a single file within a commit
/// (`git diff <oid>^..<oid> -- :(literal)<path>`).
///
/// `:(literal)` for the reason [`super::ls_files`] gives: a caller moving this diff
/// whole would otherwise move whatever else the path matched as a glob.
pub fn diff_commit_file(workdir: &Path, oid: &str, path: &str) -> Result<String> {
    diff_stdout(
        workdir,
        &[&format!("{}^..{}", oid, oid), "--", &literal_pathspec(path)],
    )
}

/// The diff between two trees (`git diff --binary <from> <to>`); saved to be
/// restored, like [`diff_cached`].
pub fn diff_trees(workdir: &Path, from: &str, to: &str) -> Result<String> {
    restore_stdout(workdir, &[from, to])
}

/// Get the whole staged diff, HEAD → index (`git diff --binary --cached`);
/// saved to be restored, like [`diff_head`].
pub fn diff_cached(workdir: &Path) -> Result<String> {
    diff_cached_files(workdir, &[])
}

/// Get the staged (cached) diff, for the listed files or for all of them.
///
/// Wraps `git diff --binary --cached -- <files>`. Returns an empty string if
/// the files have no staged changes. Every caller saves this to re-stage it
/// later, so it carries binary files like [`diff_head`] does — inline, which a
/// caller that parks it in `LoomState` pays for on disk.
pub fn diff_cached_files(workdir: &Path, files: &[&str]) -> Result<String> {
    let literal = literal_pathspecs(files);
    let mut args = vec!["--cached", "--"];
    args.extend(literal.iter().map(String::as_str));
    restore_stdout(workdir, &args)
}

/// Get the diff of all tracked files against HEAD, one filename per line
/// (`git diff HEAD --name-only`).
pub fn diff_head_name_only(workdir: &Path) -> Result<String> {
    meta_stdout(workdir, &["HEAD", "--name-only"])
}

/// Get the unified diff for a single file against HEAD
/// (`git diff HEAD -- <path>`).
pub fn diff_head_file(workdir: &Path, path: &str) -> Result<String> {
    diff_stdout(workdir, &["HEAD", "--", &literal_pathspec(path)])
}

/// Whether a file's working-tree changes against HEAD are binary.
///
/// Uses `git diff --numstat HEAD -- <path>`: binary files report dashes instead
/// of counts, which is locale-independent unlike the "Binary files" string.
pub fn diff_head_file_is_binary(workdir: &Path, path: &str) -> Result<bool> {
    let out = meta_stdout(
        workdir,
        &["--numstat", "HEAD", "--", &literal_pathspec(path)],
    )?;
    Ok(out.starts_with("-\t"))
}

/// Above this many bytes of pathspecs a side is diffed whole instead: Windows
/// caps a command line at 32,767 characters.
const PATHSPEC_BUDGET: usize = 16 * 1024;

/// One side of a diff split per file, read with one `git diff` and one
/// `git diff --numstat` for all its files rather than two per file.
#[derive(Default)]
pub struct FileDiffs {
    patches: HashMap<String, String>,
    binary: HashSet<String>,
}

impl FileDiffs {
    /// What `git diff <side> -- <path>` prints; empty when `path` is unchanged,
    /// conflicted, not UTF-8 (see [`header_path`]) or was not asked for.
    pub fn patch(&self, path: &str) -> &str {
        self.patches.get(path).map_or("", String::as_str)
    }

    /// Whether git counts `path`'s change binary, which numstat reports as
    /// dashes (see [`diff_head_file_is_binary`]).
    pub fn is_binary(&self, path: &str) -> bool {
        self.binary.contains(path)
    }
}

/// Unstaged changes of `paths`, index → worktree.
///
/// `--ignore-submodules=dirty`: checking a submodule for dirt spawns a status
/// in it, and no caller reads a submodule's patch.
pub fn diff_by_file(workdir: &Path, paths: &[&str]) -> Result<FileDiffs> {
    split_by_file(workdir, &["--ignore-submodules=dirty"], paths)
}

/// Staged changes of `paths`, HEAD → index.
pub fn diff_cached_by_file(workdir: &Path, paths: &[&str]) -> Result<FileDiffs> {
    split_by_file(workdir, &["--cached"], paths)
}

/// A commit's changes to `paths` against its first parent.
pub fn diff_commit_by_file(workdir: &Path, oid: &str, paths: &[&str]) -> Result<FileDiffs> {
    split_by_file(workdir, &[&format!("{oid}^..{oid}")], paths)
}

/// [`FileDiffs`] of `paths` on one side; none, and no git run, for no paths.
///
/// The paths go in as pathspecs, so git looks at those files only, unless they
/// exceed [`PATHSPEC_BUDGET`] and it diffs the whole side. `--no-renames`
/// because a diff of several paths pairs a rename's two, or a copy's under
/// `diff.renames=copies`, where a diff of one path never would.
fn split_by_file(workdir: &Path, side: &[&str], paths: &[&str]) -> Result<FileDiffs> {
    if paths.is_empty() {
        return Ok(FileDiffs::default());
    }
    let literal = literal_pathspecs(paths);
    let mut args = vec!["--no-renames"];
    args.extend(side);
    if literal.iter().map(|p| p.len() + 1).sum::<usize>() <= PATHSPEC_BUDGET {
        args.push("--");
        args.extend(literal.iter().map(String::as_str));
    }
    let mut numstat = vec!["-z", "--numstat"];
    numstat.extend(&args);
    // Records are `<added>\t<deleted>\t<path>\0`, dashes for a binary file.
    // Read raw, so a path that is not UTF-8 is dropped as `header_path` drops it.
    let binary = diff_stdout(workdir, &numstat)?
        .split('\0')
        .filter_map(|record| record.strip_prefix("-\t-\t"))
        .filter_map(|path| String::from_utf8(super::raw_text::encode(path)).ok())
        .collect();
    Ok(FileDiffs {
        patches: split_patch(&diff_stdout(workdir, &args)?),
        binary,
    })
}

/// Each file's sections of `patch`, by path; a typechange prints two.
///
/// A section starts at a line no hunk line can be, since those start with ` `,
/// `+`, `-` or `\`. A conflicted path's sections (`diff --cc`,
/// `* Unmerged path`) are dropped: no picker lists one.
fn split_patch(patch: &str) -> HashMap<String, String> {
    let mut files: HashMap<String, String> = HashMap::new();
    let mut section: Option<(String, usize)> = None;
    let mut offset = 0;
    for line in patch.split_inclusive('\n') {
        if line.starts_with("diff ") || line.starts_with("* Unmerged path ") {
            if let Some((path, start)) = section.take() {
                files
                    .entry(path)
                    .or_default()
                    .push_str(&patch[start..offset]);
            }
            section = header_path(line).map(|path| (path, offset));
        }
        offset += line.len();
    }
    if let Some((path, start)) = section {
        files.entry(path).or_default().push_str(&patch[start..]);
    }
    files
}

/// The path a `diff --git` line names. Without renames both sides name it:
/// `a/<p> b/<p>`, each C-quoted when `<p>` needs it.
///
/// `None` for a name that is not UTF-8: loom reads paths lossily, and a lossy
/// name matches no file in a git command and may stand for several.
fn header_path(line: &str) -> Option<String> {
    let names = super::raw_text::encode(line.strip_prefix("diff --git ")?.trim_end_matches('\n'));
    let a = if names.first() == Some(&b'"') {
        unquote_c(&names)?
    } else {
        // `<p>` may hold " b/" itself, but both halves are the same length.
        let (a, b) = names.split_at(names.len().checked_sub(1)? / 2);
        if b.strip_prefix(b" b/")? != a.get(2..)? {
            return None;
        }
        a.to_vec()
    };
    String::from_utf8(a.strip_prefix(b"a/")?.to_vec()).ok()
}

/// The bytes of the C-quoted name `quoted` starts with, unquoted as git's
/// `unquote_c_style` does.
fn unquote_c(quoted: &[u8]) -> Option<Vec<u8>> {
    let mut bytes = quoted.strip_prefix(b"\"")?.iter().copied();
    let mut out = Vec::new();
    loop {
        let byte = match bytes.next()? {
            b'"' => return Some(out),
            b'\\' => match bytes.next()? {
                b'a' => 0x07,
                b'b' => 0x08,
                b'f' => 0x0c,
                b'n' => b'\n',
                b'r' => b'\r',
                b't' => b'\t',
                b'v' => 0x0b,
                c @ (b'\\' | b'"') => c,
                c @ b'0'..=b'3' => {
                    let mut value = c - b'0';
                    for _ in 0..2 {
                        let digit = bytes.next().filter(|d| (b'0'..=b'7').contains(d))?;
                        value = value << 3 | (digit - b'0');
                    }
                    value
                }
                _ => return None,
            },
            byte => byte,
        };
        out.push(byte);
    }
}

/// List files changed in a commit as `(status_char, path)` pairs
/// (`git diff -z --no-renames --name-status <oid>^..<oid>`).
///
/// `--no-renames` because a rename would list only its new path, and a caller
/// rebuilding the commit from these entries would never delete the old one.
/// `-z` because the default `core.quotePath` would otherwise escape and quote
/// a non-ASCII path into something no other git command takes.
pub fn diff_commit_name_status(workdir: &Path, oid: &str) -> Result<Vec<(char, String)>> {
    let out = meta_stdout(
        workdir,
        &[
            "-z",
            "--no-renames",
            "--name-status",
            &format!("{}^..{}", oid, oid),
        ],
    )?;
    // Records are `<status>\0<path>\0`: with renames off, never a second path.
    let mut result = Vec::new();
    let mut fields = out.split('\0');
    while let (Some(status_field), Some(path)) = (fields.next(), fields.next()) {
        if let Some(status) = status_field.chars().next() {
            result.push((status, path.to_string()));
        }
    }
    Ok(result)
}

/// Get the full unified diff of all working-tree changes against HEAD.
///
/// Wraps `git diff --binary HEAD`. This is the snapshot a rollback restores
/// the user's uncommitted changes from, so it carries binary files too.
pub fn diff_head(workdir: &Path) -> Result<String> {
    restore_stdout(workdir, &["HEAD"])
}

/// [`diff_head`] and [`diff_cached`] for the work `merge` sets aside: without
/// submodule pointers, which its `reset --hard` does not move and its replay
/// could not put back.
pub fn diff_head_without_submodules(workdir: &Path) -> Result<String> {
    restore_stdout(workdir, &["--ignore-submodules=all", "HEAD"])
}

/// See [`diff_head_without_submodules`].
pub fn diff_cached_without_submodules(workdir: &Path) -> Result<String> {
    restore_stdout(workdir, &["--ignore-submodules=all", "--cached"])
}

/// Get the unified diff for specific files against HEAD
/// (`git diff --binary HEAD -- <files>`); saved to be restored, like
/// [`diff_head`].
pub fn diff_head_files(workdir: &Path, files: &[&str]) -> Result<String> {
    let literal = literal_pathspecs(files);
    let mut args = vec!["HEAD", "--"];
    args.extend(literal.iter().map(String::as_str));
    restore_stdout(workdir, &args)
}

/// Get the working-tree diff against HEAD, for display (`git diff HEAD`).
/// Unlike [`diff_head`], keeps the user's textconv filters: the output is
/// read, never applied.
pub fn diff_head_display(workdir: &Path) -> Result<String> {
    patch_stdout(workdir, "diff", DISPLAY, &["HEAD"])
}

/// Get the staged diff against HEAD, for display (`git diff --cached`),
/// keeping the user's textconv filters.
pub fn diff_cached_display(workdir: &Path) -> Result<String> {
    patch_stdout(workdir, "diff", DISPLAY, &["--cached"])
}

/// Get the working-tree diff of one file against HEAD, for display
/// (`git diff HEAD -- <path>`), keeping the user's textconv filters.
pub fn diff_head_file_display(workdir: &Path, path: &str) -> Result<String> {
    diff_head_files_display(workdir, &[path])
}

/// Get the working-tree diff of several files against HEAD, for display
/// (`git diff HEAD -- <paths>`), in one call. No paths means no diff, not
/// every file: an empty pathspec would make git match the whole tree.
pub fn diff_head_files_display(workdir: &Path, paths: &[&str]) -> Result<String> {
    if paths.is_empty() {
        return Ok(String::new());
    }
    let literal = literal_pathspecs(paths);
    let mut args = vec!["HEAD", "--"];
    args.extend(literal.iter().map(String::as_str));
    patch_stdout(workdir, "diff", DISPLAY, &args)
}

/// Get the diff between two commits, for display (`git diff <base> <tip>`).
pub fn diff_range(workdir: &Path, base: &str, tip: &str) -> Result<String> {
    patch_stdout(workdir, "diff", DISPLAY, &[base, tip])
}

/// Get a commit's summary and patch, for display
/// (`git show --stat --patch <oid>`).
pub fn show_commit_patch(workdir: &Path, oid: &str) -> Result<String> {
    patch_stdout(workdir, "show", DISPLAY, &["--stat", "--patch", oid])
}

/// Get the patch a commit applied to one file, without the commit header, for
/// display (`git show --format= <oid> -- <path>`).
pub fn show_commit_file(workdir: &Path, oid: &str, path: &str) -> Result<String> {
    patch_stdout(
        workdir,
        "show",
        DISPLAY,
        &["--format=", oid, "--", &literal_pathspec(path)],
    )
}

#[cfg(test)]
#[path = "git_diff_test.rs"]
mod tests;
