const MAX_LENGTH: usize = 2048;
const FALLBACK: &str = "/";

pub fn sanitize(raw: Option<&str>) -> String {
    match raw {
        Some(candidate) if is_local_path(candidate) => candidate.to_owned(),
        _ => FALLBACK.to_owned(),
    }
}

fn is_local_path(candidate: &str) -> bool {
    let bytes = candidate.as_bytes();
    bytes.first() == Some(&b'/')
        && bytes.get(1) != Some(&b'/')
        && bytes.len() <= MAX_LENGTH
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_graphic() && *byte != b'\\')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_local_paths_are_kept() {
        for path in [
            "/",
            "/e/abc",
            "/e/abc/captures?x=1&y=2#frag",
            "/endpoints",
            "/a//b",
            "/%2F/evil",
        ] {
            assert_eq!(sanitize(Some(path)), path);
        }
    }

    #[test]
    fn everything_else_falls_back_to_the_root() {
        let long = format!("/{}", "a".repeat(MAX_LENGTH));
        let rejected = [
            "",
            "e/abc",
            "//evil.example",
            "//",
            "https://evil.example",
            "http://evil.example/",
            "javascript:alert(1)",
            "/\\evil.example",
            "\\\\evil.example",
            "/a\\b",
            "/\t/evil.example",
            "/\n/evil.example",
            "/\r\nSet-Cookie: x=y",
            "/a\u{0}b",
            "/a\u{7f}b",
            "/a b",
            "/caf\u{e9}",
            long.as_str(),
        ];
        for path in rejected {
            assert_eq!(sanitize(Some(path)), "/", "{path:?}");
        }
        assert_eq!(sanitize(None), "/");
    }
}
