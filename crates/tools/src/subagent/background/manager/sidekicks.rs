//! Sidekick admission, queueing and idle/live bookkeeping.

use super::*;

impl super::SubagentManagerInner {
    pub(crate) fn admit_sidekick(
        self: &Arc<Self>,
        spawn: &SubagentSpawn,
        completion: bool,
    ) -> Result<Admission, &'static str> {
        let admission = self.admit_job(spawn, None, None, true)?;
        if !completion {
            self.state.lock().unwrap().unacknowledged.remove(&spawn.id);
        }
        Ok(admission)
    }

    pub(crate) fn queue_sidekick(
        self: &Arc<Self>,
        _generation: u64,
        spawn_id: u64,
        completion: bool,
    ) -> Result<Option<Slot>, &'static str> {
        let mut state = self.state.lock().unwrap();
        if state.closed {
            return Err("sidekick is stopped");
        }
        let Some(job) = state.jobs.get(&spawn_id) else {
            return Err("sidekick is stopped");
        };
        if job.cancellation.is_cancelled() || !job.sidekick {
            return Err("sidekick is stopped");
        }
        if completion {
            if let Some(capacity) = state.completion_capacity {
                if state.unacknowledged.values().sum::<usize>() >= capacity {
                    return Err("background completion capacity reached; consume pending subagent results before spawning more");
                }
                *state.unacknowledged.entry(spawn_id).or_default() += 1;
            }
        }
        let job = state.jobs.get_mut(&spawn_id).unwrap();
        job.status = BackgroundStatus::Queued;
        job.sidekick_active = true;
        state.queue.push_back(spawn_id);
        drop(state);
        let cap = self.settings.background_limit() as usize;
        let slot = self
            .try_start(spawn_id, cap)
            .then(|| Slot(self.clone(), None));
        if slot.is_none() {
            self.wake();
        }
        Ok(slot)
    }

    pub(crate) fn sidekick_idle(&self, _generation: u64, spawn_id: u64) {
        let mut state = self.state.lock().unwrap();
        if let Some(job) = state.jobs.get_mut(&spawn_id) {
            job.status = BackgroundStatus::Queued;
            job.sidekick_active = false;
        }
        state.queue.retain(|id| *id != spawn_id);
        drop(state);
        self.wake();
    }

    pub(crate) fn finish_sidekick(&self, _generation: u64, spawn_id: u64) {
        let mut state = self.state.lock().unwrap();
        state.jobs.remove(&spawn_id);
        state.queue.retain(|id| *id != spawn_id);
        state.unacknowledged.remove(&spawn_id);
        state.suppressed.insert(spawn_id);
        drop(state);
        self.wake();
    }

    pub(crate) fn sidekick_live(&self, _generation: u64, spawn_id: u64) -> bool {
        let state = self.state.lock().unwrap();
        state
            .jobs
            .get(&spawn_id)
            .is_some_and(|job| job.sidekick && !job.cancellation.is_cancelled())
    }
}
