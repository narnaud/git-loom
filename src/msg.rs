use colored::Colorize;

/// Replace text between backticks with yellow-colored text.
fn colorize_backticks(message: &str) -> String {
    let mut result = String::new();
    let mut rest = message;
    while let Some(start) = rest.find('`') {
        result.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(end) = after.find('`') {
            result.push_str(&format!("{}", after[..end].yellow()));
            rest = &after[end + 1..];
        } else {
            result.push_str(rest);
            return result;
        }
    }
    result.push_str(rest);
    result
}

/// Print a success message with a green checkmark.
/// Text between backticks is highlighted in yellow.
pub fn success(message: &str) {
    println!("{} {}", "✓".green(), colorize_backticks(message));
}

/// Print an error message with a red cross to stderr.
/// Additional lines are treated as hints and prefixed with a blue arrow.
/// Text between backticks is highlighted in yellow.
pub fn error(message: &str) {
    let mut lines = message.lines();
    if let Some(first) = lines.next() {
        eprintln!("{} {}", "×".red(), colorize_backticks(first));
        for line in lines {
            eprintln!("  {} {}", "›".blue(), colorize_backticks(line));
        }
    }
}
