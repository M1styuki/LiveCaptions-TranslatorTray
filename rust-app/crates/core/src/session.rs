use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkKey {
    pub instance_id: String,
    pub session_id: u64,
    pub sentence_id: u64,
    pub revision: u64,
    pub config_revision: u64,
}
pub struct Session {
    instance: String,
    number: u64,
    config: u64,
    active: bool,
    revisions: HashMap<u64, u64>,
    latest: Option<(u64, u64)>,
    released_current: bool,
}
impl Session {
    pub fn new(instance: String) -> Self {
        Self {
            instance,
            number: 0,
            config: 0,
            active: false,
            revisions: HashMap::new(),
            latest: None,
            released_current: false,
        }
    }
    pub fn start(&mut self) {
        self.stop();
        self.number += 1;
        self.active = true;
    }
    pub fn identity(&self) -> (&str, u64, u64) {
        (&self.instance, self.number, self.config)
    }
    pub fn stop(&mut self) {
        self.active = false;
        self.revisions.clear();
        self.latest = None;
        self.released_current = false;
    }
    pub fn reconfigure(&mut self) {
        self.config += 1;
        self.revisions.clear();
        self.latest = None;
        self.released_current = false;
    }
    pub fn observe(&mut self, sentence: u64, revision: u64) -> Option<WorkKey> {
        if !self.active {
            return None;
        }
        if self
            .latest
            .is_none_or(|previous| (sentence, revision) > previous)
        {
            if self.released_current
                && let Some((previous, _)) = self.latest
                && previous != sentence
            {
                self.revisions.remove(&previous);
            }
            self.released_current = false;
        }
        let previous = self.revisions.entry(sentence).or_insert(revision);
        *previous = (*previous).max(revision);
        self.latest = Some(
            self.latest
                .unwrap_or((sentence, revision))
                .max((sentence, revision)),
        );
        Some(WorkKey {
            instance_id: self.instance.clone(),
            session_id: self.number,
            sentence_id: sentence,
            revision,
            config_revision: self.config,
        })
    }
    pub fn accepts(&self, key: &WorkKey) -> bool {
        self.active
            && key.instance_id == self.instance
            && key.session_id == self.number
            && key.config_revision == self.config
            && self.revisions.get(&key.sentence_id) == Some(&key.revision)
    }
    pub fn may_display(&self, key: &WorkKey) -> bool {
        self.accepts(key) && self.latest == Some((key.sentence_id, key.revision))
    }
    /// Re-adopt a persisted task without changing the current display sentence.
    /// A delayed restore response from an old session/configuration is rejected.
    pub fn retain(&mut self, key: &WorkKey) -> bool {
        if !self.active
            || key.instance_id != self.instance
            || key.session_id != self.number
            || key.config_revision != self.config
        {
            return false;
        }
        let revision = self
            .revisions
            .entry(key.sentence_id)
            .or_insert(key.revision);
        if *revision > key.revision {
            return false;
        }
        *revision = key.revision;
        if self.latest == Some((key.sentence_id, key.revision)) {
            self.released_current = false;
        }
        true
    }
    /// Release tracking after history/offload acknowledgement. The current
    /// display key remains until a newer sentence arrives. An old revision's
    /// acknowledgement must never remove a newer revision's validity entry.
    pub fn forget(&mut self, key: &WorkKey) {
        if !self.accepts(key) {
            return;
        }
        if self.latest == Some((key.sentence_id, key.revision)) {
            self.released_current = true;
        } else {
            self.revisions.remove(&key.sentence_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delayed_results_cannot_change_newer_revision_session_or_configuration() {
        let mut session = Session::new("worker-instance-A".into());
        session.start();
        let old = session.observe(1, 1).unwrap();
        let new = session.observe(1, 2).unwrap();
        assert!(!session.accepts(&old));
        assert!(session.may_display(&new));
        let next = session.observe(2, 1).unwrap();
        assert!(session.accepts(&new));
        assert!(!session.may_display(&new));
        assert!(session.may_display(&next));
        session.reconfigure();
        assert!(!session.accepts(&next));
        let configured = session.observe(3, 1).unwrap();
        assert!(session.accepts(&configured));
        session.stop();
        assert!(!session.accepts(&configured));
        session.start();
        assert!(!session.accepts(&configured));
        let mut different = Session::new("worker-instance-B".into());
        different.start();
        different.observe(1, 2);
        assert!(!different.accepts(&new));
    }
    #[test]
    fn completed_sentence_tracking_stays_bounded_and_old_ack_does_not_forget_a_revision() {
        let mut session = Session::new("instance".into());
        session.start();
        for sentence in 1..=1000 {
            let key = session.observe(sentence, 1).unwrap();
            session.forget(&key);
            assert_eq!(session.revisions.len(), 1);
        }
        let old = session.observe(1001, 1).unwrap();
        let revised = session.observe(1001, 2).unwrap();
        session.forget(&old);
        assert!(session.accepts(&revised));
        session.forget(&revised);
        session.observe(1002, 1);
        assert!(!session.accepts(&revised));
        assert_eq!(session.revisions.len(), 1);
    }
    #[test]
    fn restored_current_task_remains_valid_when_the_next_sentence_arrives() {
        let mut session = Session::new("instance".into());
        session.start();
        let offloaded = session.observe(40, 1).unwrap();
        session.forget(&offloaded);
        assert!(session.retain(&offloaded));
        session.observe(41, 1);
        assert!(session.accepts(&offloaded));
        assert!(!session.may_display(&offloaded));
        session.forget(&offloaded);
        assert!(!session.accepts(&offloaded));
        session.reconfigure();
        assert!(!session.retain(&offloaded));
    }
}
