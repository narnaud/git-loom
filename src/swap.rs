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
        rollback: Rollback {
            // Restored whichever way the rebase ends (Spec 014).
            saved_staged_patch: git::diff_cached(workdir)?,
            ..Default::default()
        },
        context: serde_json::to_value(&SwapContext {
            display_a: display_a.to_string(),
            display_b: display_b.to_string(),
        })?,
        // A swap that silently dropped one of them is not a swap, and the
        // success message names both.
        protect: vec![hash_a.clone(), hash_b.clone()],
        targets: Vec::new(),
    };
    transaction::save(&git_dir, &state)?;

    let todo = graph.to_todo();
    let base = graph.base_oid.to_string();
    let outcome = weave::run_rebase_protecting(workdir, Some(&base), &todo, state.protected())
        .map_err(|e| transaction::roll_back_failed_rebase(workdir, &git_dir, &state, e))?;
    match outcome {
        RebaseOutcome::Completed => {
            git::restore_staged_after_rebase(workdir, &state.rollback.saved_staged_patch);
            transaction::delete(&git_dir)?;
            msg::success(&format!(
                "Swapped commits `{}` and `{}`",
                display_a, display_b
            ));
        }
        RebaseOutcome::Stopped => {
            transaction::warn_paused(workdir, "swap");
        }
        RebaseOutcome::Paused => {
            transaction::warn_paused_at_edit(Some("swap"));
        }
    }

    Ok(())
}

/// Resume a `swap` operation after a conflict has been resolved.
pub fn after_continue(
    workdir: &Path,
    rollback: &Rollback,
    context: &serde_json::Value,
) -> Result<()> {
    let ctx: SwapContext =
        serde_json::from_value(context.clone()).context("Failed to parse swap resume context")?;
    git::restore_staged_after_rebase(workdir, &rollback.saved_staged_patch);
    msg::success(&format!(
        "Swapped commits `{}` and `{}`",
        ctx.display_a, ctx.display_b
    ));
    Ok(())
}

#[cfg(test)]
#[path = "swap_test.rs"]
mod tests;
