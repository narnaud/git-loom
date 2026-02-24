use colored::Colorize;

/// Print a success message with a green checkmark.
pub fn success(message: &str) {
    println!("{} {}", "✓".green(), message);
}

/// Print an error message with a red cross to stderr.
pub fn error(message: &str) {
    eprintln!("{} {}", "×".red(), message);
}
