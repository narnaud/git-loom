pub fn run(shell: String) -> Result<(), Box<dyn std::error::Error>> {
    match shell.as_str() {
        "powershell" | "pwsh" => {
            print!("{}", include_str!("git-loom.ps1"));
            Ok(())
        }
        "clink" | "cmd" => {
            print!("{}", include_str!("git-loom.lua"));
            Ok(())
        }
        _ => Err(format!(
            "Unsupported shell: '{}'. Supported shells: powershell, clink",
            shell
        )
        .into()),
    }
}
