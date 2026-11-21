#[derive(Debug, Clone, Default)]
pub struct Filters {
    methods: Vec<String>,
    path_prefix: Option<String>,
}

impl Filters {
    pub fn new(methods: Vec<String>, path_prefix: Option<String>) -> Self {
        Self {
            methods: methods
                .into_iter()
                .map(|method| method.trim().to_ascii_uppercase())
                .filter(|method| !method.is_empty())
                .collect(),
            path_prefix,
        }
    }

    pub fn matches(&self, method: &str, path: &str) -> bool {
        let method_ok = self.methods.is_empty()
            || self
                .methods
                .iter()
                .any(|wanted| wanted.eq_ignore_ascii_case(method));
        let path_ok = self
            .path_prefix
            .as_deref()
            .is_none_or(|prefix| path.starts_with(prefix));
        method_ok && path_ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_everything_without_filters() {
        assert!(Filters::default().matches("DELETE", "/anything"));
    }

    #[test]
    fn matches_methods_case_insensitively() {
        let filters = Filters::new(vec!["post".to_owned(), "PUT".to_owned()], None);
        assert!(filters.matches("POST", "/"));
        assert!(filters.matches("put", "/"));
        assert!(!filters.matches("GET", "/"));
    }

    #[test]
    fn matches_the_path_by_prefix() {
        let filters = Filters::new(Vec::new(), Some("/stripe".to_owned()));
        assert!(filters.matches("POST", "/stripe/events"));
        assert!(!filters.matches("POST", "/github"));
    }

    #[test]
    fn requires_both_filters_to_match() {
        let filters = Filters::new(vec!["POST".to_owned()], Some("/a".to_owned()));
        assert!(!filters.matches("GET", "/a"));
        assert!(!filters.matches("POST", "/b"));
        assert!(filters.matches("POST", "/a/1"));
    }
}
