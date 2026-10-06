//! The job runner: first-class long operations with progress events and
//! cooperative cancellation (protocol spec §7, ARCH-REVIEW §2 rule 5).
//!
//! Every backup/restore runs as a `Job`: it is registered before the work
//! starts, `job.started` / `job.progress` / `job.completed` ride the
//! regular events stream, and `jobs.cancel` sets a flag the job observes
//! at its next checkpoint — the state transition to `cancelled` is made
//! here, authoritatively, when the task returns.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;
use zamin_protocol::error::{ErrorCode, ProtocolError};
use zamin_protocol::jobs::{Job, JobKind, JobOutcome, JobProgress, JobState};
use zamin_protocol::streams::CoreEvent;

use crate::hub::HubHandle;

/// Finished job records kept for `jobs.list` before the oldest is dropped.
const MAX_FINISHED_JOBS: usize = 50;

/// How a job task can end, other than successfully. Cancellation is
/// data (an outcome), not an error.
#[derive(Debug)]
pub enum JobFailure {
    Cancelled,
    Error(ProtocolError),
}

impl JobFailure {
    fn into_outcome_and_error(self) -> (JobOutcome, Option<ProtocolError>) {
        match self {
            JobFailure::Cancelled => (JobOutcome::Cancelled, None),
            JobFailure::Error(e) => (JobOutcome::Failed, Some(e)),
        }
    }
}

#[derive(Clone)]
struct JobRecord {
    job: Job,
    cancel: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct JobRunner {
    inner: Arc<JobsInner>,
}

struct JobsInner {
    jobs: Mutex<HashMap<Uuid, JobRecord>>,
    order: Mutex<Vec<Uuid>>,
    hub: HubHandle,
}

/// The handle a running task uses to report progress, observe
/// cancellation, and publish it all on the events stream.
#[derive(Clone)]
pub struct JobCtl {
    job_id: Uuid,
    server_id: Option<String>,
    cancel: Arc<AtomicBool>,
    hub: HubHandle,
}

impl JobCtl {
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel)
    }

    pub fn progress(
        &self,
        current: u64,
        total: Option<u64>,
        unit: Option<&str>,
        message: Option<&str>,
    ) {
        let progress = JobProgress {
            current,
            total,
            unit: unit.map(str::to_owned),
            message: message.map(str::to_owned),
        };
        self.hub.publish_event(
            self.server_id.clone(),
            CoreEvent::JobProgress {
                job_id: self.job_id,
                progress,
            },
        );
    }
}

impl JobRunner {
    pub fn new(hub: HubHandle) -> JobRunner {
        JobRunner {
            inner: Arc::new(JobsInner {
                jobs: Mutex::new(HashMap::new()),
                order: Mutex::new(Vec::new()),
                hub,
            }),
        }
    }

