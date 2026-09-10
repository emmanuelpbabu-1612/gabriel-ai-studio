use tokio::sync::mpsc;

use crate::types::{Job, Priority};

#[derive(Debug)]
pub struct Scheduler {
    high: mpsc::Sender<Job>,
    standard: mpsc::Sender<Job>,
}

pub struct SchedulerReceiver {
    pub high: mpsc::Receiver<Job>,
    pub standard: mpsc::Receiver<Job>,
}

impl Scheduler {
    pub fn new(queue_capacity: usize) -> (Self, SchedulerReceiver) {
        let (high_tx, high_rx) = mpsc::channel(queue_capacity);
        let (std_tx, std_rx) = mpsc::channel(queue_capacity);
        (
            Self {
                high: high_tx,
                standard: std_tx,
            },
            SchedulerReceiver {
                high: high_rx,
                standard: std_rx,
            },
        )
    }

    pub async fn submit(&self, job: Job) -> Result<(), Job> {
        match job.priority {
            Priority::Interactive => self.high.send(job).await.map_err(|e| e.0),
            Priority::Standard => self.standard.send(job).await.map_err(|e| e.0),
        }
    }

    #[allow(dead_code)]
    pub fn try_submit(&self, job: Job) -> Result<(), Job> {
        match job.priority {
            Priority::Interactive => self.high.try_send(job).map_err(|e| e.into_inner()),
            Priority::Standard => self.standard.try_send(job).map_err(|e| e.into_inner()),
        }
    }
}

pub mod dispatch {


    use crate::core::engine::EngineState;
    use crate::types::Job;

    use super::{SchedulerReceiver, Priority};

    async fn recv_biased(
        rx: &mut SchedulerReceiver,
        high_open: &mut bool,
        standard_open: &mut bool,
    ) -> Option<Job> {
        if *high_open && *standard_open {
            tokio::select! {
                biased;
                job = rx.high.recv() => match job {
                    Some(j) => Some(j),
                    None => { *high_open = false; None }
                },
                job = rx.standard.recv() => match job {
                    Some(j) => Some(j),
                    None => { *standard_open = false; None }
                },
            }
        } else if *high_open {
            match rx.high.recv().await {
                Some(j) => Some(j),
                None => {
                    *high_open = false;
                    None
                }
            }
        } else if *standard_open {
            rx.standard.recv().await
        } else {
            None
        }
    }

    pub async fn run(mut rx: SchedulerReceiver, engine: EngineState) {
        tracing::info!(
            "scheduler online: interactive lane (priority -1) preempts standard lane (priority 0)"
        );
        let mut high_open = true;
        let mut standard_open = true;

        loop {
            let Some(job) = recv_biased(&mut rx, &mut high_open, &mut standard_open).await else {
                if !high_open && !standard_open {
                    break;
                }
                continue;
            };

            tracing::debug!(
                job_id = %job.id,
                priority = job.priority.level(),
                model = %job.model_id,
                lane = if job.priority == Priority::Interactive { "interactive" } else { "standard" },
                "dequeued job"
            );

            let inner = engine.inner_arc();
            tokio::spawn(async move {
                inner.execute(job).await;
            });
        }

        tracing::warn!("scheduler dispatcher shutting down");
    }
}
