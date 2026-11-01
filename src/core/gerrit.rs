//! What Gerrit holds for each feature branch, saved for the status remote
//! indicator (Specs 001 and 010).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use git2::{Oid, Repository};

use crate::core::repo::RepoInfo;
use crate::core::trace_log;
use crate::git;

/// `branch.<name>.<key>`: the current patch set revision Gerrit holds for the
/// branch, [`CLOSED`], or [`STALE`].
pub const REVISION_KEY: &str = "loomGerritRevision";

/// The [`REVISION_KEY`] value for a change merged or abandoned on Gerrit.
pub const CLOSED: &str = "closed";

/// The [`REVISION_KEY`] value for a branch whose newest commits were never
/// uploaded above a closed change: no revision on the server is its tip.
pub const STALE: &str = "stale";

/// A feature branch's own Change-Ids.
#[derive(Debug, PartialEq, Eq)]
pub struct BranchChanges<'a> {
    pub name: &'a str,
    pub tip_id: Option<&'a str>,
    /// From the tip down through the commits above the base. A stacked
    /// branch stops at the tip of the one below, whose changes are not its own.
    pub ids: Vec<&'a str>,
}

/// The [`BranchChanges`] of every branch in `info`.
pub fn branch_change_ids(info: &RepoInfo) -> Vec<BranchChanges<'_>> {
    let commits: HashMap<Oid, (Option<Oid>, Option<&str>)> = info
        .commits
        .iter()
        .map(|c| (c.oid, (c.parent_oid, c.change_id.as_deref())))
        .collect();
    let tips: HashSet<Oid> = info.branches.iter().map(|b| b.tip_oid).collect();
    info.branches
        .iter()
        .map(|b| {
            let mut ids = Vec::new();
            let mut oid = Some(b.tip_oid);
            while let Some((parent, id)) = oid.and_then(|o| commits.get(&o)) {
                ids.extend(*id);
                oid = parent.filter(|p| !tips.contains(p));
            }
            let tip_id = commits.get(&b.tip_oid).and_then(|(_, id)| *id);
            BranchChanges {
                name: &b.name,
                tip_id,
                ids,
            }
        })
        .collect()
}

/// The config key holding what Gerrit holds for `branch`.
pub(crate) fn key(branch: &str) -> String {
    format!("branch.{branch}.{REVISION_KEY}")
}

/// Save `revision` as what Gerrit holds for `branch`.
pub fn save(workdir: &Path, branch: &str, revision: &str) -> Result<()> {
    git::run_git(workdir, &["config", &key(branch), revision])
}

/// What a Gerrit query says about one change.
#[derive(Debug, PartialEq, Eq)]
pub enum Change {
    Open(String),
    Closed,
}

/// The Gerrit server `remote` pushes to, when it does so over OpenSSH: the
/// query passes OpenSSH options.
pub fn ssh_server(repo: &Repository, remote: &str) -> Option<SshServer> {
    if !ssh_program(repo).is_openssh(repo) {
        return None;
    }
    let remote = repo.find_remote(remote).ok()?;
    SshServer::parse(remote.pushurl().ok().flatten().or(remote.url().ok())?)
}

/// Ask `server` what it holds for the Change-Ids of `branches` on
/// `target_branch`, keyed by Change-Id.
pub fn ask(
    repo: &Repository,
    server: &SshServer,
    target_branch: &str,
    branches: &[BranchChanges],
) -> Result<HashMap<String, Change>> {
    let wanted: BTreeSet<&str> = branches
        .iter()
        .flat_map(|b| b.ids.iter().copied())
        .collect();
    server.query(ssh_program(repo).command(), target_branch, &wanted)
}

/// Save what `changes` say about each of `branches`. A branch whose
/// commits the server knows none of is forgotten.
pub fn save_all(
    repo: &Repository,
    workdir: &Path,
    branches: &[BranchChanges],
    changes: &HashMap<String, Change>,
) -> Result<()> {
    let config = repo.config()?;
    for branch in branches {
        let name = branch.name;
        let value = saved_value(&branch.ids, branch.tip_id, changes);
        let current = config.get_string(&key(name)).ok();
        // Only what changed: most branches keep their value from run to run.
        match value {
            _ if current.as_deref() == value => {}
            Some(value) => save(workdir, name, value)?,
            None => git::run_git(workdir, &["config", "--unset", &key(name)])?,
        }
    }
    Ok(())
}

