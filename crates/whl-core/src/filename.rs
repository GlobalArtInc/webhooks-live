pub const FALLBACK_STEM: &str = "capture";
pub const MAX_STEM_CHARS: usize = 64;
pub const MAX_EXTENSION_CHARS: usize = 16;

pub fn safe_filename(stem: &str, extension: &str) -> String {
    let stem = sanitize(stem, MAX_STEM_CHARS);
    let stem = stem.trim_start_matches('.');
    let stem = if stem.is_empty() { FALLBACK_STEM } else { stem };
    let extension = sanitize(extension, MAX_EXTENSION_CHARS);
    let extension = extension.trim_matches('.');
    if extension.is_empty() {
        stem.to_owned()
    } else {
        format!("{stem}.{extension}")
    }
}

fn sanitize(text: &str, limit: usize) -> String {
    text.chars()
        .take(limit)
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect()
}
