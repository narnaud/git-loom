use std::path::Path;

use anyhow::{Context, Result};
use git2::{Oid, Repository};
use serde::{Deserialize, Serialize};

use crate::core::msg;
use crate::core::repo::{self, Target, TargetKind};
use crate::core::transaction::{self, LoomState, Rollback};
use crate::core::weave::{self, RebaseOutcome, Weave};
use crate::git;

#[derive(Serialize, Deserialize)]
struct SwapContext {
    display_a: String,
    display_b: String,
}

/// Swap two commits within the same sequence.
pub fn run(a: String, b: String) -> Result<()> {
    let repo = repo::open_repo()?;

    let resolved_a = repo::resolve_arg(&repo, &a, &[TargetKind::Commit])?;
    let resolved_b = repo::resolve_arg(&repo, &b, &[TargetKind::Commit])?;

    match (resolved_a, resolved_b) {
        (Target::Commit(hash_a), Target::Commit(hash_b)) => swap_two_commits(&repo, hash_a, hash_b),
        _ => unreachable!(),
    }
}

fn swap_two_commits(repo: &Repository, hash_a: String, hash_b: String) -> Result<()> {
    let workdir = repo::require_workdir(repo, "swap")?;
    let git_dir = repo.path().to_path_buf();

    let oid_a = Oid::from_str(&hash_a)?;
    let oid_b = Oid::from_str(&hash_b)?;

    let display_a = git::short_hash(&hash_a);
    let display_b = git::short_hash(&hash_b);

    let mut graph = Weave::from_repo(repo)?;
    graph.swap_commits(oid_a, oid_b)?;

    let state = LoomState {
        command: "swap".to_string(),
        rollback: Rollback::default(),
        context: serde_json::to_value(&SwapContext {
            display_a: display_a.to_string(),
            display_b: display_b.to_string(),
        })?,
    };
    transaction::save(&git_dir, &state)?;

    let todo = graph.to_todo();
    match weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)? {
        RebaseOutcome::Completed => {
            transaction::delete(&git_dir)?;
            msg::success(&format!(
                "Swapped commits `{}` and `{}`",
                display_a, display_b
            ));
        }
        RebaseOutcome::Conflicted => {
            transaction::warn_conflict_paused("swap");
        }
    }

    Ok(())
}

/// Resume a `swap` operation after a conflict has been resolved.
pub fn after_continue(_workdir: &Path, context: &serde_json::Value) -> Result<()> {
    let ctx: SwapContext =
        serde_json::from_value(context.clone()).context("Failed to parse swap resume context")?;
    msg::success(&format!(
        "Swapped commits `{}` and `{}`",
        ctx.display_a, ctx.display_b
    ));
    Ok(())
}

#[cfg(test)]
#[path = "swap_test.rs"]
mod tests;
