use whl_store::schedules::{MAX_PAUSED_REASON_BYTES, MAX_RUN_ERROR_BYTES, RunOutcome};

pub const PAUSE_AFTER_FAILURES: i16 = 10;
pub const SUCCESS_STATUS_LIMIT: u16 = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub failures: i16,
    pub paused: bool,
}

pub fn progress(previous: i16, outcome: RunOutcome) -> Progress {
    match outcome {
        RunOutcome::Succeeded => Progress {
            failures: 0,
            paused: false,
        },
        RunOutcome::Skipped => Progress {
            failures: previous,
            paused: false,
        },
        RunOutcome::Failed => {
            let failures = previous.saturating_add(1);
            Progress {
                failures,
                paused: failures >= PAUSE_AFTER_FAILURES,
            }
        }
    }
}

pub fn is_success(status: u16) -> bool {
    (100..SUCCESS_STATUS_LIMIT).contains(&status)
}

pub fn pause_reason(failures: i16, last_error: Option<&str>) -> String {
    let base = format!("{failures} consecutive failures");
    let reason = match last_error {
        Some(error) => format!("{base}, last: {error}"),
        None => base,
    };
    bounded(&reason, MAX_PAUSED_REASON_BYTES)
}

pub fn run_error(error: &str) -> String {
    bounded(error, MAX_RUN_ERROR_BYTES)
}

pub fn bounded(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let end = (0..=max_bytes)
        .rev()
        .find(|index| text.is_char_boundary(*index))
        .unwrap_or(0);
    text[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tenth_consecutive_failure_pauses_and_a_success_resets() {
        let mut failures = 0;
        for attempt in 1..=PAUSE_AFTER_FAILURES {
            let step = progress(failures, RunOutcome::Failed);
            assert_eq!(step.paused, attempt == PAUSE_AFTER_FAILURES);
            failures = step.failures;
        }
        assert_eq!(
            progress(failures, RunOutcome::Succeeded),
            Progress {
                failures: 0,
                paused: false
            }
        );
    }

    #[test]
    fn a_skipped_run_neither_counts_nor_resets() {
        assert_eq!(
            progress(4, RunOutcome::Skipped),
            Progress {
                failures: 4,
                paused: false
            }
        );
    }

    #[test]
    fn statuses_of_400_and_above_are_failures() {
        assert!(is_success(204));
        assert!(is_success(302));
        assert!(!is_success(400));
        assert!(!is_success(503));
    }

    #[test]
    fn bounded_text_never_splits_a_character() {
        let text = "\u{1F600}".repeat(200);
        let cut = bounded(&text, 501);
        assert!(cut.len() <= 501);
        assert!(cut.chars().all(|character| character == '\u{1F600}'));
    }
}
