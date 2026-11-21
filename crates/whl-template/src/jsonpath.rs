use serde_json_path::JsonPath;

use crate::error::ErrorCode;

const MAX_PATH_BYTES: usize = 256;
const MAX_DESCENDANTS: usize = 1;
const MAX_FILTERS: usize = 1;
const MAX_NESTING: usize = 8;
const MAX_MESSAGE_BYTES: usize = 200;

pub(crate) struct PathProblem {
    pub code: ErrorCode,
    pub message: String,
}

fn too_complex(message: &str) -> PathProblem {
    PathProblem {
        code: ErrorCode::JsonPathTooComplex,
        message: message.to_owned(),
    }
}

fn preflight(text: &str) -> Result<(), PathProblem> {
    if text.len() > MAX_PATH_BYTES {
        return Err(too_complex("JSON path is longer than 256 bytes"));
    }
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut depth = 0usize;
    let mut filters = 0usize;
    let mut descendants = 0usize;
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if let Some(open) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == open {
                quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' => quote = Some(character),
            '(' | '[' => {
                depth += 1;
                if depth > MAX_NESTING {
                    return Err(too_complex("JSON path is nested deeper than 8 levels"));
                }
            }
            ')' | ']' => depth = depth.saturating_sub(1),
            '?' => {
                filters += 1;
                if filters > MAX_FILTERS {
                    return Err(too_complex("JSON path may contain at most one filter"));
                }
            }
            '.' if characters.peek() == Some(&'.') => {
                characters.next();
                descendants += 1;
                if descendants > MAX_DESCENDANTS {
                    return Err(too_complex(
                        "JSON path may contain at most one descendant segment",
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn compile(text: &str) -> Result<JsonPath, PathProblem> {
    preflight(text)?;
    JsonPath::parse(text).map_err(|error| {
        let rendered = error.to_string();
        let first_line = rendered.lines().next().unwrap_or("");
        let message: String = first_line.chars().take(MAX_MESSAGE_BYTES).collect();
        PathProblem {
            code: ErrorCode::InvalidJsonPath,
            message: format!("invalid JSON path: {message}"),
        }
    })
}
