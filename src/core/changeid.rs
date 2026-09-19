//! Persistent commit identity: Gerrit `Change-Id` trailers (Spec 002).

use std::path::Path;

use anyhow::{Context, Result};
use git2::Repository;

pub const TRAILER: &str = "Change-Id";
const HEX_LEN: usize = 40;
/// `git hash-object -t tree /dev/null`, what Gerrit's hook uses as `refhash`
/// on an unborn branch.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// Canonical form of a Change-Id: `I` plus 40 lowercase hex digits.
pub fn normalize(value: &str) -> Option<String> {
    let value = value.trim();
    let hex = value.strip_prefix(['I', 'i'])?;
    if hex.len() != HEX_LEN || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("I{}", hex.to_ascii_lowercase()))
}

/// The Change-Id a commit message carries, from its trailer block: the last
/// `Change-Id:` trailer, or the last `Link:` trailer ending in `/id/I<hex>`
/// (the form Gerrit's hook writes when `gerrit.reviewUrl` is set).
pub fn from_message(message: &str) -> Option<String> {
    let trailers = git2::message_trailers_strs(message).ok()?;
    let mut found = None;
    for (key, value) in trailers.iter() {
        if key.eq_ignore_ascii_case(TRAILER) {
            if let Some(id) = normalize(value) {
                found = Some(id);
            }
        } else if key.eq_ignore_ascii_case("Link")
            && let Some((_, tail)) = value.trim().rsplit_once("/id/")
            && let Some(id) = normalize(tail)
        {
            found = Some(id);
        }
    }
    found
}

/// Whether loom should add a Change-Id to the commits it creates: git config
/// `loom.changeId` (default true), also off under the hook's own opt-out,
/// `gerrit.createChangeId=false`.
pub fn enabled(repo: &Repository) -> bool {
    let Ok(config) = repo.config() else {
        return true;
    };
    if config.get_bool("loom.changeId") == Ok(false) {
        return false;
    }
    !config
        .get_string("gerrit.createChangeId")
        .map(|v| v.eq_ignore_ascii_case("false"))
        .unwrap_or(false)
}

/// `gerrit.reviewUrl` without its trailing slash, when set: the hook then
/// writes a `Link:` trailer instead of `Change-Id:`.
pub fn link_base(repo: &Repository) -> Option<String> {
    let url = repo.config().ok()?.get_string("gerrit.reviewUrl").ok()?;
    let url = url.trim().trim_end_matches('/');
    (!url.is_empty()).then(|| url.to_string())
}

/// A fresh Change-Id for `message`, by the recipe of Gerrit's current
/// `commit-msg` hook: the blob hash of
/// `<git var GIT_COMMITTER_IDENT>\n<refhash>\n<message>`, `refhash` being
/// HEAD or the empty tree. The value is opaque (Gerrit checks only its
/// shape), so after an editor-path commit HEAD is the new commit itself.
/// `git var` is asked, not libgit2, so `GIT_COMMITTER_*` overrides count.
pub fn generate(repo: &Repository, workdir: &Path, message: &str) -> Result<String> {
    let refhash = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .map(|oid| oid.to_string())
        .unwrap_or_else(|| EMPTY_TREE.to_string());
    generate_with(workdir, &refhash, message)
}

/// A fresh Change-Id for `message` that differs from `other`: the hook's
/// recipe with `other` in place of the ref hash. For the second of two
/// commits stamped from the same text before either exists (Spec 013).
pub fn generate_unlike(workdir: &Path, message: &str, other: &str) -> Result<String> {
    generate_with(workdir, other, message)
}

fn generate_with(workdir: &Path, refhash: &str, message: &str) -> Result<String> {
    let ident = crate::git::run_git_stdout(workdir, &["var", "GIT_COMMITTER_IDENT"])
        .context("Could not read the committer identity")?;
    Ok(generate_from(ident.trim_end(), refhash, message))
}

