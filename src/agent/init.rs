use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::agent::AgentKind;
use crate::core::{agent_mode, msg, repo};

/// The skill taught to the agent, embedded at build time. One constant per
/// agent kind; they may share content or diverge as agents get added.
const CLAUDE_SKILL: &str = include_str!("../../skills/git-loom/SKILL.md");

/// Install the loom skill for an AI agent (see spec 019).
///
/// Default target: `<home>/<config-dir>/skills/git-loom/SKILL.md`.
/// With `project`: relative to the work tree root instead of the home
/// directory. `dir` (tests) replaces the `<home>/<config-dir>` base entirely.
pub fn run(agent: AgentKind, project: bool, dir: Option<PathBuf>) -> Result<()> {
    let base = install_base(agent, project, dir)?;
    let target = skill_target(&base);
    let content = skill_content(agent);
    install_skill(&target, content, agent.label(), agent.restart_hint())
}

/// The skill file inside an install base directory.
fn skill_target(base: &Path) -> PathBuf {
    base.join("skills").join("git-loom").join("SKILL.md")
}

/// The skill source this binary embeds for `agent`.
fn skill_content(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Claude => CLAUDE_SKILL,
    }
}

/// Resolve the base directory the `skills/git-loom/SKILL.md` suffix is
/// appended to.
fn install_base(agent: AgentKind, project: bool, dir: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(dir) = dir {
        return Ok(dir);
    }
    if project {
        let repo = repo::open_repo()?;
        let workdir = repo::require_workdir(&repo, "agent init --project")?;
        return Ok(workdir.join(agent.config_dir()));
    }
    // Requires Rust >= 1.85, where home_dir was fixed on Windows; the
    // deprecation attribute was only removed later, hence the allow.
    #[allow(deprecated)]
    let home = std::env::home_dir().context("Could not determine the home directory")?;
    Ok(home.join(agent.config_dir()))
}

/// Write the skill file: report created / updated / already up to date.
///
/// No `--force` exists — the file is loom-owned and regenerated from the
/// binary, so refreshing it after an upgrade is the desired behavior.
fn install_skill(
    target: &Path,
    content: &str,
    agent_label: &str,
    restart_hint: &str,
) -> Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory '{}'", parent.display()))?;
    }

    match std::fs::read_to_string(target) {
        Ok(existing) if existing == content => {
            msg::success(&format!("{} skill already up to date", agent_label));
        }
        Ok(_) => {
            std::fs::write(target, content)
                .with_context(|| format!("Failed to write '{}'", target.display()))?;
            msg::success(&format!(
                "Updated {} skill at `{}`",
                agent_label,
                target.display()
            ));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::write(target, content)
                .with_context(|| format!("Failed to write '{}'", target.display()))?;
            msg::success(&format!(
                "Installed {} skill at `{}`\n{}",
                agent_label,
                target.display(),
                restart_hint
            ));
        }
        Err(e) => {
            return Err(e).with_context(|| format!("Failed to read '{}'", target.display()));
        }
    }

    Ok(())
}

/// Report installed skills that differ from the one this binary embeds.
///
/// Called from agent mode on every invocation (spec 019): an agent that
/// already loaded a stale skill has no other way to learn a newer one exists.
/// The notice goes straight into the JSON `messages` array and is never
/// printed — its only reader is the agent, which parses the JSON. Purely
/// advisory: nothing is rewritten behind the user's back, and no failure here
/// (no home directory, unreadable file, no repository) affects the command's
/// own result.
pub fn report_outdated_skills() {
    for agent in AgentKind::ALL {
        for (target, project) in installed_targets(agent) {
            if !is_outdated(agent, &target) {
                continue;
            }
            let command = if project {
                "git-loom agent init --project"
            } else {
                "git-loom agent init"
            };
            agent_mode::record_message(&format!(
                "The {} git-loom skill at `{}` differs from the one this loom ships. \
                 Run `{}` to refresh it (local edits are overwritten). {}.",
                agent.label(),
                target.display(),
                command,
                agent.restart_hint()
            ));
        }
    }
}

/// The skill locations to check: the home install, plus the in-repo one when
/// run inside a work tree. Deduplicated, since the two can coincide.
fn installed_targets(agent: AgentKind) -> Vec<(PathBuf, bool)> {
    let mut targets: Vec<(PathBuf, bool)> = Vec::new();
    for project in [false, true] {
        if let Ok(base) = install_base(agent, project, None) {
            let target = skill_target(&base);
            if !targets.iter().any(|(seen, _)| *seen == target) {
                targets.push((target, project));
            }
        }
    }
    targets
}

/// Whether the skill installed at `target` differs from the embedded one —
/// the same byte comparison `install_skill` uses to decide whether to rewrite.
///
/// A file that is absent or unreadable is not outdated: nothing is installed
/// there, and a user who never ran `agent init` is not nagged.
fn is_outdated(agent: AgentKind, target: &Path) -> bool {
    std::fs::read_to_string(target).is_ok_and(|installed| installed != skill_content(agent))
}

#[cfg(test)]
#[path = "init_test.rs"]
mod tests;
