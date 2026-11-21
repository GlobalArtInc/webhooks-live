use std::io::{self, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_SHOWN_CHARS: usize = 200;
pub const MAX_MESSAGE_CHARS: usize = 500;
const SECONDS_PER_DAY: u64 = 86_400;

pub fn safe(text: &str) -> String {
    safe_to(text, MAX_SHOWN_CHARS)
}

pub fn safe_to(text: &str, limit: usize) -> String {
    let mut shown = String::with_capacity(text.len().min(limit));
    for (count, character) in text.chars().enumerate() {
        if count >= limit {
            shown.push('…');
            break;
        }
        let hidden = matches!(
            character,
            '\u{061c}'
                | '\u{200b}'..='\u{200f}'
                | '\u{2028}'..='\u{202e}'
                | '\u{2066}'..='\u{206f}'
                | '\u{feff}'
                | '\u{e0000}'..='\u{e007f}'
        );
        if character.is_control() || hidden {
            shown.extend(character.escape_default());
        } else {
            shown.push(character);
        }
    }
    shown
}

pub fn clock(now: SystemTime) -> String {
    let seconds = now
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
        % SECONDS_PER_DAY;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60
    )
}

pub fn millis(elapsed: Duration) -> String {
    format!("{}ms", elapsed.as_millis())
}

#[derive(Debug, Clone, Copy)]
pub struct Output {
    verbose: bool,
}

impl Output {
    pub fn new(verbose: bool) -> Self {
        Self { verbose }
    }

    pub fn event(&self, line: &str) -> io::Result<()> {
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "{} {line}", clock(SystemTime::now()))?;
        stdout.flush()
    }

    pub fn detail(&self, line: &str) -> io::Result<()> {
        if self.verbose {
            self.event(line)
        } else {
            Ok(())
        }
    }

    pub fn notice(&self, line: &str) -> io::Result<()> {
        let mut stderr = io::stderr().lock();
        writeln!(stderr, "{} {line}", clock(SystemTime::now()))?;
        stderr.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_terminal_control_sequences() {
        assert_eq!(safe("a\u{1b}[31mred\n"), "a\\u{1b}[31mred\\n");
    }

    #[test]
    fn escapes_invisible_formatting_and_tag_characters() {
        let hostile = "a\u{061c}b\u{206a}c\u{feff}d\u{2028}e\u{2029}f\u{e0041}g\u{202e}h";
        let shown = safe(hostile);
        for character in shown.chars() {
            assert!(!character.is_control());
            assert!(
                character.is_ascii() || character.is_alphanumeric(),
                "{shown:?}"
            );
        }
        assert!(shown.starts_with("a\\u{61c}b"));
    }

    #[test]
    fn truncates_long_text() {
        let shown = safe(&"a".repeat(500));
        assert_eq!(shown.chars().count(), MAX_SHOWN_CHARS + 1);
        assert!(shown.ends_with('…'));
    }

    #[test]
    fn formats_the_clock_in_utc() {
        let moment = UNIX_EPOCH + Duration::from_secs(86_400 + 3 * 3600 + 4 * 60 + 5);
        assert_eq!(clock(moment), "03:04:05");
    }
}