/// The [`REVISION_KEY`] value for a branch whose Change-Ids, from its tip
/// down, are `ids`: from the topmost change the server knows.
pub(crate) fn saved_value<'a>(
    ids: &[&str],
    tip_id: Option<&str>,
    changes: &'a HashMap<String, Change>,
) -> Option<&'a str> {
    let (id, change) = ids
        .iter()
        .find_map(|id| changes.get(*id).map(|change| (*id, change)))?;
    Some(match change {
        Change::Open(revision) => revision,
        Change::Closed if Some(id) == tip_id => CLOSED,
        Change::Closed => STALE,
    })
}

/// A Gerrit server reached by its ssh command line interface.
#[derive(Debug, PartialEq, Eq)]
pub struct SshServer {
    /// `[user@]host`.
    pub destination: String,
    pub port: Option<String>,
    pub project: String,
}

impl SshServer {
    /// From an `ssh://[user@]host[:port]/project` or `[user@]host:project` URL.
    pub fn parse(url: &str) -> Option<SshServer> {
        let ssh_url = ["ssh://", "git+ssh://", "ssh+git://"]
            .iter()
            .find_map(|scheme| url.strip_prefix(scheme));
        let (authority, path) = if let Some(rest) = ssh_url {
            rest.split_once('/')?
        } else if !url.contains("://") {
            // Git's rule for scp-style: a colon before any slash, and no drive letter.
            url.split_once(':')
                .filter(|(host, _)| host.len() > 1 && !host.contains('/'))?
        } else {
            return None;
        };
        let (destination, port) = match authority.rsplit_once(':') {
            Some((dest, port)) => (dest, Some(port.to_string())),
            None => (authority, None),
        };
        let path = path.trim_matches('/');
        let project = path.strip_suffix(".git").unwrap_or(path);
        (!destination.is_empty() && !project.is_empty()).then(|| SshServer {
            destination: destination.to_string(),
            port,
            project: project.to_string(),
        })
    }

    /// One `gerrit query` through `ssh` for all of `ids` on `branch`, keyed by
    /// Change-Id.
    fn query(
        &self,
        mut ssh: Command,
        branch: &str,
        ids: &BTreeSet<&str>,
    ) -> Result<HashMap<String, Change>> {
        let changes = format!(
            "({})",
            ids.iter()
                .map(|id| format!("change:{id}"))
                .collect::<Vec<_>>()
                .join(" OR ")
        );
        // Never prompt: a passphrase or host key question would land under the
        // spinner or the TUI (Spec 020). Never hang on a host out of reach.
        let mut args: Vec<String> = [
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "ServerAliveInterval=10",
            "-o",
            "ServerAliveCountMax=3",
        ]
        .map(String::from)
        .to_vec();
        if let Some(port) = &self.port {
            args.extend(["-p".into(), port.clone()]);
        }
        // `--`: a host starting with `-` is not an option.
        args.extend([
            "--".into(),
            self.destination.clone(),
            "gerrit".into(),
            "query".into(),
            "--format=JSON".into(),
            "--current-patch-set".into(),
            format!("project:{}", self.project),
            format!("branch:{branch}"),
            changes,
        ]);

        ssh.args(&args)
            // ssh forwards its stdin to the remote command: it would eat the
            // TUI's keystrokes.
            .stdin(Stdio::null());
        let program = ssh.get_program().to_string_lossy().into_owned();
        let all_args: Vec<_> = ssh.get_args().map(|a| a.to_string_lossy()).collect();
        let all_args = all_args.join(" ");

        let start = Instant::now();
        let output = ssh
            .output()
            .context("Could not run the ssh command git uses")?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        trace_log::log_command(
            &program,
            &all_args,
            start.elapsed().as_millis(),
            output.status.success(),
            &stderr,
        );
        if !output.status.success() {
            bail!(
                "The Gerrit query failed (run `loom trace` for the ssh error)\n\
                 If ssh needed to prompt: it cannot here, so a key needs ssh-agent and a new host a known_hosts entry"
            );
        }
        parse_query_output(&String::from_utf8_lossy(&output.stdout))
    }
}

