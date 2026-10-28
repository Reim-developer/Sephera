//! JSON with comments and trailing commas, as configuration files
//! actually carry it.
//!
//! A tsconfig is JSONC by definition: `//` and `/* */` comments and a
//! comma before a closing bracket are all legal there and all fatal
//! to a plain JSON parse. This module is the one place that leniency
//! lives, so a config parsed here and a config parsed anywhere else
//! cannot disagree about what the file says.

/// Parse JSONC into a JSON value.
///
/// Comments are stripped and trailing commas dropped first, so a
/// file that is plain JSON parses identically, and a file with
/// comments or trailing commas parses where plain JSON fails.
/// Anything else is `None`, never a partial value.
#[must_use]
pub fn parse(text: &str) -> Option<serde_json::Value> {
    let uncommented = strip_comments(text);
    let mut cleaned = String::with_capacity(uncommented.len());
    let chars: Vec<char> = uncommented.chars().collect();
    let mut in_string = false;
    let mut escaped = false;
    for (index, &ch) in chars.iter().enumerate() {
        if in_string {
            cleaned.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => {
                in_string = true;
                cleaned.push(ch);
            }
            ',' => {
                // A comma whose next meaningful character closes the
                // object or array is a trailing comma; drop it.
                let next =
                    chars[index + 1..].iter().find(|c| !c.is_whitespace());
                if !matches!(next, Some('}' | ']')) {
                    cleaned.push(ch);
                }
            }
            _ => cleaned.push(ch),
        }
    }
    serde_json::from_str(&cleaned).ok()
}

/// Remove `//` and `/* */` comments, leaving string contents untouched.
fn strip_comments(text: &str) -> String {
    let mut cleaned = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if in_string {
            cleaned.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => {
                in_string = true;
                cleaned.push(ch);
            }
            '/' if chars.peek() == Some(&'/') => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        cleaned.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = '\0';
                for next in chars.by_ref() {
                    if previous == '*' && next == '/' {
                        break;
                    }
                    previous = next;
                }
            }
            _ => cleaned.push(ch),
        }
    }
    cleaned
}
