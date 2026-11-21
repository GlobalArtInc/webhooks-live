use std::future::Future;
use std::time::Duration;

use tokio::sync::watch;
use tokio::task::JoinHandle;
use whl_capture::StorageFull;
use whl_core::config::Config;
use whl_core::limiter::Limiters;
use whl_dns::DnsHandle;
use whl_notify::{NotifyHandle, NotifyWorker};
use whl_schedule::{RunnerHandle, ScheduleRunner};
use whl_store::Store;
use whl_store::maintenance::{
    MaintenanceHandle, MaintenanceSettings, exceeds_hard_limit, spawn_maintenance,
};
use whl_telemetry::series;

use crate::until_stopped;

pub const STORAGE_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
pub const LIMITER_CLEANUP_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundSettings {
    pub storage_refresh: Duration,
    pub limiter_cleanup: Duration,
    pub maintenance: MaintenanceSettings,
}

impl BackgroundSettings {
    pub fn from_config(config: &Config) -> Self {
        let mut maintenance = MaintenanceSettings::new(
            config.capture.retention,
            config.capture.storage_cap_bytes.as_u64(),
        );
        maintenance.session_idle = config.auth.session_idle;
        Self {
            storage_refresh: STORAGE_REFRESH_INTERVAL,
            limiter_cleanup: LIMITER_CLEANUP_INTERVAL,
            maintenance,
        }
    }
}

pub(crate) struct Workers {
    pub(crate) dns: Option<DnsHandle>,
    pub(crate) schedules: Option<ScheduleRunner>,
    pub(crate) notify: Option<NotifyWorker>,
}

pub(crate) struct Background {
    refresher: JoinHandle<()>,
    cleanup: JoinHandle<()>,
    maintenance: MaintenanceHandle,
    dns: Option<DnsHandle>,
    schedules: Option<RunnerHandle>,
    notify: Option<NotifyHandle>,
}

impl Background {
    pub(crate) fn spawn(
        store: Store,
        settings: &BackgroundSettings,
        storage_full: StorageFull,
        limiters: Limiters,
        workers: Workers,
        stop: &watch::Receiver<bool>,
    ) -> Self {
        let refresher = tokio::spawn(refresh_storage(
            store.clone(),
            storage_full,
            settings.maintenance.cap_bytes,
            settings.storage_refresh,
            until_stopped(stop.clone()),
        ));
        let cleanup = tokio::spawn(clean_limiters(
            limiters,
            settings.limiter_cleanup,
            until_stopped(stop.clone()),
        ));
        let maintenance = spawn_maintenance(
            store,
            settings.maintenance.clone(),
            until_stopped(stop.clone()),
        );
        let Workers {
            dns,
            schedules,
            notify,
        } = workers;
        let schedules = schedules.map(|runner| runner.spawn(until_stopped(stop.clone())));
        let notify = notify.map(|worker| worker.spawn(until_stopped(stop.clone())));
        Self {
            refresher,
            cleanup,
            maintenance,
            dns,
            schedules,
            notify,
        }
    }

    pub(crate) async fn join(self) {
        for (name, task) in [
            ("storage refresher", self.refresher),
            ("limiter cleanup", self.cleanup),
        ] {
            if let Err(error) = task.await {
                tracing::error!(%error, task = name, "a background task ended abnormally");
            }
        }
        self.maintenance.join().await;
        if let Some(dns) = self.dns {
            dns.join().await;
        }
        if let Some(mut schedules) = self.schedules {
            schedules.join().await;
        }
        if let Some(mut notify) = self.notify {
            notify.join().await;
        }
    }
}

async fn refresh_storage(
    store: Store,
    storage_full: StorageFull,
    cap_bytes: u64,
    interval: Duration,
    stop: impl Future<Output = ()>,
) {
    tokio::pin!(stop);
    loop {
        let measured = tokio::select! {
            () = &mut stop => return,
            measured = store.storage_bytes() => measured,
        };
        match measured {
            Ok(used) => {
                metrics::gauge!(series::STORE_BYTES).set(used as f64);
                let full = exceeds_hard_limit(used, cap_bytes);
                if full != storage_full.is_full() {
                    tracing::warn!(
                        used_bytes = used,
                        cap_bytes,
                        full,
                        "the storage-full state changed"
                    );
                }
                storage_full.set(full);
            }
            Err(error) => tracing::warn!(%error, "refreshing the storage figure failed"),
        }
        tokio::select! {
            () = &mut stop => return,
            () = tokio::time::sleep(interval) => {}
        }
    }
}

async fn clean_limiters(limiters: Limiters, interval: Duration, stop: impl Future<Output = ()>) {
    tokio::pin!(stop);
    loop {
        tokio::select! {
            () = &mut stop => return,
            () = tokio::time::sleep(interval) => limiters.cleanup(),
        }
    }
}
