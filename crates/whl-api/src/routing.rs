use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::handler::Handler;
use axum::http::Method;
use axum::routing::{self, MethodRouter};

use crate::state::ApiState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteEntry {
    pub method: Method,
    pub path: &'static str,
}

pub struct Verbs {
    methods: Vec<Method>,
    router: MethodRouter<ApiState>,
}

macro_rules! verbs {
    ($($name:ident => $method:expr),+ $(,)?) => {
        $(
            #[allow(dead_code)]
            pub fn $name<H, T>(handler: H) -> Verbs
            where
                H: Handler<T, ApiState>,
                T: 'static,
            {
                Verbs {
                    methods: vec![$method],
                    router: routing::$name(handler),
                }
            }
        )+

        impl Verbs {
            $(
                #[allow(dead_code)]
                pub fn $name<H, T>(mut self, handler: H) -> Self
                where
                    H: Handler<T, ApiState>,
                    T: 'static,
                {
                    self.methods.push($method);
                    self.router = self.router.$name(handler);
                    self
                }
            )+
        }
    };
}

verbs! {
    get => Method::GET,
    post => Method::POST,
    put => Method::PUT,
    patch => Method::PATCH,
    head => Method::HEAD,
    delete => Method::DELETE,
}

impl Verbs {
    pub fn body_limit(mut self, limit: usize) -> Self {
        self.router = self.router.layer(DefaultBodyLimit::max(limit));
        self
    }
}

#[derive(Default)]
pub struct Routes {
    router: Router<ApiState>,
    entries: Vec<RouteEntry>,
}

impl Routes {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn route(mut self, path: &'static str, verbs: Verbs) -> Self {
        self.entries.extend(
            verbs
                .methods
                .into_iter()
                .map(|method| RouteEntry { method, path }),
        );
        self.router = self.router.route(path, verbs.router);
        self
    }

    pub fn merge(mut self, other: Routes) -> Self {
        self.entries.extend(other.entries);
        self.router = self.router.merge(other.router);
        self
    }

    pub fn into_router(self) -> Router<ApiState> {
        self.router
    }

    #[cfg(test)]
    pub fn entries(&self) -> &[RouteEntry] {
        &self.entries
    }
}
