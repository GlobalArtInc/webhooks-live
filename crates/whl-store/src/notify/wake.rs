use sqlx::postgres::PgListener;

use super::{NotifyStoreError, WAKE_CHANNEL, classify};
use crate::Store;

#[derive(Debug)]
pub struct NotifyWakeListener {
    inner: PgListener,
}

impl NotifyWakeListener {
    pub async fn wait(&mut self) -> Result<(), NotifyStoreError> {
        self.inner.recv().await.map(|_| ()).map_err(classify)
    }
}

impl Store {
    pub async fn listen_for_notify_wakeups(&self) -> Result<NotifyWakeListener, NotifyStoreError> {
        let mut inner = PgListener::connect_with(self.pool())
            .await
            .map_err(classify)?;
        inner.listen(WAKE_CHANNEL).await.map_err(classify)?;
        Ok(NotifyWakeListener { inner })
    }
}
