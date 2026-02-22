use anyhow::Result;

use crate::{git, graph};

pub fn run() -> Result<()> {
    let repo = git::open_repo()?;
    let info = git::gather_repo_info(&repo)?;
    let output = graph::render(info);
    print!("{}", output);
    Ok(())
}
