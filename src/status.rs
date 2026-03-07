use anyhow::Result;

use crate::{git, graph};

pub fn run(show_files: bool, context: usize, theme: graph::Theme) -> Result<()> {
    let repo = git::open_repo()?;
    let _ = git::require_workdir(&repo, "display status")?;

    let opts = graph::default_render_opts(theme);
    let info = git::gather_repo_info(&repo, show_files, context)?;
    let output = graph::render(info, &opts);
    print!("{}", output);
    Ok(())
}
