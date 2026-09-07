//! Small bounded runtime for blocking application jobs.
//!
//! RockCast uses blocking audio and Cast APIs. A fixed worker set keeps those
//! operations off the egui thread without creating an unbounded OS thread for
//! every click, delayed tap, or channel forwarder.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

use parking_lot::Mutex;

type Job = Box<dyn FnOnce(CancelToken) + Send + 'static>;

const MAX_QUEUED_JOBS: usize = 128;

#[derive(Clone)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

pub struct BackgroundRuntime {
    tx: Option<mpsc::SyncSender<Job>>,
    cancelled: Arc<AtomicBool>,
    workers: Vec<thread::JoinHandle<()>>,
}

impl BackgroundRuntime {
    pub fn new(worker_count: usize) -> Self {
        Self::new_named(worker_count, "rockcast-bg")
    }

    pub fn new_named(worker_count: usize, thread_name: &str) -> Self {
        let worker_count = worker_count.max(2);
        let (tx, rx) = mpsc::sync_channel::<Job>(MAX_QUEUED_JOBS);
        let rx = Arc::new(Mutex::new(rx));
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut workers = Vec::with_capacity(worker_count);

        for index in 0..worker_count {
            let rx = Arc::clone(&rx);
            let cancelled = Arc::clone(&cancelled);
            let name = format!("{thread_name}-{index}");
            workers.push(
                thread::Builder::new()
                    .name(name)
                    .spawn(move || {
                        loop {
                            // Keep the receiver mutex strictly around recv. A
                            // temporary guard in the `match` scrutinee lives to
                            // the end of the match and would serialize all jobs.
                            let received = { rx.lock().recv() };
                            match received {
                                Ok(job) => {
                                    if !cancelled.load(Ordering::Acquire) {
                                        job(CancelToken(Arc::clone(&cancelled)));
                                    }
                                }
                                Err(_) => break,
                            }
                        }
                    })
                    .expect("spawn RockCast background worker"),
            );
        }

        Self {
            tx: Some(tx),
            cancelled,
            workers,
        }
    }

    pub fn spawn(
        &self,
        job: impl FnOnce(CancelToken) + Send + 'static,
    ) -> Result<(), &'static str> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err("runtime is shutting down");
        }
        self.tx
            .as_ref()
            .ok_or("runtime is shutting down")?
            .try_send(Box::new(job))
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => "runtime queue is full",
                mpsc::TrySendError::Disconnected(_) => "runtime workers stopped",
            })
    }

    pub fn cancel_token(&self) -> CancelToken {
        CancelToken(Arc::clone(&self.cancelled))
    }

    pub fn shutdown(&mut self) {
        if self.cancelled.swap(true, Ordering::AcqRel) {
            return;
        }
        self.tx.take();
        // Blocking HTTP implementations may still be inside an OS read. Do not
        // join here: window close must remain bounded. Workers observe the token
        // before accepting another job and the process exit remains the final
        // safety boundary on Windows.
        self.workers.clear();
    }
}

impl Drop for BackgroundRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn bounded_runtime_runs_jobs_and_cancels() {
        let mut runtime = BackgroundRuntime::new(2);
        let (tx, rx) = mpsc::channel();
        runtime.spawn(move |_| tx.send(7).unwrap()).unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), 7);
        let token = runtime.cancel_token();
        runtime.shutdown();
        assert!(token.is_cancelled());
        assert!(runtime.spawn(|_| {}).is_err());
    }

    #[test]
    fn workers_execute_jobs_concurrently() {
        let mut runtime = BackgroundRuntime::new(2);
        let (first_started_tx, first_started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (second_done_tx, second_done_rx) = mpsc::channel();
        runtime
            .spawn(move |_| {
                first_started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            })
            .unwrap();
        first_started_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        runtime
            .spawn(move |_| second_done_tx.send(()).unwrap())
            .unwrap();
        second_done_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        release_tx.send(()).unwrap();
        runtime.shutdown();
    }
}
