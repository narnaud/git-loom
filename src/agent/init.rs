use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::agent::AgentKind;
use crate::core::{msg, repo};

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
    let target = base.join("skills").join("git-loom").join("SKILL.md");
    let content = match agent {
        AgentKind::Claude => CLAUDE_SKILL,
    };
    install_skill(&target, content, agent.label(), agent.restart_hint())
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

#[cfg(test)]
#[path = "init_test.rs"]
mod tests;
