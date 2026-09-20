//! The realtime promise, measured: the pitch detector allocates
//! nothing per hop.
//!
//! The vocal worker runs `PitchDetector::detect` every 16 ms beside a
//! 60 FPS game. An allocation there is not a performance detail — it
//! is a lock on the global allocator that a page fault or another
//! thread's free can hold, and the symptom is a dropped analysis
//! block rather than an error anyone would see.
//!
//! The plan asks for an allocation audit before adopting a detector.
//! This is it, and it is a *measurement*: a counting allocator wraps
//! the system one for the whole test binary and the count is read
//! around the call.
//!
//! ⚠️ **The counter is per THREAD, and that is the whole difficulty.**
//! The first version of this file counted into a global `AtomicUsize`
//! and duly reported five allocations inside a loop that makes none —
//! the test harness runs tests in parallel, so the counter was adding
//! up the *other tests'* allocations. A global counter here does not
//! measure what it appears to. It is a thread-local `Cell` with a
//! `const` initialiser, which is also what keeps the counter itself
//! from allocating the first time a thread touches it.
//!
//! ⚠️ And a counting allocator only sees what it is asked for, so the
//! counter is proved to work first — the test allocates on purpose
//! and watches the number move. A counter wired to nothing reports
//! zero and passes for the wrong reason.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use beatbyte_audio::pitch::{PitchConfig, PitchDetector};

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

struct Counting;

// SAFETY: every call is forwarded unchanged to the system allocator;
// the wrapper only bumps a thread-local counter beside it. `try_with`
// rather than `with`, because an allocation can happen while the
// thread's locals are being destroyed, and panicking there would
// abort the process.
#[expect(unsafe_code, reason = "a GlobalAlloc wrapper cannot be written safely")]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn count<T>(body: impl FnOnce() -> T) -> (T, usize) {
    let before = ALLOCATIONS.with(Cell::get);
    let value = body();
    (value, ALLOCATIONS.with(Cell::get) - before)
}

fn tone(hz: f32, n: usize, rate: f32) -> Vec<f32> {
    (0..n)
        .map(|i| 0.4 * (core::f32::consts::TAU * hz * i as f32 / rate).sin())
        .collect()
}

#[test]
fn the_counter_sees_an_allocation_when_there_is_one() {
    let (_, allocations) = count(|| vec![0u8; 4096]);
    assert!(
        allocations > 0,
        "the allocation counter is not counting; every other test here is meaningless"
    );
}

#[test]
fn the_counter_does_not_see_another_threads_allocations() {
    // Why this pin exists: the first version of this file counted
    // into a global `AtomicUsize`, and the test harness runs tests on
    // parallel threads. Without thread-locality every assertion here
    // is a race against whatever else is running.
    //
    // ⚠️ Handing work to the other thread is the hard part, because
    // the obvious ways to do it allocate on THIS thread and read
    // exactly like the leak being looked for. `thread::spawn` costs
    // five allocations (the closure, the handle, the bookkeeping) and
    // `Barrier::wait` four. So the thread is spawned outside the
    // counted window and the handover is two atomics and a spin,
    // which allocate nothing at all.
    let phase = Arc::new(AtomicUsize::new(0));
    let child = std::thread::spawn({
        let phase = Arc::clone(&phase);
        move || {
            while phase.load(Ordering::Acquire) < 1 {
                std::hint::spin_loop();
            }
            let mut sink: Vec<Vec<u8>> = Vec::new();
            for _ in 0..64 {
                sink.push(vec![0u8; 1024]);
            }
            phase.store(2, Ordering::Release);
            sink.len()
        }
    });
    let (_, allocations) = count(|| {
        phase.store(1, Ordering::Release);
        while phase.load(Ordering::Acquire) < 2 {
            std::hint::spin_loop();
        }
    });
    assert_eq!(
        child.join().expect("the thread runs"),
        64,
        "the child really did allocate"
    );
    assert_eq!(
        allocations, 0,
        "another thread's {allocations} allocation(s) leaked into this thread's count"
    );
}

#[test]
fn detecting_a_pitch_allocates_nothing() {
    let config = PitchConfig::default();
    let mut detector = PitchDetector::new(config);
    let rate = config.sample_rate as f32;
    let window = tone(220.0, 2048, rate);

    // One call first: anything lazily set up on the first use is not
    // what this test is about.
    let _ = detector.detect(&window);

    let (frame, allocations) = count(|| detector.detect(&window));
    assert!(frame.voiced, "the window has to be a real estimate");
    assert_eq!(allocations, 0, "detect() allocated {allocations} time(s)");

    // And over a run of hops, which is how it is actually used: an
    // allocation every thousandth call would still be a stall.
    let (_, allocations) = count(|| {
        let mut voiced = 0;
        for hop in 0..600usize {
            let start = (hop * 16) % 1024;
            if detector.detect(&window[start..start + 1024]).voiced {
                voiced += 1;
            }
        }
        voiced
    });
    assert_eq!(allocations, 0, "600 hops allocated {allocations} time(s)");
}

#[test]
fn the_quiet_and_the_clipped_paths_allocate_nothing_either() {
    // The early returns have their own code. Silence is the commonest
    // frame there is, and a clipped one takes a third path.
    let mut detector = PitchDetector::new(PitchConfig::default());
    let silence = vec![0.0f32; 2048];
    let rails = vec![1.0f32; 2048];
    let short = vec![0.2f32; 64];
    let _ = detector.detect(&silence);
    let _ = detector.detect(&rails);
    let _ = detector.detect(&short);
    let (_, allocations) = count(|| {
        let a = detector.detect(&silence);
        let b = detector.detect(&rails);
        let c = detector.detect(&short);
        (a.voiced, b.clipped, c.voiced)
    });
    assert_eq!(
        allocations, 0,
        "the quiet paths allocated {allocations} time(s)"
    );
}

#[test]
fn building_a_detector_is_where_the_memory_is_taken() {
    // The other half of the promise: the scratch exists, it is just
    // taken up front. If this ever reads zero the detector has
    // stopped preallocating and the tests above are hollow.
    let (_, allocations) = count(|| PitchDetector::new(PitchConfig::default()));
    assert!(
        allocations > 0,
        "the detector allocated nothing at construction — where is its scratch?"
    );
}