    /// Register a job and run `task` on the async runtime. The returned
    /// `Job` is already `running`; progress and completion arrive as
    /// events, and the record stays queryable via `list`/`get`.
    pub fn spawn<F, Fut>(&self, kind: JobKind, server_id: Option<String>, task: F) -> Job
    where
        F: FnOnce(JobCtl) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), JobFailure>> + Send + 'static,
    {
        let job_id = Uuid::now_v7();
        let cancel = Arc::new(AtomicBool::new(false));
        let now = now_ms();
        let mut job = Job {
            job_id,
            kind,
            server_id: server_id.clone(),
            state: JobState::Queued,
            progress: None,
            error: None,
            created_at_ms: now,
            started_at_ms: None,
            ended_at_ms: None,
        };

        {
            let mut jobs = self
                .inner
                .jobs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            jobs.insert(
                job_id,
                JobRecord {
                    job: job.clone(),
                    cancel: Arc::clone(&cancel),
                },
            );
            self.inner
                .order
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(job_id);
        }

        // The daemon runs the task immediately: queued → running before
        // the caller's reply is even written.
        job.state = JobState::Running;
        job.started_at_ms = Some(now);
        self.update_record(job_id, |record| record.job = job.clone());
        self.inner.hub.publish_event(
            server_id.clone(),
            CoreEvent::JobStarted { job: job.clone() },
        );

        let runner = self.clone();
        let ctl = JobCtl {
            job_id,
            server_id,
            cancel,
            hub: self.inner.hub.clone(),
        };
        let future = task(ctl);

        let _task = tokio::spawn(async move {
            let result = future.await;
            let (outcome, error) = match result {
                Ok(()) => (JobOutcome::Succeeded, None),
                Err(failure) => failure.into_outcome_and_error(),
            };
            runner.finish(job_id, outcome, error);
        });

        job
    }

    fn finish(&self, job_id: Uuid, outcome: JobOutcome, error: Option<ProtocolError>) {
        let state = match outcome {
            JobOutcome::Succeeded => JobState::Succeeded,
            JobOutcome::Failed => JobState::Failed,
            JobOutcome::Cancelled => JobState::Cancelled,
        };
        self.update_record(job_id, |record| {
            record.job.state = state;
            record.job.error = error.clone();
            record.job.ended_at_ms = Some(now_ms());
        });
        self.inner.hub.publish_event(
            self.get(job_id).ok().and_then(|j| j.server_id),
            CoreEvent::JobCompleted {
                job_id,
                outcome,
                error,
            },
        );
        self.prune_finished();
    }

    pub fn list(&self) -> Vec<Job> {
        let order = self
            .inner
            .order
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let jobs = self
            .inner
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        order
            .iter()
            .filter_map(|id| jobs.get(id).map(|r| r.job.clone()))
            .collect()
    }

    pub fn get(&self, job_id: Uuid) -> Result<Job, ProtocolError> {
        self.inner
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&job_id)
            .map(|r| r.job.clone())
            .ok_or_else(|| job_not_found(job_id))
    }

    /// Set the cancellation flag; the task observes it at its next
    /// checkpoint. The transition to `cancelled` happens when the task
    /// returns — this method only arms it. Cancelling an already finished
    /// job is a typed rejection.
    pub fn cancel(&self, job_id: Uuid) -> Result<Job, ProtocolError> {
        let record = self
            .inner
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&job_id)
            .cloned()
            .ok_or_else(|| job_not_found(job_id))?;
        if record.job.state != JobState::Queued && record.job.state != JobState::Running {
            return Err(ProtocolError::new(
                ErrorCode::JobNotCancellable,
                format!(
                    "Job {job_id} has already finished ({:?}); nothing to cancel.",
                    record.job.state
                ),
            ));
        }
        record.cancel.store(true, Ordering::Relaxed);
        Ok(record.job)
    }

    fn update_record(&self, job_id: Uuid, update: impl FnOnce(&mut JobRecord)) {
        if let Some(record) = self
            .inner
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(&job_id)
        {
            update(record);
        }
    }

    /// Keep the finished-history bounded (ARCH-REVIEW: bounded everything).
    fn prune_finished(&self) {
        let mut jobs = self
            .inner
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let finished: Vec<Uuid> = self
            .inner
            .order
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter(|id| {
                jobs.get(*id).is_some_and(|r| {
                    matches!(
                        r.job.state,
                        JobState::Succeeded | JobState::Failed | JobState::Cancelled
                    )
                })
            })
            .copied()
            .collect();
        if finished.len() <= MAX_FINISHED_JOBS {
            return;
        }
        let mut order = self
            .inner
            .order
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let excess = finished.len() - MAX_FINISHED_JOBS;
        for id in &finished[..excess] {
            jobs.remove(id);
            order.retain(|o| o != id);
        }
    }
}

fn job_not_found(job_id: Uuid) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::JobNotFound,
        format!("Job {job_id} is unknown (it may have been trimmed from history)."),
    )
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
