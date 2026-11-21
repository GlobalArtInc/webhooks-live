use std::time::Duration;

use axum::http::{HeaderMap, HeaderValue, header};
use axum_extra::extract::cookie::{Cookie, SameSite};

use crate::token::RandomToken;

const SESSION_NAME: &str = "whl_session";
const LOGIN_NAME_PREFIX: &str = "whl_login_";
const HOST_PREFIX: &str = "__Host-";
const LOGIN_SUFFIX_CHARS: usize = 12;
const MAX_LOGIN_COOKIES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lookup<'a> {
    Absent,
    Single(&'a [u8]),
    Ambiguous,
}

pub(crate) fn lookup<'a>(headers: &'a HeaderMap, name: &str) -> Lookup<'a> {
    let mut found = None;
    for (candidate, value) in pairs(headers) {
        if candidate == name.as_bytes() {
            if found.is_some() {
                return Lookup::Ambiguous;
            }
            found = Some(value);
        }
    }
    found.map_or(Lookup::Absent, Lookup::Single)
}

fn pairs(headers: &HeaderMap) -> impl Iterator<Item = (&[u8], &[u8])> {
    headers
        .get_all(header::COOKIE)
        .into_iter()
        .flat_map(|value| value.as_bytes().split(|byte| *byte == b';'))
        .filter_map(|pair| {
            let pair = pair.trim_ascii_start();
            let separator = pair.iter().position(|byte| *byte == b'=')?;
            Some((&pair[..separator], &pair[separator + 1..]))
        })
}

fn token_from(lookup: Lookup<'_>) -> Option<RandomToken> {
    match lookup {
        Lookup::Single(value) => RandomToken::parse(std::str::from_utf8(value).ok()?),
        Lookup::Absent | Lookup::Ambiguous => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginCookie {
    name: String,
}

impl LoginCookie {
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Debug, Clone)]
pub struct CookiePolicy {
    secure: bool,
    session_name: String,
    login_prefix: String,
}

impl CookiePolicy {
    pub fn new(secure: bool) -> Self {
        let prefix = if secure { HOST_PREFIX } else { "" };
        Self {
            secure,
            session_name: format!("{prefix}{SESSION_NAME}"),
            login_prefix: format!("{prefix}{LOGIN_NAME_PREFIX}"),
        }
    }

    pub fn session_name(&self) -> &str {
        &self.session_name
    }

    pub fn login_cookie(&self, state: &RandomToken) -> LoginCookie {
        let suffix: String = state.unpack().chars().take(LOGIN_SUFFIX_CHARS).collect();
        LoginCookie {
            name: format!("{}{suffix}", self.login_prefix),
        }
    }

    pub fn read_session(&self, headers: &HeaderMap) -> Option<RandomToken> {
        token_from(lookup(headers, &self.session_name))
    }

    pub fn read_login(&self, headers: &HeaderMap, cookie: &LoginCookie) -> Option<RandomToken> {
        token_from(lookup(headers, &cookie.name))
    }

    pub fn has_session_cookie(&self, headers: &HeaderMap) -> bool {
        lookup(headers, &self.session_name) != Lookup::Absent
    }

    pub fn set_session(&self, token: &RandomToken) -> Option<HeaderValue> {
        header_value(self.build(&self.session_name, token.unpack(), None))
    }

    pub fn set_login(
        &self,
        cookie: &LoginCookie,
        binding: &RandomToken,
        ttl: Duration,
    ) -> Option<HeaderValue> {
        let max_age = time::Duration::try_from(ttl).ok()?;
        header_value(self.build(&cookie.name, binding.unpack(), Some(max_age)))
    }

    pub fn clear_session(&self) -> Option<HeaderValue> {
        header_value(self.build(&self.session_name, "", Some(time::Duration::ZERO)))
    }

    pub fn clear_login(&self, cookie: &LoginCookie) -> Option<HeaderValue> {
        header_value(self.build(&cookie.name, "", Some(time::Duration::ZERO)))
    }

    pub fn clear_stale_logins(&self, headers: &HeaderMap) -> Vec<Option<HeaderValue>> {
        let mut names: Vec<&[u8]> = Vec::new();
        for (name, _) in pairs(headers) {
            if self.is_login_name(name) && !names.contains(&name) {
                names.push(name);
            }
        }
        if names.len() < MAX_LOGIN_COOKIES {
            return Vec::new();
        }
        names
            .into_iter()
            .filter_map(|name| std::str::from_utf8(name).ok())
            .map(|name| header_value(self.build(name, "", Some(time::Duration::ZERO))))
            .collect()
    }

    fn is_login_name(&self, name: &[u8]) -> bool {
        name.strip_prefix(self.login_prefix.as_bytes())
            .is_some_and(|suffix| {
                suffix.len() == LOGIN_SUFFIX_CHARS
                    && suffix
                        .iter()
                        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-' || *byte == b'_')
            })
    }

    fn build(&self, name: &str, value: &str, max_age: Option<time::Duration>) -> Cookie<'static> {
        let mut cookie = Cookie::build((name.to_owned(), value.to_owned()))
            .http_only(true)
            .secure(self.secure)
            .same_site(SameSite::Lax)
            .path("/");
        if let Some(max_age) = max_age {
            cookie = cookie.max_age(max_age);
        }
        cookie.build()
    }
}