/// The ssh program `git push` would run: `GIT_SSH_COMMAND`, `core.sshCommand`,
/// `GIT_SSH`, else `ssh`.
fn ssh_program(repo: &Repository) -> SshProgram {
    let shell_command = std::env::var("GIT_SSH_COMMAND")
        .ok()
        .filter(|cmd| !cmd.is_empty())
        .or_else(|| repo.config().ok()?.get_string("core.sshCommand").ok());
    if let Some(cmd) = shell_command {
        return SshProgram::Shell(cmd);
    }
    match std::env::var("GIT_SSH").ok().filter(|ssh| !ssh.is_empty()) {
        Some(ssh) => SshProgram::Path(ssh),
        None => SshProgram::Path("ssh".into()),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SshProgram {
    /// A command line, with arguments, that git hands to the shell.
    Shell(String),
    Path(String),
}

impl SshProgram {
    /// Whether git would take it for OpenSSH, as `ssh.variant` (or
    /// `GIT_SSH_VARIANT`) or else the program's name says. Git probes an
    /// unknown name with `-G`; loom takes it for not OpenSSH.
    fn is_openssh(&self, repo: &Repository) -> bool {
        let variant = std::env::var("GIT_SSH_VARIANT")
            .ok()
            .or_else(|| repo.config().ok()?.get_string("ssh.variant").ok());
        // Git's mapping: any value it does not list means OpenSSH.
        match variant.as_deref() {
            None | Some("auto") => self.is_named_ssh(),
            Some("plink" | "putty" | "tortoiseplink" | "simple") => false,
            Some(_) => true,
        }
    }

    pub(crate) fn is_named_ssh(&self) -> bool {
        let program = match self {
            SshProgram::Shell(cmd) => cmd.split_whitespace().next().unwrap_or(""),
            SshProgram::Path(path) => path,
        };
        let name = Path::new(program)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        let name = name.strip_suffix(".exe").unwrap_or(name);
        name.eq_ignore_ascii_case("ssh")
    }

    /// As git runs it: through git itself, as an alias, so the program is
    /// found on git's PATH and a command line gets git's shell rules. Git for
    /// Windows puts its own `ssh` and `sh` on that PATH only.
    fn command(&self) -> Command {
        let alias = match self {
            SshProgram::Shell(cmd) => cmd.clone(),
            SshProgram::Path(program) => format!("'{}'", program.replace('\'', r"'\''")),
        };
        let mut command = Command::new("git");
        command.args(["-c", &format!("alias.{ALIAS}=!{alias}"), ALIAS]);
        command
    }
}

/// Unlikely as a name: git runs a `git-<name>` program on PATH instead of
/// the alias.
const ALIAS: &str = "loom-gerrit-ssh";

/// The changes in `gerrit query --format=JSON --current-patch-set` output.
/// An error row, a truncated answer, or one cut short is an error: Gerrit
/// exits 0 for the first two, and reading any as "no change known" would
/// forget saved state.
pub(crate) fn parse_query_output(stdout: &str) -> Result<HashMap<String, Change>> {
    let mut changes = HashMap::new();
    let mut complete = false;
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let row: serde_json::Value =
            serde_json::from_str(line).context("Gerrit's answer to the query is not JSON")?;
        match row.get("type").and_then(|t| t.as_str()) {
            Some("error") => bail!(
                "Gerrit refused the query: {}",
                row.get("message").and_then(|m| m.as_str()).unwrap_or("")
            ),
            Some("stats") => {
                if row.get("moreChanges").and_then(|m| m.as_bool()) == Some(true) {
                    bail!("Gerrit truncated the answer to the query");
                }
                // Gerrit ends every answer with it.
                complete = true;
                continue;
            }
            _ => {}
        }
        let Some(id) = row.get("id").and_then(|id| id.as_str()) else {
            continue;
        };
        let state = match row.get("status").and_then(|s| s.as_str()) {
            Some("MERGED" | "ABANDONED") => Change::Closed,
            _ => match row["currentPatchSet"]["revision"].as_str() {
                Some(revision) => Change::Open(revision.to_string()),
                None => continue,
            },
        };
        changes.insert(id.to_string(), state);
    }
    if !complete {
        bail!("Gerrit's answer to the query was cut short");
    }
    Ok(changes)
}

#[cfg(test)]
#[path = "gerrit_test.rs"]
mod tests;
