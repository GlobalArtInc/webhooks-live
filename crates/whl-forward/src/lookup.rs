use std::future::Future;
use std::io;
use std::net::IpAddr;
use std::pin::Pin;

pub type LookupFuture<'a> = Pin<Box<dyn Future<Output = io::Result<Vec<IpAddr>>> + Send + 'a>>;

pub trait LookupHost: Send + Sync + 'static {
    fn lookup<'a>(&'a self, host: &'a str) -> LookupFuture<'a>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemLookup;

impl LookupHost for SystemLookup {
    fn lookup<'a>(&'a self, host: &'a str) -> LookupFuture<'a> {
        Box::pin(async move {
            let addresses = tokio::net::lookup_host((host, 0)).await?;
            Ok(addresses.map(|address| address.ip()).collect())
        })
    }
}
