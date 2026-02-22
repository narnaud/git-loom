use anyhow::Result;

use crate::{git, graph};

pub fn run(show_files: bool) -> Result<()> {
    let repo = git::open_repo()?;
    let info = git::gather_repo_info(&repo, show_files)?;
    let output = graph::render(info);
    print!("{}", output);
    Ok(())
}
