/// Tokens a git-mirroring command received, split into the parts loom resolves
/// itself and the parts forwarded verbatim to git.
///
/// `loom show` and `loom diff` delegate rendering to git, so any option loom
/// does not define is passed straight through. Options that take a value must
/// use the attached (`-U5`) or `=` (`--unified=5`) form — loom keeps no table of
/// git options, so a detached value would be mistaken for a target.
#[derive(Debug, Default, PartialEq)]
pub struct GitArgs {
    /// Options loom does not define, forwarded to git in the order given.
    pub options: Vec<String>,
    /// Non-option tokens: short IDs, hashes, ranges, paths.
    pub targets: Vec<String>,
    /// Loom's own flags found among the tokens. clap misses these when they
    /// follow a positional value, so the command re-applies them.
    pub loom_flags: Vec<String>,
    /// Everything after a literal `--`, forwarded verbatim after a `--`.
    pub pathspec: Vec<String>,
}

/// Split raw CLI tokens into what loom resolves and what git receives.
///
/// `loom_flags` lists the flags the command owns (e.g. `--staged` for `diff`).
/// They are reported back instead of being forwarded, because clap swallows
/// them into the positional list when they appear after a positional value.
pub fn split(tokens: &[String], loom_flags: &[&str]) -> GitArgs {
    let mut out = GitArgs::default();
    let mut iter = tokens.iter();

    while let Some(token) = iter.next() {
        if token == "--" {
            out.pathspec.extend(iter.cloned());
            break;
        }
        if !is_option(token) {
            out.targets.push(token.clone());
            continue;
        }
        let name = token.split_once('=').map_or(token.as_str(), |(n, _)| n);
        if loom_flags.contains(&name) {
            out.loom_flags.push(token.clone());
        } else {
            out.options.push(token.clone());
        }
    }

    out
}

/// Whether a token looks like an option. A bare `-` is a value (stdin), not an
/// option, and neither is anything that doesn't start with a hyphen.
fn is_option(token: &str) -> bool {
    token.starts_with('-') && token.len() > 1
}

/// The hint shown when a token that was meant as an option value ends up being
/// treated as a target.
pub const VALUE_HINT: &str =
    "Options taking a value must be attached: `-U5` or `--unified=5`, not `-U 5`.";

#[cfg(test)]
#[path = "cli_args_test.rs"]
mod tests;
