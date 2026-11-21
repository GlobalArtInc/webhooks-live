#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout};
use std::cell::Cell;
use std::marker::PhantomData;

const PROBE_BYTES: usize = 4096;

#[derive(Clone, Copy)]
struct Snapshot {
    active: bool,
    live: i64,
    peak: i64,
    limit: i64,
    exceeded: bool,
}

struct Account {
    active: Cell<bool>,
    live: Cell<i64>,
    peak: Cell<i64>,
    limit: Cell<i64>,
    exceeded: Cell<bool>,
}

impl Account {
    const fn idle() -> Self {
        Self {
            active: Cell::new(false),
            live: Cell::new(0),
            peak: Cell::new(0),
            limit: Cell::new(i64::MAX),
            exceeded: Cell::new(false),
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            active: self.active.get(),
            live: self.live.get(),
            peak: self.peak.get(),
            limit: self.limit.get(),
            exceeded: self.exceeded.get(),
        }
    }

    fn restore(&self, snapshot: Snapshot) {
        self.active.set(snapshot.active);
        self.live.set(snapshot.live);
        self.peak.set(snapshot.peak);
        self.limit.set(snapshot.limit);
        self.exceeded.set(snapshot.exceeded);
    }

    fn add(&self, size: usize) {
        if !self.active.get() {
            return;
        }
        let live = self.live.get().saturating_add(to_signed(size));
        self.live.set(live);
        if live > self.peak.get() {
            self.peak.set(live);
        }
        if live > self.limit.get() {
            self.exceeded.set(true);
        }
    }

    fn sub(&self, size: usize) {
        if self.active.get() {
            let live = self.live.get().saturating_sub(to_signed(size));
            self.live.set(live.max(0));
        }
    }
}

thread_local! {
    static ACCOUNT: Account = const { Account::idle() };
}

fn to_signed(size: usize) -> i64 {
    i64::try_from(size).unwrap_or(i64::MAX)
}

fn record_alloc(size: usize) {
    let _ = ACCOUNT.try_with(|account| account.add(size));
}

fn record_free(size: usize) {
    let _ = ACCOUNT.try_with(|account| account.sub(size));
}

#[derive(Debug, Default)]
pub struct CountingAlloc<A> {
    inner: A,
}

impl<A> CountingAlloc<A> {
    pub const fn new(inner: A) -> Self {
        Self { inner }
    }
}

unsafe impl<A: GlobalAlloc> GlobalAlloc for CountingAlloc<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { self.inner.alloc(layout) };
        if !pointer.is_null() {
            record_alloc(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { self.inner.dealloc(pointer, layout) };
        record_free(layout.size());
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { self.inner.alloc_zeroed(layout) };
        if !pointer.is_null() {
            record_alloc(layout.size());
        }
        pointer
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let moved = unsafe { self.inner.realloc(pointer, layout, new_size) };
        if !moved.is_null() {
            record_free(layout.size());
            record_alloc(new_size);
        }
        moved
    }
}

#[derive(Debug)]
pub struct MemoryWindow {
    previous: PreviousWindow,
    not_send: PhantomData<*const ()>,
}

struct PreviousWindow(Snapshot);

impl std::fmt::Debug for PreviousWindow {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PreviousWindow")
    }
}

impl MemoryWindow {
    pub fn enter(limit_bytes: usize) -> Self {
        let previous = ACCOUNT.with(|account| {
            let previous = account.snapshot();
            account.restore(Snapshot {
                active: true,
                live: 0,
                peak: 0,
                limit: to_signed(limit_bytes),
                exceeded: false,
            });
            previous
        });
        Self {
            previous: PreviousWindow(previous),
            not_send: PhantomData,
        }
    }

    pub fn live_bytes(&self) -> usize {
        ACCOUNT.with(|account| usize::try_from(account.live.get().max(0)).unwrap_or(0))
    }

    pub fn peak_bytes(&self) -> usize {
        ACCOUNT.with(|account| usize::try_from(account.peak.get().max(0)).unwrap_or(0))
    }

    pub fn exceeded(&self) -> bool {
        memory_exceeded()
    }
}

impl Drop for MemoryWindow {
    fn drop(&mut self) {
        let previous = self.previous.0;
        let _ = ACCOUNT.try_with(|account| account.restore(previous));
    }
}

pub fn memory_exceeded() -> bool {
    ACCOUNT
        .try_with(|account| account.active.get() && account.exceeded.get())
        .unwrap_or(false)
}

pub fn allocator_installed() -> bool {
    let window = MemoryWindow::enter(usize::MAX);
    let probe: Vec<u8> = Vec::with_capacity(PROBE_BYTES);
    let probe = std::hint::black_box(probe);
    let counted = window.live_bytes() >= PROBE_BYTES;
    drop(probe);
    counted
}
