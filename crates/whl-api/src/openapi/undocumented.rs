#![allow(dead_code)]

pub(super) struct Route {
    pub method: &'static str,
    pub path: &'static str,
}

const fn route(method: &'static str, path: &'static str) -> Route {
    Route { method, path }
}

const UNDOCUMENTED: &[Route] = &[];

pub(super) fn all() -> Vec<&'static Route> {
    UNDOCUMENTED.iter().collect()
}
