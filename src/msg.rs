use colored::Colorize;

/// Print a success message with a green checkmark.
pub fn success(message: &str) {
    println!("{} {}", "✓".green(), message);
}

/// Print an error message with a red cross to stderr.
/// Additional lines are treated as hints and prefixed with a blue arrow.
pub fn error(message: &str) {
    let mut lines = message.lines();
    if let Some(first) = lines.next() {
        eprintln!("{} {}", "×".red(), first);
        for line in lines {
            eprintln!("  {} {}", "›".blue(), line);
        }
    }
}
