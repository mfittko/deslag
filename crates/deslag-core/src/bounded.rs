//! Bounded execution: a parallel job limit and a processing budget.
//!
//! The executor is cooperative and fail-open. A caller that gets a
//! [`Rejected`] passes the original content through. Nothing queues.
//! A permit stays held until the worker thread really ends, so work that
//! outlives its budget still counts against the parallel job limit.

use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::time::Duration;

/// Why [`BoundedExecutor::run`] returned no value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejected {
    /// Every permit is held. No job was started.
    Busy,
    /// The job did not finish inside the processing budget. It keeps its permit until it ends.
    BudgetExceeded,
    Panicked,
}

impl Rejected {
    pub fn as_str(self) -> &'static str {
        match self {
            Rejected::Busy => "busy",
            Rejected::BudgetExceeded => "budget_exceeded",
            Rejected::Panicked => "job_panicked",
        }
    }
}

/// Runs jobs under a parallel job limit and a per-call processing budget.
/// Clones share one permit pool.
#[derive(Clone, Debug)]
pub struct BoundedExecutor {
    max_parallel_jobs: usize,
    processing_budget: Duration,
    held: Arc<Mutex<usize>>,
}

impl Default for BoundedExecutor {
    /// Proposed starting values from the rollout policy: 4 jobs, 10 ms.
    fn default() -> Self {
        Self::new(4, Duration::from_millis(10)).expect("default limits are non-zero")
    }
}

/// Holds one permit. Dropping it releases the permit, also on unwind.
struct Permit(Arc<Mutex<usize>>);

fn lock(held: &Mutex<usize>) -> MutexGuard<'_, usize> {
    // The counter is always valid, so a poisoned lock is still usable.
    held.lock().unwrap_or_else(|e| e.into_inner())
}

impl Drop for Permit {
    fn drop(&mut self) {
        *lock(&self.0) -= 1;
    }
}

impl BoundedExecutor {
    /// `None` when `max_parallel_jobs` or `processing_budget` is zero.
    pub fn new(max_parallel_jobs: usize, processing_budget: Duration) -> Option<Self> {
        (max_parallel_jobs > 0 && !processing_budget.is_zero()).then(|| Self {
            max_parallel_jobs,
            processing_budget,
            held: Arc::new(Mutex::new(0)),
        })
    }

    pub fn max_parallel_jobs(&self) -> usize {
        self.max_parallel_jobs
    }

    pub fn processing_budget(&self) -> Duration {
        self.processing_budget
    }

    /// Permits currently held, including by jobs that outlived their budget.
    pub fn in_flight(&self) -> usize {
        *lock(&self.held)
    }

    fn try_acquire(&self) -> Option<Permit> {
        let mut held = lock(&self.held);
        (*held < self.max_parallel_jobs).then(|| {
            *held += 1;
            Permit(Arc::clone(&self.held))
        })
    }

    /// Runs `job` on its own thread and waits at most the processing budget.
    /// Returns [`Rejected::Busy`] at once, without starting a thread, when no
    /// permit is free.
    // ponytail: thread per job. Use a fixed pool if spawn cost shows in profiles.
    // ponytail: no cancellation inside the scanners. The byte caps in `Limits` bound the worst case.
    pub fn run<T: Send + 'static>(
        &self,
        job: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, Rejected> {
        let permit = self.try_acquire().ok_or(Rejected::Busy)?;
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("deslag-bounded".into())
            .spawn(move || {
                let value = job();
                // Release before sending, so a returned value implies a free permit.
                drop(permit);
                let _ = tx.send(value);
            });
        if spawned.is_err() {
            return Err(Rejected::Busy);
        }
        match rx.recv_timeout(self.processing_budget) {
            Ok(value) => Ok(value),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(Rejected::BudgetExceeded),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(Rejected::Panicked),
        }
    }
}
