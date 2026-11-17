//! Cancellation, reset, shutdown and inspection of background jobs.
//! `cancel_all` spares sidekicks; `reset` and `close` do not.

use super::*;

impl super::SubagentManagerInner {
    pub fn close(&self) -> usize {
        self.state.lock().unwrap().closed = true;
        self.reset()
    }

    /// Cancel ordinary jobs without replacing session-owned sidekicks.
    pub fn cancel_all(&self) -> usize {
        let mut state = self.state.lock().unwrap();
        let ids = state
            .jobs
            .iter()
            .filter(|(_, job)| !job.sidekick)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        let cancelled = ids.len();
        state.generation = state.generation.wrapping_add(1);
        state.cancellation = orca_harness_core::CancellationToken::new();
        let callbacks = ids
            .iter()
            .filter_map(|id| state.jobs.get(id))
            .filter_map(|job| {
                job.cancellation.cancel();
                job.on_cancel
                    .clone()
                    .map(|callback| (callback, job.peak_running))
            })
            .collect::<Vec<_>>();
        for id in &ids {
            state.jobs.remove(id);
            state.unacknowledged.remove(id);
            state.suppressed.insert(*id);
        }
        state.queue.retain(|id| !ids.contains(id));
        drop(state);
        for (callback, peak_running) in callbacks {
            callback(peak_running);
        }
        self.wake();
        cancelled
    }

    /// Replace the session generation and cancel every kind of work.
    pub(crate) fn reset(&self) -> usize {
        let mut state = self.state.lock().unwrap();
        let cancelled = state.jobs.len();
        state.cancellation.cancel();
        for job in state.jobs.values() {
            job.cancellation.cancel();
        }
        state.generation = state.generation.wrapping_add(1);
        state.cancellation = orca_harness_core::CancellationToken::new();
        let callbacks: Vec<_> = state
            .jobs
            .values()
            .filter_map(|job| {
                job.on_cancel
                    .clone()
                    .map(|callback| (callback, job.peak_running))
            })
            .collect();
        state.jobs.clear();
        state.queue.clear();
        state.unacknowledged.clear();
        state.suppressed.clear();
        // Retain live slots: resetting this count would admit replacements
        // before cancelled workers exit, and their later drops would then
        // incorrectly release capacity held by those replacements.
        drop(state);
        for (callback, peak_running) in callbacks {
            callback(peak_running);
        }
        self.wake();
        cancelled
    }

    pub fn cancel(&self, spawn_id: u64) -> bool {
        let state = self.state.lock().unwrap();
        let Some(job) = state.jobs.get(&spawn_id) else {
            return false;
        };
        job.cancellation.cancel();
        let callback = job.on_cancel.clone();
        let peak_running = job.peak_running;
        drop(state);
        if let Some(callback) = callback {
            callback(peak_running);
        }
        true
    }

    pub fn active(&self) -> Vec<BackgroundJob> {
        self.active_visible(true)
    }

    pub(super) fn active_visible(&self, workflows: bool) -> Vec<BackgroundJob> {
        let state = self.state.lock().unwrap();
        let mut jobs = state
            .jobs
            .values()
            .filter(|job| {
                !job.sidekick && (workflows || (job.spawn.run.is_none() && job.on_cancel.is_none()))
            })
            .map(|job| BackgroundJob {
                spawn: job.spawn.clone(),
                status: job.status,
            })
            .collect::<Vec<_>>();
        jobs.sort_by_key(|job| job.spawn.id);
        jobs
    }

    pub fn is_current(&self, generation: u64) -> bool {
        self.state.lock().unwrap().generation == generation
    }

    pub(super) fn live_workers(&self) -> usize {
        let state = self.state.lock().unwrap();
        // Jobs with a slot are counted once through `running`; queued jobs
        // and workflow roots (no slot) are counted through the map.
        let slotted = state
            .jobs
            .values()
            .filter(|job| job.on_cancel.is_none() && job.status == BackgroundStatus::Running)
            .count();
        let retained_idle = state
            .jobs
            .values()
            .filter(|job| job.sidekick && !job.sidekick_active)
            .count();
        state.jobs.len().saturating_sub(retained_idle) + state.running.saturating_sub(slotted)
    }

    pub(crate) fn run_peak(&self, id: u64) -> usize {
        self.state
            .lock()
            .unwrap()
            .jobs
            .get(&id)
            .map_or(0, |job| job.peak_running)
    }

    pub(crate) fn run_ids(&self) -> Vec<u64> {
        let state = self.state.lock().unwrap();
        let mut ids: Vec<_> = state
            .jobs
            .iter()
            .filter(|(_, job)| job.on_cancel.is_some())
            .map(|(id, _)| *id)
            .collect();
        ids.sort_unstable();
        ids
    }

    pub(crate) fn finish(&self, generation: u64, spawn_id: u64) {
        let mut state = self.state.lock().unwrap();
        if state.generation == generation {
            state.jobs.remove(&spawn_id);
            state.queue.retain(|id| *id != spawn_id);
        }
        drop(state);
        self.wake();
    }
}
