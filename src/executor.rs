use core::ops::Range;

/// Minimal executor abstraction used by [crate::Phonk::run_parallel] to distribute the
/// autocorrelation work across multiple threads or CPU cores.
///
/// The library calls [Executor::execute] once per detection run, passing the lag range that
/// needs to be evaluated. The executor is responsible for splitting that range into chunks and
/// invoking `job(from, to)` for each chunk — potentially in parallel. Chunks must be
/// non-overlapping; together they must cover the entire input `range`.
///
/// # Example
///
/// A trivial single-threaded implementation:
///
/// ```no_run
/// use phonk::executor::Executor;
/// use core::ops::Range;
///
/// struct SerialExecutor;
///
/// impl Executor for SerialExecutor {
///     fn execute<F>(&self, range: Range<usize>, job: F)
///     where
///         F: Fn(usize, usize) + Sync,
///     {
///         job(range.start, range.end);
///     }
/// }
/// ```
///
/// A multi-threaded implementation using Rayon would split `range` into per-thread chunks and
/// call `job` concurrently on each.
pub trait Executor {
    /// Execute `job` over the given `range`, splitting it into non-overlapping `[from, to)`
    /// chunks as the implementation sees fit.
    ///
    /// `job` is `Sync` so that it can be shared across threads safely.
    fn execute<F>(&self, range: Range<usize>, job: F)
    where
        F: Fn(usize, usize) + Sync;
}