fn header_value(cookie: Cookie<'static>) -> Option<HeaderValue> {
    HeaderValue::from_str(&cookie.to_string()).ok()
}

pub(crate) fn append(
    headers: &mut HeaderMap,
    values: impl IntoIterator<Item = Option<HeaderValue>>,
) -> Result<(), CookieError> {
    for value in values {
        headers.append(header::SET_COOKIE, value.ok_or(CookieError)?);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a cookie could not be encoded as a header value")]
pub struct CookieError;

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: Option<HeaderValue>) -> String {
        value
            .expect("cookie encodes")
            .to_str()
            .expect("cookie is ASCII")
            .to_owned()
    }

    fn token() -> RandomToken {
        RandomToken::parse(&"a".repeat(43)).expect("valid token")
    }

    fn attributes(cookie: &str) -> Vec<String> {
        let mut parts: Vec<String> = cookie.split("; ").skip(1).map(str::to_owned).collect();
        parts.sort();
        parts
    }

    fn headers(values: &[&str]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for value in values {
            map.append(
                header::COOKIE,
                HeaderValue::from_str(value).expect("header value"),
            );
        }
        map
    }

    #[test]
    fn secure_session_cookie_has_the_host_prefix_and_exact_attributes() {
        let policy = CookiePolicy::new(true);
        let cookie = text(policy.set_session(&token()));
        assert!(cookie.starts_with(&format!("__Host-whl_session={}; ", "a".repeat(43))));
        assert_eq!(
            attributes(&cookie),
            ["HttpOnly", "Path=/", "SameSite=Lax", "Secure"]
        );
    }

    #[test]
    fn insecure_session_cookie_drops_secure_and_the_prefix() {
        let policy = CookiePolicy::new(false);
        let cookie = text(policy.set_session(&token()));
        assert!(cookie.starts_with("whl_session="));
        assert_eq!(attributes(&cookie), ["HttpOnly", "Path=/", "SameSite=Lax"]);
    }

    #[test]
    fn login_cookie_name_carries_a_suffix_of_the_state_and_is_short_lived() {
        let policy = CookiePolicy::new(true);
        let state = RandomToken::parse(&format!("{}{}", "abcdefghijkl", "z".repeat(31)))
            .expect("valid token");
        let login = policy.login_cookie(&state);
        assert_eq!(login.name(), "__Host-whl_login_abcdefghijkl");
        let cookie = text(policy.set_login(&login, &token(), Duration::from_secs(600)));
        assert!(cookie.starts_with("__Host-whl_login_abcdefghijkl="));
        assert_eq!(
            attributes(&cookie),
            [
                "HttpOnly",
                "Max-Age=600",
                "Path=/",
                "SameSite=Lax",
                "Secure"
            ]
        );
    }

    #[test]
    fn cleared_cookies_expire_immediately_with_the_same_scope() {
        let policy = CookiePolicy::new(true);
        let cookie = text(policy.clear_session());
        assert!(cookie.starts_with("__Host-whl_session=; "));
        let parts = attributes(&cookie);
        assert!(parts.contains(&"Max-Age=0".to_owned()));
        assert!(parts.contains(&"Path=/".to_owned()));
        assert!(parts.contains(&"Secure".to_owned()));
    }

    #[test]
    fn reading_ignores_malformed_and_foreign_cookies() {
        let policy = CookiePolicy::new(true);
        let map = headers(&["whl_session=abc; __Host-whl_session=short"]);
        assert_eq!(policy.read_session(&map), None);
        assert!(policy.has_session_cookie(&map));
        let valid = format!("other=1; __Host-whl_session={}", "b".repeat(43));
        assert_eq!(
            policy
                .read_session(&headers(&[&valid]))
                .map(|t| t.unpack().to_owned()),
            Some("b".repeat(43))
        );
    }

    #[test]
    fn a_name_that_only_decodes_to_the_session_name_never_matches() {
        let policy = CookiePolicy::new(true);
        let value = "b".repeat(43);
        for name in [
            "%5F%5FHost-whl_session",
            "__Host-whl%5Fsession",
            "__%48ost-whl_session",
            "__host-whl_session",
            "__HOST-WHL_SESSION",
            "__Host-whl_session ",
            "__Host-whl_session\t",
            "x__Host-whl_session",
        ] {
            let map = headers(&[&format!("{name}={value}")]);
            assert_eq!(policy.read_session(&map), None, "{name:?}");
            assert!(!policy.has_session_cookie(&map), "{name:?}");
        }
    }

    #[test]
    fn a_smuggled_lookalike_does_not_shadow_or_disturb_the_real_cookie() {
        let policy = CookiePolicy::new(true);
        let real = "c".repeat(43);
        let other = "d".repeat(43);
        for line in [
            format!("%5F%5FHost-whl_session={other}; __Host-whl_session={real}"),
            format!("__Host-whl_session={real}; %5F%5FHost-whl_session={other}"),
            format!("__host-whl_session={other}; __Host-whl_session={real}"),
        ] {
            assert_eq!(
                policy
                    .read_session(&headers(&[&line]))
                    .map(|t| t.unpack().to_owned()),
                Some(real.clone()),
                "{line}"
            );
        }
    }

    #[test]
    fn a_repeated_cookie_name_is_ambiguous_across_pairs_and_headers() {
        let policy = CookiePolicy::new(true);
        let first = "e".repeat(43);
        let second = "f".repeat(43);
        let same_header = format!("__Host-whl_session={first}; __Host-whl_session={second}");
        let identical = format!("__Host-whl_session={first}; x=1; __Host-whl_session={first}");
        let separate = [
            format!("__Host-whl_session={first}"),
            format!("__Host-whl_session={second}"),
        ];
        assert_eq!(policy.read_session(&headers(&[&same_header])), None);
        assert_eq!(policy.read_session(&headers(&[&identical])), None);
        assert_eq!(
            policy.read_session(&headers(&[&separate[0], &separate[1]])),
            None
        );
        assert!(policy.has_session_cookie(&headers(&[&same_header])));
    }

    #[test]
    fn values_are_neither_decoded_nor_unquoted() {
        let policy = CookiePolicy::new(true);
        let encoded = format!("__Host-whl_session={}%41", "g".repeat(42));
        let quoted = format!("__Host-whl_session=\"{}\"", "g".repeat(43));
        assert_eq!(policy.read_session(&headers(&[&encoded])), None);
        assert_eq!(policy.read_session(&headers(&[&quoted])), None);
    }

    #[test]
    fn a_login_cookie_is_looked_up_by_its_own_exact_name() {
        let policy = CookiePolicy::new(true);
        let state = RandomToken::parse(&"h".repeat(43)).expect("valid token");
        let login = policy.login_cookie(&state);
        let binding = "i".repeat(43);
        let name = login.name().to_owned();
        let one = headers(&[&format!("{name}={binding}")]);
        assert_eq!(
            policy
                .read_login(&one, &login)
                .map(|t| t.unpack().to_owned()),
            Some(binding.clone())
        );
        let twice = headers(&[&format!("{name}={binding}; {name}={binding}")]);
        assert_eq!(policy.read_login(&twice, &login), None);
        let encoded = headers(&[&format!("__Host-whl%5Flogin_hhhhhhhhhhhh={binding}")]);
        assert_eq!(policy.read_login(&encoded, &login), None);
        let other = RandomToken::parse(&"j".repeat(43)).expect("valid token");
        assert_eq!(policy.read_login(&one, &policy.login_cookie(&other)), None);
    }

    #[test]
    fn stale_login_cookies_are_cleared_only_past_the_limit_and_only_by_exact_name() {
        let policy = CookiePolicy::new(true);
        let names: Vec<String> = (0..MAX_LOGIN_COOKIES)
            .map(|index| format!("__Host-whl_login_{index:0>12}"))
            .collect();
        let few = names[..MAX_LOGIN_COOKIES - 1]
            .iter()
            .map(|name| format!("{name}=x"))
            .collect::<Vec<_>>()
            .join("; ");
        assert!(policy.clear_stale_logins(&headers(&[&few])).is_empty());

        let many = names
            .iter()
            .map(|name| format!("{name}=x"))
            .chain(["__Host-whl_login_short=x".to_owned(), "other=1".to_owned()])
            .collect::<Vec<_>>()
            .join("; ");
        let cleared = policy.clear_stale_logins(&headers(&[&many]));
        assert_eq!(cleared.len(), MAX_LOGIN_COOKIES);
        for (value, name) in cleared.into_iter().zip(&names) {
            let cookie = text(value);
            assert!(cookie.starts_with(&format!("{name}=; ")), "{cookie}");
            assert!(cookie.contains("Max-Age=0"));
        }
    }
}
