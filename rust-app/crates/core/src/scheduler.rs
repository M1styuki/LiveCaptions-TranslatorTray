use crate::session::WorkKey;
use std::collections::{HashMap, VecDeque};

pub const MAX_IN_FLIGHT: usize = 2;
pub const MAX_COMPLETE_WAITING: usize = 32;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    pub key: WorkKey,
    pub source: String,
    pub complete: bool,
    pub sequence: u64,
    pub created_at: i64,
}
#[derive(Default)]
pub struct Offer {
    pub cancel: Vec<WorkKey>,
    pub superseded: Vec<WorkKey>,
    pub spill: Option<Task>,
}
#[derive(Default)]
pub struct Scheduler {
    in_flight: HashMap<u64, Task>,
    complete: VecDeque<Task>,
    partial: Option<Task>,
    has_spill: bool,
}
impl Scheduler {
    pub fn offer(&mut self, mut task: Task) -> Offer {
        let mut action = Offer::default();
        if let Some(running) = self.in_flight.get(&task.key.sentence_id) {
            if running.key.revision >= task.key.revision {
                return action;
            }
            action.cancel.push(running.key.clone());
            // Keep the slot occupied until the cancelled HTTP future terminates.
        }
        if let Some(waiting) = self
            .complete
            .iter_mut()
            .find(|waiting| waiting.key.sentence_id == task.key.sentence_id)
        {
            if task.key.revision > waiting.key.revision {
                task.sequence = waiting.sequence;
                *waiting = task;
            }
            return action;
        }
        if let Some(waiting) = &mut self.partial
            && waiting.key.sentence_id == task.key.sentence_id
        {
            if task.key.revision <= waiting.key.revision {
                return action;
            }
            task.sequence = waiting.sequence;
            self.partial = None;
        }
        if !task.complete {
            if let Some(previous) = self.partial.take() {
                action.superseded.push(previous.key);
            }
            self.partial = Some(task);
            return action;
        }
        if self.has_spill || self.complete.len() == MAX_COMPLETE_WAITING {
            self.has_spill = true;
            action.spill = Some(task);
        } else {
            self.complete.push_back(task);
        }
        action
    }
    pub fn next_ready(&mut self) -> Option<Task> {
        if self.in_flight.len() == MAX_IN_FLIGHT {
            return None;
        }
        let ready = self
            .complete
            .iter()
            .position(|task| !self.in_flight.contains_key(&task.key.sentence_id));
        let use_partial = self.partial.as_ref().is_some_and(|partial| {
            !self.in_flight.contains_key(&partial.key.sentence_id)
                && ready.is_none_or(|position| partial.sequence < self.complete[position].sequence)
        }) && !self.has_spill;
        let task = if use_partial {
            self.partial.take()
        } else {
            ready.and_then(|position| self.complete.remove(position))
        }?;
        self.in_flight.insert(task.key.sentence_id, task.clone());
        Some(task)
    }
    pub fn finished(&mut self, key: &WorkKey) {
        if self
            .in_flight
            .get(&key.sentence_id)
            .is_some_and(|task| task.key == *key)
        {
            self.in_flight.remove(&key.sentence_id);
        }
    }
    /// Storage transfers its oldest persisted row only while a slot is free.
    pub fn restore(&mut self, task: Task) -> Result<(), Task> {
        if self.complete.len() == MAX_COMPLETE_WAITING {
            return Err(task);
        }
        let position = self
            .complete
            .iter()
            .position(|waiting| waiting.sequence > task.sequence)
            .unwrap_or(self.complete.len());
        self.complete.insert(position, task);
        Ok(())
    }
    pub fn storage_drained(&mut self) {
        self.has_spill = false;
    }
    pub fn waiting(&self) -> usize {
        self.complete.len()
    }
    pub fn in_flight(&self) -> usize {
        self.in_flight.len()
    }
    pub fn has_spill(&self) -> bool {
        self.has_spill
    }
    pub fn running_key(&self, sentence: u64) -> Option<&WorkKey> {
        self.in_flight.get(&sentence).map(|task| &task.key)
    }
    pub fn stop(&mut self) -> Vec<WorkKey> {
        let cancel = self
            .in_flight
            .values()
            .map(|task| task.key.clone())
            .collect();
        self.in_flight.clear();
        self.complete.clear();
        self.partial = None;
        self.has_spill = false;
        cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn task(sentence: u64, revision: u64) -> Task {
        Task {
            key: WorkKey {
                instance_id: "test".into(),
                session_id: 1,
                sentence_id: sentence,
                revision,
                config_revision: 0,
            },
            source: format!("Sentence {sentence} revision {revision}."),
            complete: true,
            sequence: sentence,
            created_at: 0,
        }
    }
    #[test]
    fn forty_complete_sentences_keep_two_requests_and_thirty_two_in_memory_without_loss() {
        let mut scheduler = Scheduler::default();
        let mut spilled = VecDeque::new();
        let mut running = VecDeque::new();
        let mut completed = Vec::new();
        for sentence in 1..=40 {
            if let Some(task) = scheduler.offer(task(sentence, 1)).spill {
                spilled.push_back(task);
            }
            while let Some(task) = scheduler.next_ready() {
                running.push_back(task);
            }
            assert!(scheduler.in_flight() <= 2);
            assert!(scheduler.waiting() <= 32);
        }
        assert_eq!(spilled.len(), 6);
        assert_eq!(scheduler.in_flight(), 2);
        assert_eq!(scheduler.waiting(), 32);
        while let Some(task) = running.pop_front() {
            completed.push(task.key.sentence_id);
            scheduler.finished(&task.key);
            while scheduler.waiting() < 32 {
                let Some(task) = spilled.pop_front() else {
                    scheduler.storage_drained();
                    break;
                };
                scheduler.restore(task).unwrap();
            }
            while let Some(task) = scheduler.next_ready() {
                running.push_back(task);
            }
        }
        assert_eq!(completed, (1..=40).collect::<Vec<_>>());
    }
    #[test]
    fn revised_in_flight_request_is_cancelled_before_reusing_its_slot() {
        let mut scheduler = Scheduler::default();
        scheduler.offer(task(1, 1));
        let old = scheduler.next_ready().unwrap();
        scheduler.offer(task(2, 1));
        let second = scheduler.next_ready().unwrap();
        let offer = scheduler.offer(task(1, 2));
        assert_eq!(offer.cancel, vec![old.key.clone()]);
        assert!(scheduler.next_ready().is_none());
        scheduler.finished(&old.key);
        let revised = scheduler.next_ready().unwrap();
        assert_eq!(revised.key.revision, 2);
        assert_eq!(scheduler.in_flight(), 2);
        scheduler.finished(&old.key);
        assert_eq!(scheduler.in_flight(), 2);
        scheduler.finished(&second.key);
        scheduler.offer(task(3, 1));
        scheduler.offer(task(3, 2));
        scheduler.offer(task(3, 2));
        assert_eq!(scheduler.waiting(), 1);
        assert_eq!(scheduler.next_ready().unwrap().key.revision, 2);
        let mut scheduler = Scheduler::default();
        scheduler.offer(task(1, 1));
        let first = scheduler.next_ready().unwrap();
        scheduler.offer(task(1, 2));
        assert!(scheduler.next_ready().is_none());
        scheduler.finished(&first.key);
        assert_eq!(scheduler.next_ready().unwrap().key.revision, 2);
    }
}
