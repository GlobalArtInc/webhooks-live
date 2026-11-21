use std::time::Duration;

use axum::http::{HeaderMap, HeaderValue, header};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};

use crate::token::RandomToken;

const SESSION_NAME: &str = "whl_session";
const LOGIN_NAME: &str = "whl_login";
const HOST_PREFIX: &str = "__Host-";

#[derive(Debug, Clone)]
pub struct CookiePolicy {
    secure: bool,
    session_name: String,
    login_name: String,
}

impl CookiePolicy {
    pub fn new(secure: bool) -> Self {
        let prefix = if secure { HOST_PREFIX } else { "" };
        Self {
            secure,
            session_name: format!("{prefix}{SESSION_NAME}"),
            login_name: format!("{prefix}{LOGIN_NAME}"),
        }
    }

    pub fn session_name(&self) -> &str {
        &self.session_name
    }

    pub fn login_name(&self) -> &str {
        &self.login_name
    }

    pub fn read_session(&self, headers: &HeaderMap) -> Option<RandomToken> {
        read_token(headers, &self.session_name)
    }

    pub fn read_login(&self, headers: &HeaderMap) -> Option<RandomToken> {
        read_token(headers, &self.login_name)
    }

    pub fn has_session_cookie(&self, headers: &HeaderMap) -> bool {
        CookieJar::from_headers(headers)
            .get(&self.session_name)
            .is_some()
    }

    pub fn set_session(&self, token: &RandomToken) -> Option<HeaderValue> {
        header_value(self.build(&self.session_name, token.unpack(), None))
    }

    pub fn set_login(&self, binding: &RandomToken, ttl: Duration) -> Option<HeaderValue> {
        let max_age = time::Duration::try_from(ttl).ok()?;
        header_value(self.build(&self.login_name, binding.unpack(), Some(max_age)))
    }

    pub fn clear_session(&self) -> Option<HeaderValue> {
        header_value(self.build(&self.session_name, "", Some(time::Duration::ZERO)))
    }

    pub fn clear_login(&self) -> Option<HeaderValue> {
        header_value(self.build(&self.login_name, "", Some(time::Duration::ZERO)))
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

fn read_token(headers: &HeaderMap, name: &str) -> Option<RandomToken> {
    let jar = CookieJar::from_headers(headers);
    let cookie = jar.get(name)?;
    RandomToken::parse(cookie.value())
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
    fn login_cookie_is_short_lived_and_lax() {
        let policy = CookiePolicy::new(true);
        let cookie = text(policy.set_login(&token(), Duration::from_secs(600)));
        assert!(cookie.starts_with("__Host-whl_login="));
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
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("whl_session=abc; __Host-whl_session=short"),
        );
        assert_eq!(policy.read_session(&headers), None);
        assert!(policy.has_session_cookie(&headers));
        let valid = format!("other=1; __Host-whl_session={}", "b".repeat(43));
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&valid).expect("header value"),
        );
        assert_eq!(
            policy.read_session(&headers).map(|t| t.unpack().to_owned()),
            Some("b".repeat(43))
        );
    }
}
