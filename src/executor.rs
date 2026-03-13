use core::ops::Range;

/// Minimal executor abstraction for parallel work distribution.
///
/// The executor receives a range and invokes `job(from, to)` for each chunk it schedules.
/// Implementations may run jobs sequentially or in parallel.
pub trait Executor {
    fn execute<F>(&self, range: Range<usize>, job: F)
    where
        F: Fn(usize, usize) + Sync;
}
