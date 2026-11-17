//! Admission of background runs and stages: the queue, the slot cap, and
//! the wait for a free slot.

use super::*;

impl super::SubagentManagerInner {
    /// Register a job at the back of the queue and start it right away if
    /// it is already at the front with a free slot.
    pub(in super::super) fn admit(
        self: &Arc<Self>,
        spawn: &SubagentSpawn,
    ) -> Result<Admission, &'static str> {
        self.admit_job(spawn, None, None, false)
    }

    pub(crate) fn admit_run(
        self: &Arc<Self>,
        spawn: &SubagentSpawn,
        on_cancel: Arc<dyn Fn(usize) + Send + Sync>,
    ) -> Result<Admission, &'static str> {
        self.admit_job(spawn, Some(on_cancel), None, false)
    }

    pub(crate) fn admit_stage(
        self: &Arc<Self>,
        spawn: &SubagentSpawn,
        generation: u64,
    ) -> Result<Admission, &'static str> {
        self.admit_job(spawn, None, Some(generation), false)
    }

    pub(super) fn admit_job(
        self: &Arc<Self>,
        spawn: &SubagentSpawn,
        on_cancel: Option<Arc<dyn Fn(usize) + Send + Sync>>,
        expected: Option<u64>,
        sidekick: bool,
    ) -> Result<Admission, &'static str> {
        let cap = self.settings.background_limit() as usize;
        let mut state = self.state.lock().unwrap();
        if state.closed {
            return Err("session is shut down");
        }
        if expected.is_some_and(|generation| generation != state.generation) {
            return Err("workflow generation was cancelled");
        }
        if let Some(capacity) = state.completion_capacity.filter(|_| spawn.run.is_none()) {
            if state.unacknowledged.values().sum::<usize>() >= capacity {
                return Err("background completion capacity reached; consume pending subagent results before spawning more");
            }
            *state.unacknowledged.entry(spawn.id).or_default() += 1;
        }
        let cancellation = state.cancellation.child_token();
        let no_slot = on_cancel.is_some();
        state.jobs.insert(
            spawn.id,
            ActiveSubagent {
                spawn: spawn.clone(),
                cancellation: cancellation.clone(),
                status: if no_slot {
                    BackgroundStatus::Running
                } else {
                    BackgroundStatus::Queued
                },
                on_cancel,
                peak_running: 0,
                running_children: 0,
                sidekick,
                sidekick_active: sidekick,
            },
        );
        if !no_slot {
            state.queue.push_back(spawn.id);
        }
        let generation = state.generation;
        drop(state);
        let slot =
            (!no_slot && self.try_start(spawn.id, cap)).then(|| Slot(self.clone(), spawn.run));
        Ok(Admission {
            manager: self.clone(),
            generation,
            spawn_id: spawn.id,
            cancellation,
            slot,
            transferred: false,
        })
    }

    /// Take a slot if this spawn heads the queue and one is free under `cap`.
    pub(super) fn try_start(&self, spawn_id: u64, cap: usize) -> bool {
        let mut state = self.state.lock().unwrap();
        if state
            .jobs
            .get(&spawn_id)
            .is_none_or(|job| job.cancellation.is_cancelled())
            || (cap != 0 && state.running >= cap)
            || state.queue.front() != Some(&spawn_id)
        {
            return false;
        }
        state.queue.pop_front();
        state.running += 1;
        if let Some(job) = state.jobs.get_mut(&spawn_id) {
            job.status = BackgroundStatus::Running;
        }
        if let Some(run) = state.jobs.get(&spawn_id).and_then(|job| job.spawn.run) {
            if let Some(root) = state.jobs.get_mut(&run) {
                root.running_children += 1;
                root.peak_running = root.peak_running.max(root.running_children);
            }
        }
        let next_can_start = !state.queue.is_empty() && (cap == 0 || state.running < cap);
        drop(state);
        if next_can_start {
            self.wake();
        }
        true
    }

    fn dequeue(&self, spawn_id: u64) {
        let mut state = self.state.lock().unwrap();
        let was_head = state.queue.front() == Some(&spawn_id);
        state.queue.retain(|id| *id != spawn_id);
        drop(state);
        if was_head {
            self.wake();
        }
    }

    pub(crate) async fn acquire(
        self: &Arc<Self>,
        spawn_id: u64,
        cancellation: &orca_harness_core::CancellationToken,
    ) -> Option<Slot> {
        let mut limit = self.settings.background_limit_watch();
        let mut slots = self.slots.subscribe();
        loop {
            // Mark both watches seen before checking, so a change landing
            // between the check and the wait still wakes this loop.
            let cap = *limit.borrow_and_update() as usize;
            let _ = slots.borrow_and_update();
            if self.try_start(spawn_id, cap) {
                let run = self
                    .state
                    .lock()
                    .unwrap()
                    .jobs
                    .get(&spawn_id)
                    .and_then(|job| job.spawn.run);
                return Some(Slot(self.clone(), run));
            }
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => {
                    self.dequeue(spawn_id);
                    return None;
                }
                _ = slots.changed() => {}
                _ = limit.changed() => {}
            }
        }
    }
}
