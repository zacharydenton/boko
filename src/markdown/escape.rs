//! Pure markdown escaping utilities.
//!
//! These functions handle escaping special Markdown characters and calculating
//! appropriate fence/tick lengths for code blocks and inline code.

/// Escape special Markdown characters in text.
///
/// Escapes characters that have special meaning in Markdown:
/// - Backslash: `\\`
/// - Emphasis: `*`, `_`
/// - Links: `[`, `]`
/// - Code: `` ` ``
/// - Headings: `#` (only at line start)
/// - Tables: `|`
/// - HTML: `<`, `>`
/// - Images: `!` (when followed by `[`)
///
/// # Examples
///
/// ```
/// use boko::markdown::escape_markdown;
///
/// assert_eq!(escape_markdown("*bold*"), "\\*bold\\*");
/// assert_eq!(escape_markdown("[link]"), "\\[link\\]");
/// ```
pub fn escape_markdown(text: &str) -> String {
    let mut result = String::with_capacity(text.len() + text.len() / 10);
    let mut chars = text.chars().peekable();
    let mut at_line_start = true;

    while let Some(c) = chars.next() {
        match c {
            '\\' => result.push_str("\\\\"),
            '*' | '_' => {
                result.push('\\');
                result.push(c);
            }
            '[' | ']' => {
                result.push('\\');
                result.push(c);
            }
            '`' => {
                result.push('\\');
                result.push(c);
            }
            '#' if at_line_start => {
                result.push('\\');
                result.push(c);
            }
            '|' => {
                result.push('\\');
                result.push(c);
            }
            '<' | '>' => {
                result.push('\\');
                result.push(c);
            }
            '!' if chars.peek() == Some(&'[') => {
                result.push('\\');
                result.push(c);
            }
            _ => result.push(c),
        }
        at_line_start = c == '\n';
    }

    result
}

/// Calculate the minimum fence length needed for a code block.
///
/// Returns the smallest number of fence characters (at least 3) that
/// doesn't appear as a run in the content.
///
/// # Examples
///
/// ```
/// use boko::markdown::calculate_fence_length;
///
/// // Normal content needs 3 backticks
/// assert_eq!(calculate_fence_length("let x = 1;", '`'), 3);
///
/// // Content with 3 backticks needs 4
/// assert_eq!(calculate_fence_length("```rust\ncode\n```", '`'), 4);
/// ```
pub fn calculate_fence_length(content: &str, fence_char: char) -> usize {
    let mut max_run = 0;
    let mut current_run = 0;

    for c in content.chars() {
        if c == fence_char {
            current_run += 1;
            max_run = max_run.max(current_run);
        } else {
            current_run = 0;
        }
    }

    max_run.max(2) + 1
}

/// Calculate the minimum backtick count needed for inline code.
///
/// Returns the smallest number of backticks (at least 1) that doesn't
/// appear as a run in the content.
///
/// # Examples
///
/// ```
/// use boko::markdown::calculate_inline_code_ticks;
///
/// // Normal content needs 1 backtick
/// assert_eq!(calculate_inline_code_ticks("code"), 1);
///
/// // Content with backticks needs more
/// assert_eq!(calculate_inline_code_ticks("code with ` backtick"), 2);
/// ```
pub fn calculate_inline_code_ticks(content: &str) -> usize {
    let mut max_run = 0;
    let mut current_run = 0;

    for c in content.chars() {
        if c == '`' {
            current_run += 1;
            max_run = max_run.max(current_run);
        } else {
            current_run = 0;
        }
    }

    max_run + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_backslash() {
        assert_eq!(escape_markdown("a\\b"), "a\\\\b");
    }

    #[test]
    fn test_escape_emphasis() {
        assert_eq!(escape_markdown("*bold*"), "\\*bold\\*");
        assert_eq!(escape_markdown("_italic_"), "\\_italic\\_");
    }

    #[test]
    fn test_escape_links() {
        assert_eq!(escape_markdown("[link]"), "\\[link\\]");
    }

    #[test]
    fn test_escape_code() {
        assert_eq!(escape_markdown("`code`"), "\\`code\\`");
    }

    #[test]
    fn test_escape_heading_at_line_start() {
        assert_eq!(escape_markdown("# heading"), "\\# heading");
        // # in the middle of a line is not escaped
        assert_eq!(escape_markdown("not # heading"), "not # heading");
        assert_eq!(escape_markdown("line\n# heading"), "line\n\\# heading");
    }

    #[test]
    fn test_escape_table_pipe() {
        assert_eq!(escape_markdown("a | b"), "a \\| b");
    }

    #[test]
    fn test_escape_html() {
        assert_eq!(escape_markdown("<tag>"), "\\<tag\\>");
    }

    #[test]
    fn test_escape_image_start() {
        // ! followed by [ starts an image, so ! is escaped
        assert_eq!(escape_markdown("![alt]"), "\\!\\[alt\\]");
        // ! not followed by [ is not escaped
        assert_eq!(escape_markdown("! not image"), "! not image");
    }

    #[test]
    fn test_fence_length_no_backticks() {
        assert_eq!(calculate_fence_length("let x = 1;", '`'), 3);
    }

    #[test]
    fn test_fence_length_with_backticks() {
        assert_eq!(calculate_fence_length("``", '`'), 3);
        assert_eq!(calculate_fence_length("```", '`'), 4);
        assert_eq!(calculate_fence_length("````", '`'), 5);
    }

    #[test]
    fn test_fence_length_multiple_runs() {
        assert_eq!(calculate_fence_length("`` and ```", '`'), 4);
    }

    #[test]
    fn test_inline_code_ticks_no_backticks() {
        assert_eq!(calculate_inline_code_ticks("code"), 1);
    }

    #[test]
    fn test_inline_code_ticks_with_backticks() {
        assert_eq!(calculate_inline_code_ticks("`"), 2);
        assert_eq!(calculate_inline_code_ticks("``"), 3);
    }
}
