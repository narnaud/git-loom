use std::path::Path;

use anyhow::{Context, Result, bail};
use git2::{Oid, Repository};
use serde::{Deserialize, Serialize};

use crate::git::{self, Target, TargetKind};
use crate::git_commands;
use crate::msg;
use crate::transaction::{self, LoomState, Rollback};
use crate::weave::{self, RebaseOutcome, Weave};

#[derive(Serialize, Deserialize)]
struct SwapContext {
    display_a: String,
    display_b: String,
}

/// Swap two commits (within the same sequence) or two branch sections.
pub fn run(a: String, b: String) -> Result<()> {
    let repo = git::open_repo()?;

    let resolved_a = git::resolve_arg(&repo, &a, &[TargetKind::Branch, TargetKind::Commit])?;
    let resolved_b = git::resolve_arg(&repo, &b, &[TargetKind::Branch, TargetKind::Commit])?;

    match (resolved_a, resolved_b) {
        (Target::Commit(hash_a), Target::Commit(hash_b)) => swap_two_commits(&repo, hash_a, hash_b),
        (Target::Branch(name_a), Target::Branch(name_b)) => {
            swap_two_branches(&repo, name_a, name_b)
        }
        _ => bail!("Both arguments must be commits or both must be branches"),
    }
}

fn swap_two_commits(repo: &Repository, hash_a: String, hash_b: String) -> Result<()> {
    let workdir = git::require_workdir(repo, "swap")?;
    let git_dir = repo.path().to_path_buf();

    let oid_a = Oid::from_str(&hash_a)?;
    let oid_b = Oid::from_str(&hash_b)?;

    let display_a = git_commands::short_hash(&hash_a);
    let display_b = git_commands::short_hash(&hash_b);

    let mut graph = Weave::from_repo(repo)?;
    graph.swap_commits(oid_a, oid_b)?;

    let saved_head = git::head_oid(repo)?.to_string();
    let saved_refs = transaction::refs_to_strings(&git::snapshot_branch_refs(repo)?);
    let state = LoomState {
        command: "swap".to_string(),
        rollback: Rollback {
            saved_head,
            saved_refs,
            ..Default::default()
        },
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

fn swap_two_branches(repo: &Repository, name_a: String, name_b: String) -> Result<()> {
    let workdir = git::require_workdir(repo, "swap")?;
    let git_dir = repo.path().to_path_buf();

    let mut graph = Weave::from_repo(repo)?;
    graph.swap_branches(&name_a, &name_b)?;

    let saved_head = git::head_oid(repo)?.to_string();
    let saved_refs = transaction::refs_to_strings(&git::snapshot_branch_refs(repo)?);
    let state = LoomState {
        command: "swap".to_string(),
        rollback: Rollback {
            saved_head,
            saved_refs,
            ..Default::default()
        },
        context: serde_json::to_value(&SwapContext {
            display_a: name_a.clone(),
            display_b: name_b.clone(),
        })?,
    };
    transaction::save(&git_dir, &state)?;

    let todo = graph.to_todo();
    match weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)? {
        RebaseOutcome::Completed => {
            transaction::delete(&git_dir)?;
            msg::success(&format!("Swapped branches `{}` and `{}`", name_a, name_b));
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
        "Swapped `{}` and `{}`",
        ctx.display_a, ctx.display_b
    ));
    Ok(())
}

#[cfg(test)]
#[path = "swap_test.rs"]
mod tests;
