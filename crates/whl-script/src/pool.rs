use std::collections::VecDeque;
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
use crate::exec::{Extension, ScriptJob, execute, no_extension};
use crate::limits::MAX_COMPILE_MEMORY_BYTES;
use crate::output::ScriptOutput;

const MIN_STACK_BYTES: usize = 2 * 1024 * 1024;
const MAX_THREADS: usize = 64;
const MAX_QUEUE_CAPACITY: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolConfig {
    pub threads: usize,
    pub queue_capacity: usize,
    pub stack_bytes: usize,
    pub result_grace: Duration,
    pub compile_timeout: Duration,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            threads: 2,
            queue_capacity: 64,
            stack_bytes: 8 * 1024 * 1024,
            result_grace: Duration::from_millis(250),
            compile_timeout: Duration::from_secs(2),
        }
    }
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

type Task = Box<dyn FnOnce() + Send + 'static>;

struct QueueState {
    tasks: VecDeque<Task>,
    closed: bool,
}

struct Shared {
    queue: Mutex<QueueState>,
    ready: Condvar,
    stop: Arc<AtomicBool>,
    capacity: usize,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, QueueState> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
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
    workers: Mutex<Vec<JoinHandle<()>>>,
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
    Ok(())
}

fn worker(shared: &Shared) {
    loop {
        let task = {
            let mut state = shared.lock();
            loop {
                if let Some(task) = state.tasks.pop_front() {
                    break Some(task);
                }
                if state.closed {
                    break None;
                }
                state = shared
                    .ready
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        };
        match task {
            Some(task) => {
                let _ = catch_unwind(AssertUnwindSafe(task));
            }
            None => return,
        }
    }
}

impl ScriptPool {
    pub fn new(config: PoolConfig) -> Result<Self, PoolError> {
        validate(&config)?;
        if !allocator_installed() {
            return Err(PoolError::AllocatorNotInstalled);
        }
        warm_up();
        let shared = Arc::new(Shared {
            queue: Mutex::new(QueueState {
                tasks: VecDeque::new(),
                closed: false,
            }),
            ready: Condvar::new(),
            stop: Arc::new(AtomicBool::new(false)),
            capacity: config.queue_capacity,
        });
        let pool = Self {
            shared: Arc::clone(&shared),
            workers: Mutex::new(Vec::new()),
            config,
        };
        for index in 0..config.threads {
            let handle = std::thread::Builder::new()
                .name(format!("whl-script-{index}"))
                .stack_size(config.stack_bytes)
                .spawn({
                    let shared = Arc::clone(&shared);
                    move || worker(&shared)
                });
            match handle {
                Ok(handle) => pool
                    .workers
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(handle),
                Err(error) => {
                    pool.shutdown();
                    return Err(PoolError::Spawn(error));
                }
            }
        }
        Ok(pool)
    }

    pub fn config(&self) -> &PoolConfig {
        &self.config
    }

    pub fn queued(&self) -> usize {
        self.shared.lock().tasks.len()
    }

    pub(crate) fn submit_task<T, F>(
        &self,
        wait: Duration,
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
        let boxed: Task = Box::new(move || {
            let result = catch_unwind(AssertUnwindSafe(|| task(task_cancel, stop)))
                .unwrap_or(Err(ScriptError::Panic));
            let _ = sender.send(result);
        });
        {
            let mut state = self.shared.lock();
            if state.closed {
                return Err(ScriptError::ShutDown);
            }
            if state.tasks.len() >= self.shared.capacity {
                return Err(ScriptError::Overloaded);
            }
            state.tasks.push_back(boxed);
        }
        self.shared.ready.notify_one();
        Ok(Pending {
            receiver,
            cancel,
            wait,
            submitted: Instant::now(),
        })
    }

    fn submit_with(
        &self,
        mut job: ScriptJob,
        extension: &'static (dyn Fn(&mut rhai::Engine) + Sync),
    ) -> Result<Pending<ScriptOutput>, ScriptError> {
        job.limits.validate()?;
        let deadline = job
            .clock
            .elapsed()
            .checked_add(job.limits.time)
            .unwrap_or(Duration::MAX);
        let wait = job
            .limits
            .time
            .checked_add(self.config.result_grace)
            .unwrap_or(Duration::MAX);
        self.submit_task(wait, move |cancel, stop| {
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
        let pending = self.submit_task(self.config.compile_timeout, move |_, _| {
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
        let dropped: Vec<Task> = {
            let mut state = self.shared.lock();
            state.closed = true;
            state.tasks.drain(..).collect()
        };
        drop(dropped);
        self.shared.ready.notify_all();
        let handles: Vec<JoinHandle<()>> =
            std::mem::take(&mut *self.workers.lock().unwrap_or_else(PoisonError::into_inner));
        for handle in handles {
            if handle.thread().id() != std::thread::current().id() {
                let _ = handle.join();
            }
        }
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
    extension: &'static (dyn Fn(&mut rhai::Engine) + Sync),
) -> Result<Pending<ScriptOutput>, ScriptError> {
    pool.submit_with(job, extension)
}
