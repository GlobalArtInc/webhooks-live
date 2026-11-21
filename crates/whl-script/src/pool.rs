use std::collections::{HashMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::alloc::{MemoryWindow, allocator_installed};
use crate::compiled::{CompiledScript, ScriptCompiler};
use crate::engine::warm_up;
use crate::error::ScriptError;
use crate::exec::{Extension, ScriptJob, TenantKey, execute, no_extension};
use crate::limits::MAX_COMPILE_MEMORY_BYTES;
use crate::output::ScriptOutput;

const MIN_STACK_BYTES: usize = 2 * 1024 * 1024;
const MAX_THREADS: usize = 64;
const MAX_QUEUE_CAPACITY: usize = 100_000;
const MAX_ABANDONED_THREADS: usize = 1024;
const SUPERVISOR_TICK: Duration = Duration::from_millis(20);
const JOIN_POLL: Duration = Duration::from_millis(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolConfig {
    pub threads: usize,
    pub queue_capacity: usize,
    pub stack_bytes: usize,
    pub result_grace: Duration,
    pub compile_timeout: Duration,
    pub max_queue_wait: Duration,
    pub stuck_grace: Duration,
    pub max_abandoned_threads: usize,
    pub shutdown_timeout: Duration,
    pub max_running_per_tenant: usize,
    pub max_queued_per_tenant: usize,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            threads: 2,
            queue_capacity: 64,
            stack_bytes: 8 * 1024 * 1024,
            result_grace: Duration::from_secs(1),
            compile_timeout: Duration::from_secs(2),
            max_queue_wait: Duration::from_secs(1),
            stuck_grace: Duration::from_secs(2),
            max_abandoned_threads: 8,
            shutdown_timeout: Duration::from_secs(5),
            max_running_per_tenant: 1,
            max_queued_per_tenant: 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PoolStats {
    pub workers: usize,
    pub busy: usize,
    pub queued: usize,
    pub stuck_workers: u64,
    pub abandoned_threads: usize,
    pub crashed_workers: u64,
    pub spawn_failures: u64,
}

#[derive(Debug, Error)]
pub enum PoolError {
    #[error("pool setting {0} is out of range")]
    Config(&'static str),
    #[error("the counting allocator is not installed as the global allocator")]
    AllocatorNotInstalled,
    #[error("a script worker thread could not be started: {0}")]
    Spawn(#[from] std::io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Start {
    Run,
    QueueTimeout,
}

type Task = Box<dyn FnOnce(Start) + Send + 'static>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Lane {
    Internal,
    Tenant(TenantKey),
}

struct Queued {
    task: Task,
    enqueued: Instant,
    budget: Duration,
}

#[derive(Default)]
struct LaneState {
    waiting: VecDeque<Queued>,
    running: usize,
}

struct Running {
    lane: Lane,
    started: Instant,
    budget: Duration,
}

struct Worker {
    id: u64,
    handle: Option<JoinHandle<()>>,
    running: Option<Running>,
}

#[derive(Default)]
struct State {
    lanes: HashMap<Lane, LaneState>,
    rotation: VecDeque<Lane>,
    queued: usize,
    closed: bool,
    workers: Vec<Worker>,
    next_worker: u64,
    abandoned: usize,
    stuck_total: u64,
    crashed_total: u64,
    spawn_failures: u64,
}

enum Next {
    Run(Task),
    Expired(Task),
    Exit,
}

struct Shared {
    state: Mutex<State>,
    ready: Condvar,
    supervisor: Condvar,
    stop: Arc<AtomicBool>,
    config: PoolConfig,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn running_limit(&self, lane: &Lane) -> usize {
        match lane {
            Lane::Internal => usize::MAX,
            Lane::Tenant(_) => self.config.max_running_per_tenant,
        }
    }

    fn queued_limit(&self, lane: &Lane) -> usize {
        match lane {
            Lane::Internal => usize::MAX,
            Lane::Tenant(_) => self.config.max_queued_per_tenant,
        }
    }

    fn enqueue(&self, lane: Lane, queued: Queued) -> Result<(), ScriptError> {
        let mut state = self.lock();
        if state.closed {
            return Err(ScriptError::ShutDown);
        }
        if state.abandoned > self.config.max_abandoned_threads {
            return Err(ScriptError::PoolUnhealthy);
        }
        if state.queued >= self.config.queue_capacity {
            return Err(ScriptError::Overloaded);
        }
        let waiting = state
            .lanes
            .get(&lane)
            .map_or(0, |entry| entry.waiting.len());
        if waiting >= self.queued_limit(&lane) {
            return Err(ScriptError::QueueFull);
        }
        let entry = state.lanes.entry(lane.clone()).or_default();
        entry.waiting.push_back(queued);
        if entry.waiting.len() == 1 {
            state.rotation.push_back(lane);
        }
        state.queued += 1;
        drop(state);
        self.ready.notify_one();
        Ok(())
    }

    fn next(&self, id: u64) -> Next {
        let mut state = self.lock();
        loop {
            if state.closed || !state.workers.iter().any(|worker| worker.id == id) {
                return Next::Exit;
            }
            if let Some(next) = self.take(&mut state, id) {
                return next;
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn take(&self, state: &mut State, id: u64) -> Option<Next> {
        let now = Instant::now();
        for _ in 0..state.rotation.len() {
            let lane = state.rotation.pop_front()?;
            let limit = self.running_limit(&lane);
            let entry = state.lanes.get_mut(&lane)?;
            if entry.running >= limit {
                state.rotation.push_back(lane);
                continue;
            }
            let queued = entry.waiting.pop_front()?;
            let more = !entry.waiting.is_empty();
            let idle = !more && entry.running == 0;
            state.queued = state.queued.saturating_sub(1);
            if more {
                state.rotation.push_back(lane.clone());
            }
            if now.saturating_duration_since(queued.enqueued) > self.config.max_queue_wait {
                if idle {
                    state.lanes.remove(&lane);
                }
                return Some(Next::Expired(queued.task));
            }
            if let Some(entry) = state.lanes.get_mut(&lane) {
                entry.running += 1;
            }
            if let Some(worker) = state.workers.iter_mut().find(|worker| worker.id == id) {
                worker.running = Some(Running {
                    lane: lane.clone(),
                    started: now,
                    budget: queued.budget,
                });
            }
            return Some(Next::Run(queued.task));
        }
        None
    }

    fn finish(&self, id: u64) {
        let mut state = self.lock();
        let lane = state
            .workers
            .iter_mut()
            .find(|worker| worker.id == id)
            .and_then(|worker| worker.running.take())
            .map(|running| running.lane);
        if let Some(lane) = lane {
            release(&mut state, &lane);
        }
        drop(state);
        self.ready.notify_all();
    }

    fn worker_exited(&self, id: u64, panicking: bool) {
        let mut state = self.lock();
        let position = state.workers.iter().position(|worker| worker.id == id);
        match position {
            Some(index) => {
                let worker = state.workers.remove(index);
                if let Some(running) = worker.running {
                    release(&mut state, &running.lane);
                }
                if !state.closed || panicking {
                    state.crashed_total += 1;
                }
            }
            None => state.abandoned = state.abandoned.saturating_sub(1),
        }
        drop(state);
        self.supervisor.notify_all();
    }

    fn spawn_worker(self: &Arc<Self>, state: &mut State) -> Result<(), std::io::Error> {
        let id = state.next_worker;
        let shared = Arc::clone(self);
        let handle = std::thread::Builder::new()
            .name(format!("whl-script-{id}"))
            .stack_size(self.config.stack_bytes)
            .spawn(move || worker_main(&shared, id))?;
        state.next_worker += 1;
        state.workers.push(Worker {
            id,
            handle: Some(handle),
            running: None,
        });
        Ok(())
    }

    fn expire(&self, state: &mut State) -> Vec<Task> {
        let now = Instant::now();
        let mut expired = Vec::new();
        let mut emptied = Vec::new();
        for (lane, entry) in &mut state.lanes {
            while entry.waiting.front().is_some_and(|queued| {
                now.saturating_duration_since(queued.enqueued) > self.config.max_queue_wait
            }) {
                if let Some(queued) = entry.waiting.pop_front() {
                    expired.push(queued.task);
                }
            }
            if entry.waiting.is_empty() {
                emptied.push(lane.clone());
            }
        }
        state.queued = state.queued.saturating_sub(expired.len());
        for lane in emptied {
            state.rotation.retain(|queued| *queued != lane);
            if state
                .lanes
                .get(&lane)
                .is_some_and(|entry| entry.running == 0)
            {
                state.lanes.remove(&lane);
            }
        }
        expired
    }

    fn abandon_stuck(&self, state: &mut State) -> bool {
        let now = Instant::now();
        let mut abandoned = false;
        let mut index = 0;
        while index < state.workers.len() {
            let stuck = state.workers[index]
                .running
                .as_ref()
                .is_some_and(|running| {
                    now.saturating_duration_since(running.started)
                        > running.budget.saturating_add(self.config.stuck_grace)
                });
            if !stuck {
                index += 1;
                continue;
            }
            let worker = state.workers.remove(index);
            if let Some(running) = &worker.running {
                release(state, &running.lane);
            }
            state.abandoned += 1;
            state.stuck_total += 1;
            abandoned = true;
        }
        abandoned
    }

    fn top_up(self: &Arc<Self>, state: &mut State) {
        while !state.closed
            && state.workers.len() < self.config.threads
            && state.abandoned <= self.config.max_abandoned_threads
        {
            if self.spawn_worker(state).is_err() {
                state.spawn_failures += 1;
                break;
            }
        }
    }
}

fn release(state: &mut State, lane: &Lane) {
    let idle = match state.lanes.get_mut(lane) {
        Some(entry) => {
            entry.running = entry.running.saturating_sub(1);
            entry.running == 0 && entry.waiting.is_empty()
        }
        None => false,
    };
    if idle {
        state.lanes.remove(lane);
    }
}

struct ExitGuard {
    shared: Arc<Shared>,
    id: u64,
}

impl Drop for ExitGuard {
    fn drop(&mut self) {
        self.shared.worker_exited(self.id, std::thread::panicking());
    }
}

fn worker_main(shared: &Arc<Shared>, id: u64) {
    let _exit = ExitGuard {
        shared: Arc::clone(shared),
        id,
    };
    loop {
        match shared.next(id) {
            Next::Run(task) => {
                let outcome = catch_unwind(AssertUnwindSafe(|| task(Start::Run)));
                shared.finish(id);
                if outcome.is_err() {
                    shared.lock().crashed_total += 1;
                }
            }
            Next::Expired(task) => task(Start::QueueTimeout),
            Next::Exit => return,
        }
    }
}

fn supervisor_main(shared: &Arc<Shared>) {
    let mut state = shared.lock();
    loop {
        if state.closed {
            return;
        }
        let expired = shared.expire(&mut state);
        let abandoned = shared.abandon_stuck(&mut state);
        shared.top_up(&mut state);
        drop(state);
        if abandoned {
            shared.ready.notify_all();
        }
        for task in expired {
            task(Start::QueueTimeout);
        }
        state = shared.lock();
        if state.closed {
            return;
        }
        state = shared
            .supervisor
            .wait_timeout(state, SUPERVISOR_TICK)
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
}

fn join_within(handles: Vec<JoinHandle<()>>, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    let mut remaining = handles;
    loop {
        let mut still_running = Vec::new();
        for handle in remaining {
            if handle.thread().id() == std::thread::current().id() {
                continue;
            }
            if handle.is_finished() {
                let _ = handle.join();
            } else {
                still_running.push(handle);
            }
        }
        remaining = still_running;
        if remaining.is_empty() || Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(JOIN_POLL);
    }
}

pub struct Pending<T> {
    receiver: Receiver<Result<T, ScriptError>>,
    cancel: Arc<AtomicBool>,
    wait: Duration,
    submitted: Instant,
}

impl<T> Pending<T> {
    pub fn wait(self) -> Result<T, ScriptError> {
        let remaining = self.wait.saturating_sub(self.submitted.elapsed());
        match self.receiver.recv_timeout(remaining) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                self.cancel.store(true, Ordering::SeqCst);
                Err(ScriptError::ResultTimeout)
            }
            Err(RecvTimeoutError::Disconnected) => Err(ScriptError::ShutDown),
        }
    }

    pub fn try_result(&self) -> Option<Result<T, ScriptError>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Disconnected) => Some(Err(ScriptError::ShutDown)),
            Err(TryRecvError::Empty) => {
                if self.submitted.elapsed() >= self.wait {
                    self.cancel.store(true, Ordering::SeqCst);
                    Some(Err(ScriptError::ResultTimeout))
                } else {
                    None
                }
            }
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

impl<T> Drop for Pending<T> {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

impl<T> std::fmt::Debug for Pending<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Pending")
    }
}

pub struct ScriptPool {
    shared: Arc<Shared>,
    supervisor: Mutex<Option<JoinHandle<()>>>,
    config: PoolConfig,
}

impl std::fmt::Debug for ScriptPool {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ScriptPool")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

fn validate(config: &PoolConfig) -> Result<(), PoolError> {
    if config.threads == 0 || config.threads > MAX_THREADS {
        return Err(PoolError::Config("threads"));
    }
    if config.queue_capacity == 0 || config.queue_capacity > MAX_QUEUE_CAPACITY {
        return Err(PoolError::Config("queue_capacity"));
    }
    if config.stack_bytes < MIN_STACK_BYTES {
        return Err(PoolError::Config("stack_bytes"));
    }
    if config.result_grace.is_zero() {
        return Err(PoolError::Config("result_grace"));
    }
    if config.compile_timeout.is_zero() {
        return Err(PoolError::Config("compile_timeout"));
    }
    if config.max_queue_wait.is_zero() {
        return Err(PoolError::Config("max_queue_wait"));
    }
    if config.stuck_grace.is_zero() {
        return Err(PoolError::Config("stuck_grace"));
    }
    if config.max_abandoned_threads > MAX_ABANDONED_THREADS {
        return Err(PoolError::Config("max_abandoned_threads"));
    }
    if config.shutdown_timeout.is_zero() {
        return Err(PoolError::Config("shutdown_timeout"));
    }
    if config.max_running_per_tenant == 0 || config.max_running_per_tenant > MAX_THREADS {
        return Err(PoolError::Config("max_running_per_tenant"));
    }
    if config.max_queued_per_tenant == 0 || config.max_queued_per_tenant > MAX_QUEUE_CAPACITY {
        return Err(PoolError::Config("max_queued_per_tenant"));
    }
    Ok(())
}

impl ScriptPool {
    pub fn new(config: PoolConfig) -> Result<Self, PoolError> {
        validate(&config)?;
        if !allocator_installed() {
            return Err(PoolError::AllocatorNotInstalled);
        }
        warm_up();
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            ready: Condvar::new(),
            supervisor: Condvar::new(),
            stop: Arc::new(AtomicBool::new(false)),
            config,
        });
        let pool = Self {
            shared: Arc::clone(&shared),
            supervisor: Mutex::new(None),
            config,
        };
        for _ in 0..config.threads {
            let spawned = {
                let mut state = shared.lock();
                shared.spawn_worker(&mut state)
            };
            if let Err(error) = spawned {
                pool.shutdown();
                return Err(PoolError::Spawn(error));
            }
        }
        let monitor = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("whl-script-supervisor".to_owned())
                .spawn(move || supervisor_main(&shared))
        };
        match monitor {
            Ok(handle) => {
                *pool
                    .supervisor
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = Some(handle);
            }
            Err(error) => {
                pool.shutdown();
                return Err(PoolError::Spawn(error));
            }
        }
        Ok(pool)
    }

    pub fn config(&self) -> &PoolConfig {
        &self.config
    }

    pub fn queued(&self) -> usize {
        self.shared.lock().queued
    }

    pub fn stats(&self) -> PoolStats {
        let state = self.shared.lock();
        PoolStats {
            workers: state.workers.len(),
            busy: state
                .workers
                .iter()
                .filter(|worker| worker.running.is_some())
                .count(),
            queued: state.queued,
            stuck_workers: state.stuck_total,
            abandoned_threads: state.abandoned,
            crashed_workers: state.crashed_total,
            spawn_failures: state.spawn_failures,
        }
    }

    fn submit_task<T, F>(
        &self,
        lane: Lane,
        wait: Duration,
        budget: Duration,
        task: F,
    ) -> Result<Pending<T>, ScriptError>
    where
        T: Send + 'static,
        F: FnOnce(Arc<AtomicBool>, Arc<AtomicBool>) -> Result<T, ScriptError> + Send + 'static,
    {
        let (sender, receiver) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&self.shared.stop);
        let task_cancel = Arc::clone(&cancel);
        let boxed: Task = Box::new(move |start| match start {
            Start::Run => {
                let result = catch_unwind(AssertUnwindSafe(|| task(task_cancel, stop)))
                    .unwrap_or(Err(ScriptError::Panic));
                let _ = sender.send(result);
            }
            Start::QueueTimeout => {
                let _ = sender.send(Err(ScriptError::QueueTimeout));
            }
        });
        let submitted = Instant::now();
        self.shared.enqueue(
            lane,
            Queued {
                task: boxed,
                enqueued: submitted,
                budget,
            },
        )?;
        Ok(Pending {
            receiver,
            cancel,
            wait,
            submitted,
        })
    }

    #[cfg(test)]
    pub(crate) fn submit_internal<T, F>(
        &self,
        wait: Duration,
        task: F,
    ) -> Result<Pending<T>, ScriptError>
    where
        T: Send + 'static,
        F: FnOnce(Arc<AtomicBool>, Arc<AtomicBool>) -> Result<T, ScriptError> + Send + 'static,
    {
        self.submit_task(Lane::Internal, wait, wait, task)
    }

    fn submit_with(
        &self,
        mut job: ScriptJob,
        extension: Extension<'static>,
    ) -> Result<Pending<ScriptOutput>, ScriptError> {
        job.limits.validate()?;
        let time = job.limits.time;
        let wait = self
            .config
            .max_queue_wait
            .checked_add(time)
            .and_then(|total| total.checked_add(self.config.result_grace))
            .unwrap_or(Duration::MAX);
        let lane = Lane::Tenant(job.tenant.clone());
        self.submit_task(lane, wait, time, move |cancel, stop| {
            let deadline = job
                .clock
                .elapsed()
                .checked_add(job.limits.time)
                .unwrap_or(Duration::MAX);
            let result = execute(&mut job, deadline, cancel, stop, extension);
            drop(job);
            result
        })
    }

    pub fn submit(&self, job: ScriptJob) -> Result<Pending<ScriptOutput>, ScriptError> {
        let extension: Extension<'static> = &no_extension;
        self.submit_with(job, extension)
    }

    pub fn run(&self, job: ScriptJob) -> Result<ScriptOutput, ScriptError> {
        self.submit(job)?.wait()
    }

    pub fn compile(
        &self,
        compiler: &ScriptCompiler,
        source: &str,
    ) -> Result<CompiledScript, ScriptError> {
        if source.len() > compiler.limits().max_source_bytes {
            return compiler.compile(source).map_err(ScriptError::from);
        }
        let compiler = *compiler;
        let source = source.to_owned();
        let timeout = self.config.compile_timeout;
        let wait = self
            .config
            .max_queue_wait
            .saturating_add(timeout)
            .saturating_add(self.config.result_grace);
        let pending = self.submit_task(Lane::Internal, wait, timeout, move |_, _| {
            let window = MemoryWindow::enter(MAX_COMPILE_MEMORY_BYTES);
            let compiled = catch_unwind(AssertUnwindSafe(|| compiler.compile(&source)));
            let exceeded = window.exceeded();
            drop(window);
            match compiled {
                Err(_) => Err(ScriptError::Panic),
                Ok(_) if exceeded => Err(ScriptError::Memory),
                Ok(result) => result.map_err(ScriptError::from),
            }
        })?;
        pending.wait()
    }

    pub fn shutdown(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        let (dropped, handles) = {
            let mut state = self.shared.lock();
            state.closed = true;
            let mut dropped = Vec::new();
            for entry in state.lanes.values_mut() {
                dropped.extend(entry.waiting.drain(..));
            }
            state.lanes.retain(|_, entry| entry.running > 0);
            state.rotation.clear();
            state.queued = 0;
            let handles: Vec<JoinHandle<()>> = state
                .workers
                .iter_mut()
                .filter_map(|worker| worker.handle.take())
                .collect();
            (dropped, handles)
        };
        drop(dropped);
        self.shared.ready.notify_all();
        self.shared.supervisor.notify_all();
        let mut all = handles;
        if let Some(handle) = self
            .supervisor
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            all.push(handle);
        }
        join_within(all, self.config.shutdown_timeout);
    }
}

impl Drop for ScriptPool {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
pub(crate) fn submit_extended(
    pool: &ScriptPool,
    job: ScriptJob,
    extension: Extension<'static>,
) -> Result<Pending<ScriptOutput>, ScriptError> {
    pool.submit_with(job, extension)
}