/// The hook's hash, given its three inputs; `ident` without its newline.
pub(crate) fn generate_from(ident: &str, refhash: &str, message: &str) -> String {
    let mut input = format!("{ident}\n{refhash}\n{message}");
    // The hook reads the message file, which git ends with a newline.
    if !input.ends_with('\n') {
        input.push('\n');
    }
    let oid = git2::Oid::hash_object(git2::ObjectType::Blob, input.as_bytes())
        .expect("hashing a blob cannot fail");
    format!("I{oid}")
}

/// The trailer line carrying `change_id`.
pub fn trailer_line(change_id: &str, link_base: Option<&str>) -> String {
    match link_base {
        Some(base) => format!("Link: {base}/id/{change_id}"),
        None => format!("{TRAILER}: {change_id}"),
    }
}

/// `message` with the trailer appended: joined to an existing trailer block,
/// otherwise as a new last paragraph. Independent of any `trailer.*` git
/// config, which is why loom never uses `git commit --trailer`.
pub fn stamp(message: &str, change_id: &str, link_base: Option<&str>) -> String {
    let body = message.trim_end();
    let has_block = git2::message_trailers_strs(&format!("{body}\n"))
        .map(|t| t.iter().next().is_some())
        .unwrap_or(false);
    let separator = if has_block { "\n" } else { "\n\n" };
    format!("{body}{separator}{}\n", trailer_line(change_id, link_base))
}

/// Whether the hook would leave this message alone: an autosquash subject,
/// any `<word>! ` start as in the hook's `grep '^[a-z][a-z]*! '`, not only
/// `fixup!`/`squash!`.
fn is_autosquash(message: &str) -> bool {
    let subject = message.lines().next().unwrap_or("");
    subject
        .split_once("! ")
        .is_some_and(|(kind, _)| !kind.is_empty() && kind.bytes().all(|b| b.is_ascii_lowercase()))
}

/// `message` ready for `git commit -m`: unchanged when it already carries a
/// Change-Id or is an autosquash (`fixup!`/`squash!`) message; otherwise
/// stamped with `keep` (the id a rewritten commit had) or, when generation is
/// enabled, a fresh one.
pub fn for_message(
    repo: &Repository,
    workdir: &Path,
    message: &str,
    keep: Option<&str>,
) -> Result<String> {
    if message.trim().is_empty() || from_message(message).is_some() || is_autosquash(message) {
        return Ok(message.to_string());
    }
    let id = match keep {
        Some(id) => id.to_string(),
        None if enabled(repo) => generate(repo, workdir, message)?,
        None => return Ok(message.to_string()),
    };
    Ok(stamp(message, &id, link_base(repo).as_deref()))
}

/// After an editor-path commit: give HEAD a Change-Id if its message lost or
/// never had one — `keep` (the id the commit had before a reword) or a fresh
/// one from the final message. Amends the message only, skipping the
/// `pre-commit`/`commit-msg` hooks that ran on the commit itself; nothing
/// staged is touched.
pub fn ensure_on_head(repo: &Repository, workdir: &Path, keep: Option<&str>) -> Result<()> {
    let head = repo.head()?.peel_to_commit()?;
    let message = head.message().unwrap_or("").to_string();
    let stamped = for_message(repo, workdir, &message, keep)?;
    if stamped == message {
        return Ok(());
    }
    crate::git::commit_amend_message_unverified(workdir, &stamped)
}

/// [`ensure_on_head`] for a commit that stands whatever happens next: a
/// failure to stamp is a warning, so the command finishes with the commit
/// rather than report an error over one that exists.
pub fn ensure_on_head_or_warn(repo: &Repository, workdir: &Path, keep: Option<&str>) {
    if let Err(e) = ensure_on_head(repo, workdir, keep) {
        crate::core::msg::warn(&format!("Commit created without a Change-Id: {e}"));
    }
}

#[cfg(test)]
#[path = "changeid_test.rs"]
mod tests;
