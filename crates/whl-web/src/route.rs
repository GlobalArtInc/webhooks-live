pub const UI_ROUTES: &[&str] = &[
    "/",
    "/endpoints",
    "/signin",
    "/settings",
    "/e/{endpoint_id}",
    "/e/{endpoint_id}/c/{capture_id}",
    "/e/{endpoint_id}/settings",
    "/e/{endpoint_id}/forwarding",
    "/e/{endpoint_id}/diff/{a}/{b}",
    "/assets/{file}",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target<'a> {
    Hashed(&'a str),
    Shell,
}

pub(crate) fn classify(path: &str) -> Option<Target<'_>> {
    let key = path.strip_prefix('/')?;
    if !is_clean(key) {
        return None;
    }
    let pattern = UI_ROUTES
        .iter()
        .copied()
        .find(|pattern| matches(pattern, path))?;
    match pattern {
        "/assets/{file}" => Some(Target::Hashed(key)),
        _ if key.rsplit('/').next().unwrap_or_default().contains('.') => None,
        _ => Some(Target::Shell),
    }
}

fn is_clean(key: &str) -> bool {
    key.is_empty()
        || key.split('/').all(|segment| {
            !segment.is_empty() && segment != "." && segment != ".." && !segment.contains('\\')
        })
}

fn matches(pattern: &str, path: &str) -> bool {
    pattern.split('/').count() == path.split('/').count()
        && pattern
            .split('/')
            .zip(path.split('/'))
            .all(|(expected, actual)| {
                if expected.starts_with('{') {
                    !actual.is_empty()
                } else {
                    expected == actual
                }
            })
}
