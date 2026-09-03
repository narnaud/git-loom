pub mod init;

use clap::ValueEnum;

/// AI agents a loom skill can be installed for.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentKind {
    /// Claude Code (installs to ~/.claude/skills/git-loom/)
    Claude,
}

impl AgentKind {
    /// Every agent kind, for scanning existing installs.
    pub const ALL: [AgentKind; 1] = [AgentKind::Claude];

    /// Human-readable name, used in messages.
    pub fn label(self) -> &'static str {
        match self {
            AgentKind::Claude => "Claude",
        }
    }

    /// The agent's configuration directory name, relative to the home
    /// directory (or the work tree root with `--project`).
    pub fn config_dir(self) -> &'static str {
        match self {
            AgentKind::Claude => ".claude",
        }
    }

    /// The line telling the user how to make the agent pick up a freshly
    /// installed skill.
    pub fn restart_hint(self) -> &'static str {
        match self {
            AgentKind::Claude => "Restart Claude Code to pick up the new skill",
        }
    }
}
