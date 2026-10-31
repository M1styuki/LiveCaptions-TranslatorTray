//! Subtitle business state, called only by the Slint-thread controller. No UIA,
//! HTTP or SQLite calls occur here; effects cross the existing bounded owners.
use crate::engine::{Completion, Request};
use captions_core::{
    scheduler::{MAX_COMPLETE_WAITING, Scheduler, Task},
    segment::{self, Segmenter},
    session::{Session, WorkKey},
    settings::Settings,
    text::{self, Context, Contexts},
};
use std::{
    collections::{HashMap, VecDeque},
    time::{SystemTime, UNIX_EPOCH},
};
use storage::{
    actor::{Client, Command, Event},
    history::{Fence, Record, Save},
};
use windows_platform::captions::CaptionSample;

pub enum Action {
    Dispatch(Box<Request>),
    Cancel(WorkKey),
}
#[derive(Clone, Default)]
pub struct View {
    pub capture_revision: u64,
    pub source: String,
    pub overlay_source: String,
    pub translation: String,
    pub overlay_translation: String,
    pub notice: String,
    pub latency_ms: u64,
    pub cards: Vec<Context>,
}
enum Meta {
    Delete,
    Fence(u64),
    Expect(Task),
    Save(Task),
    Spill(Task),
    Take,
    Noop,
}
#[derive(Clone, Copy, PartialEq)]
enum Sent {
    Queued,
    Submitted,
    Failed,
}
struct Pending {
    request: u64,
    command: Command,
    meta: Meta,
    sent: Sent,
}
struct Suppressed {
    id: u64,
    tail: String,
    crossed_boundary: bool,
}
pub struct Flow {
    session: Session,
    segmenter: Segmenter,
    scheduler: Scheduler,
    contexts: Contexts,
    settings: Option<Settings>,
    active: bool,
    log_only: bool,
    epoch: u64,
    next_request: u64,
    fence_ready: bool,
    pending: VecDeque<Pending>,
    actions: VecDeque<Action>,
    tracking: HashMap<u64, WorkKey>,
    observed: Option<WorkKey>,
    sample: Option<(CaptionSample, u64)>,
    processed_changes: u64,
    sample_pending: bool,
    blocked: bool,
    error: Option<String>,
    view: View,
    view_dirty: bool,
    displayed: Option<(WorkKey, String)>,
    deferred: VecDeque<Completion>,
    seen_full: String,
    clearing: bool,
    clear_result: Option<Result<(), String>>,
    suppressed_sentence: Option<Suppressed>,
    post_delete_current: bool,
}
impl Flow {
    #[cfg(test)]
    pub fn test_clearing(&self) -> bool {
        self.clearing
    }
    #[cfg(test)]
    pub fn test_delete_submitted(&self) -> bool {
        self.pending
            .iter()
            .any(|pending| matches!(pending.meta, Meta::Delete) && pending.sent == Sent::Submitted)
    }
    #[cfg(test)]
    pub fn test_context_count(&self) -> usize {
        self.contexts.len()
    }
    #[cfg(test)]
    pub fn test_spill_ids(&self) -> Vec<u64> {
        self.pending
            .iter()
            .filter_map(|pending| {
                if let Meta::Spill(task) = &pending.meta {
                    Some(task.key.sentence_id)
                } else {
                    None
                }
            })
            .collect()
    }
    #[cfg(test)]
    pub fn test_save_request(&self) -> (u64, WorkKey) {
        self.pending
            .iter()
            .find_map(|pending| match &pending.meta {
                Meta::Save(task) => Some((pending.request, task.key.clone())),
                _ => None,
            })
            .unwrap()
    }
    #[cfg(test)]
    pub fn test_fence_ready(&self) -> bool {
        self.fence_ready
    }
    #[cfg(test)]
    pub fn test_waiting(&self) -> usize {
        self.scheduler.waiting()
    }
    #[cfg(test)]
    pub fn test_processing(&self) -> usize {
        self.scheduler.in_flight()
    }
    #[cfg(test)]
    pub fn test_sample_pending(&self) -> bool {
        self.sample_pending
    }
    #[cfg(test)]
    pub fn test_spill_pending(&self) -> bool {
        self.pending
            .iter()
            .any(|pending| matches!(pending.meta, Meta::Spill(_) | Meta::Take))
    }
    #[cfg(test)]
    pub fn test_view(&self) -> &View {
        &self.view
    }
    #[cfg(test)]
    pub fn test_retained_spill(&self) -> Option<&Task> {
        self.pending.iter().find_map(|pending| {
            if let Meta::Spill(task) = &pending.meta {
                Some(task)
            } else {
                None
            }
        })
    }
    pub fn new(instance: String) -> Self {
        Self {
            session: Session::new(instance),
            segmenter: Segmenter::default(),
            scheduler: Scheduler::default(),
            contexts: Contexts::default(),
            settings: None,
            active: false,
            log_only: false,
            epoch: 0,
            next_request: 1 << 63,
            fence_ready: false,
            pending: VecDeque::new(),
            actions: VecDeque::new(),
            tracking: HashMap::new(),
            observed: None,
            sample: None,
            processed_changes: 0,
            sample_pending: false,
            blocked: false,
            error: None,
            view: View::default(),
            view_dirty: false,
            displayed: None,
            deferred: VecDeque::new(),
            seen_full: String::new(),
            clearing: false,
            clear_result: None,
            suppressed_sentence: None,
            post_delete_current: false,
        }
    }
    fn enqueue(&mut self, meta: Meta, build: impl FnOnce(u64) -> Command) {
        self.next_request += 1;
        let request = self.next_request;
        // Two processing slots, one spill, one refill, one current fence and
        // coalesced revision notices keep this well below the channel capacity.
        assert!(self.pending.len() < storage::actor::CAPACITY);
        self.pending.push_back(Pending {
            request,
            command: build(request),
            meta,
            sent: Sent::Queued,
        });
    }
    fn fence(&mut self, enabled: bool) {
        self.fence_ready = false;
        let (instance, session, config) = self.session.identity();
        let value = enabled.then(|| Fence {
            instance_id: instance.into(),
            session_id: session,
            config_revision: config,
        });
        self.enqueue(Meta::Fence(self.epoch), |request| Command::Fence {
            request,
            value,
        });
    }
    pub fn begin(&mut self, settings: Settings, log_only: bool) {
        self.session.start();
        self.active = true;
        self.settings = Some(settings);
        self.log_only = log_only;
        self.reset(false);
        self.fence(true);
    }
    fn reset(&mut self, replay: bool) {
        self.epoch += 1;
        for key in self.scheduler.stop() {
            self.actions.push_back(Action::Cancel(key));
        }
        self.actions
            .retain(|action| matches!(action, Action::Cancel(_)));
        // An already accepted successful result remains owned until its save
        // acknowledgement. Unfinished/waiting translations are cancelled.
        self.pending
            .retain(|pending| matches!(pending.meta, Meta::Save(_) | Meta::Delete));
        self.tracking.clear();
        self.observed = None;
        if self.suppressed_sentence.is_none() && !self.post_delete_current {
            self.segmenter.reconfigure();
            self.seen_full.clear();
        } else if let Some((previous, current)) = self.segmenter.restart_sync()
            && let Some(suppressed) = &mut self.suppressed_sentence
            && suppressed.id == previous
        {
            suppressed.id = current;
        }
        self.contexts.clear();
        self.displayed = None;
        self.deferred.clear();
        self.view.translation.clear();
        self.view.overlay_translation.clear();
        self.view.cards.clear();
        self.view_dirty = true;
        self.blocked = self
            .pending
            .iter()
            .any(|pending| pending.sent == Sent::Failed);
        self.sample_pending = replay && self.sample.is_some();
        self.processed_changes = if replay {
            self.sample
                .as_ref()
                .map_or(0, |(sample, _)| sample.changes.saturating_sub(1))
        } else {
            0
        };
        if !replay {
            self.sample = None;
            self.view = View::default();
        }
        self.view.notice = if self.active && self.log_only {
            "翻译已暂停".into()
        } else {
            String::new()
        };
    }
    pub fn reconfigure(&mut self, settings: Settings, log_only: bool) {
        if !self.active {
            return;
        }
        self.session.reconfigure();
        self.settings = Some(settings);
        self.log_only = log_only;
        self.reset(true);
        self.fence(true);
    }
    /// A confirmed user deletion is serialized after durable accepted saves.
    /// Invalidate callbacks before cancellation, but keep receiving the latest
    /// caption while the database command is pending. Preserve the current
    /// segment identity so its next revision cannot recreate deleted history.
    pub fn clear_history(&mut self) {
        if self.clearing {
            return;
        }
        let latest = self.sample.clone();
        self.session.reconfigure();
        self.suppressed_sentence = None;
        self.post_delete_current = false;
        self.reset(false);
        self.clearing = true;
        self.clear_result = None;
        self.fence_ready = false;
        if let Some((sample, revision)) = latest {
            let full = text::normalize_caption(&sample.text);
            if let Some(settings) = &self.settings {
                self.suppressed_sentence = self
                    .segmenter
                    .observe_sample(
                        &sample.text,
                        settings.json()["MaxSyncInterval"].as_u64().unwrap() as u8,
                        settings.json()["DisplaySentences"].as_u64().unwrap() as usize,
                        0,
                        1,
                        0,
                    )
                    .map(|observation| Suppressed {
                        id: observation.sentence.id,
                        tail: logical_tail(&full).into(),
                        crossed_boundary: false,
                    });
            }
            self.processed_changes = sample.changes;
            self.seen_full = full;
            self.capture(sample, revision);
            self.sample_pending = false;
        }
        self.enqueue(Meta::Delete, |request| Command::Delete { request });
    }
    fn cleared(&mut self, result: Result<(), String>) {
        self.clearing = false;
        self.clear_result = Some(result);
        if self.active
            && !self
                .pending
                .iter()
                .any(|pending| matches!(pending.meta,Meta::Fence(epoch) if epoch==self.epoch))
        {
            self.fence(true);
        }
    }
    pub fn take_clear_result(&mut self) -> Option<Result<(), String>> {
        self.clear_result.take()
    }
    pub fn presentation_settings(&mut self, settings: Settings) {
        if !self.active {
            return;
        }
        self.settings = Some(settings);
        if let Some((sample, revision)) = self.sample.clone() {
            self.capture(sample, revision);
        }
        self.refresh_translation();
    }
    pub fn stop(&mut self) {
        if !self.active {
            return;
        }
        self.session.stop();
        self.active = false;
        self.reset(false);
        self.fence(false);
    }
    pub fn capture(&mut self, sample: CaptionSample, revision: u64) {
        if !self.active || sample.text.trim().is_empty() {
            return;
        }
        if let Some(settings) = &self.settings {
            let display = settings.json()["DisplaySentences"].as_u64().unwrap() as usize;
            if let Some(text) = segment::extract(&sample.text, display, self.contexts.len()) {
                self.view.source = text.display;
                self.view.overlay_source = text.overlay;
            } else {
                self.view.source.clear();
                self.view.overlay_source.clear();
                self.view.translation.clear();
                self.view.overlay_translation.clear();
            }
        }
        self.view.capture_revision = revision;
        self.view_dirty = true;
        self.sample = Some((sample, revision));
        self.sample_pending = true;
    }
    fn capture_ready(&self) -> bool {
        self.active
            && self.fence_ready
            && !self.clearing
            && !self.blocked
            && self.pending.len() < 56
    }
    /// One observed snapshot produces exactly the original latest-caption
    /// extraction. Never infer intermediate sentences from the accumulated
    /// transcript: Windows may revise or roll its already completed prefix.
    pub fn can_capture(&self) -> bool {
        !self.sample_pending && self.capture_ready()
    }
    fn process_sample(&mut self) {
        if !self.sample_pending || !self.capture_ready() {
            return;
        }
        let (sample, _) = self.sample.as_ref().unwrap().clone();
        let changes = sample.changes.saturating_sub(self.processed_changes);
        let full = text::normalize_caption(&sample.text);
        let boundaries = |text: &str| {
            text[..text.len() - logical_tail(text).len()]
                .chars()
                .filter(|mark| text::is_eos(*mark))
                .count()
        };
        // This explicit boundary is used only by the deletion fence. It does
        // not manufacture source strings or enqueue earlier completed text.
        let after_boundary = boundaries(&full) > boundaries(&self.seen_full);
        let force = after_boundary && self.suppressed_sentence.is_some();
        self.process_piece(
            &sample.text,
            changes,
            sample.unchanged,
            force,
            after_boundary,
        );
        self.seen_full = full;
        self.processed_changes = sample.changes;
        self.sample_pending = false;
        self.refresh_translation();
    }
    fn process_piece(
        &mut self,
        raw: &str,
        changes: u64,
        unchanged: u16,
        force: bool,
        mut after_boundary: bool,
    ) {
        let normalized = text::normalize_caption(raw);
        let full = normalized.as_str();
        let settings = self.settings.as_ref().unwrap();
        if force {
            self.segmenter.force_next_boundary();
        }
        let Some(mut observation) = self.segmenter.observe_sample(
            raw,
            settings.json()["MaxSyncInterval"].as_u64().unwrap() as u8,
            settings.json()["DisplaySentences"].as_u64().unwrap() as usize,
            self.contexts.len(),
            changes,
            unchanged,
        ) else {
            self.contexts.clear();
            self.displayed = None;
            return;
        };
        after_boundary |= observation.retained_boundary;
        if after_boundary {
            self.post_delete_current = false;
        }
        if observation.text.clear_context {
            self.contexts.clear();
        }
        if let Some(suppressed) = &mut self.suppressed_sentence {
            let tail = logical_tail(full);
            // A short unfinished tail can initially be merged into the prior
            // completed sentence by the display/extraction contract. Its later
            // size-induced ID change is not a new logical sentence.
            if after_boundary
                || (observation.sentence.id != suppressed.id
                    && !text::is_revision(suppressed.tail.trim(), tail.trim()))
            {
                suppressed.crossed_boundary = true;
            }
            if !suppressed.crossed_boundary {
                suppressed.id = observation.sentence.id;
                suppressed.tail = tail.into();
                return;
            }
            // Until the new short tail is included in translation, extraction
            // may still return only the deleted preceding complete sentence.
            if !full.chars().next_back().is_some_and(text::is_eos) && observation.sentence.complete
                || !after_boundary && tail.trim() == suppressed.tail.trim()
            {
                return;
            }
            if observation.sentence.id == suppressed.id {
                self.segmenter.force_next_boundary();
                observation = self
                    .segmenter
                    .observe_sample(
                        raw,
                        settings.json()["MaxSyncInterval"].as_u64().unwrap() as u8,
                        settings.json()["DisplaySentences"].as_u64().unwrap() as usize,
                        self.contexts.len(),
                        changes,
                        unchanged,
                    )
                    .unwrap();
            }
            // SHORT extraction may include the deleted predecessor in this
            // first new sentence. Keep its display, but never send or save it.
            // Continue trimming revisions/configuration replay until the next
            // physical boundary makes both extracted sentences genuinely new.
            self.post_delete_current = true;
        }
        self.suppressed_sentence = None;
        let key = self
            .session
            .observe(observation.sentence.id, observation.sentence.revision)
            .unwrap();
        if let Some(previous) = self.observed.replace(key.clone()) {
            if previous.sentence_id != key.sentence_id
                && !self.tracking.contains_key(&previous.sentence_id)
            {
                self.session.forget(&previous);
            }
            if previous.sentence_id == key.sentence_id
                && previous.revision != key.revision
                && let Some(running) = self.scheduler.running_key(key.sentence_id).cloned()
            {
                self.actions.push_back(Action::Cancel(running));
            }
        }
        // A newer source invalidates its outstanding responses, not text that
        // was already shown. Keep that presentation until a current reply wins.
        self.refresh_translation();
        if !observation.submit {
            return;
        }
        if self.tracking.contains_key(&key.sentence_id) {
            self.revise(key.clone());
        }
        let task = Task {
            key: key.clone(),
            source: if self.post_delete_current {
                logical_tail(full).into()
            } else {
                observation.sentence.source
            },
            complete: observation.sentence.complete,
            sequence: observation.sentence.id,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .min(i64::MAX as u64) as i64,
        };
        self.tracking.insert(key.sentence_id, key);
        let offer = self.scheduler.offer(task);
        for key in offer.cancel {
            self.actions.push_back(Action::Cancel(key));
        }
        for key in offer.superseded {
            self.release(&key);
        }
        if let Some(task) = offer.spill {
            self.enqueue(Meta::Spill(task.clone()), |request| Command::Pending {
                request,
                task,
            });
        }
    }
    fn revise(&mut self, key: WorkKey) {
        if let Some(pending) = self.pending.iter_mut().find(|pending| matches!(&pending.command, Command::Revise { key: previous, .. } if previous.sentence_id == key.sentence_id)) {
            if pending.sent == Sent::Queued { pending.command = Command::Revise { request: pending.request, key }; }
            return;
        }
        self.enqueue(Meta::Noop, |request| Command::Revise { request, key });
    }
    pub fn drive(&mut self, physical_available: usize) {
        self.process_sample();
        if !self.sample_pending {
            let completed: Vec<_> = self.deferred.drain(..).collect();
            for completion in completed {
                self.completed(completion);
            }
        }
        if !self.active || !self.fence_ready || self.clearing || self.blocked {
            return;
        }
        let reserved = self
            .pending
            .iter()
            .filter(|pending| matches!(pending.meta, Meta::Expect(_)))
            .count()
            + self
                .actions
                .iter()
                .filter(|action| matches!(action, Action::Dispatch(_)))
                .count();
        for _ in 0..physical_available.saturating_sub(reserved) {
            let Some(task) = self.scheduler.next_ready() else {
                break;
            };
            self.enqueue(Meta::Expect(task.clone()), |request| Command::Expect {
                request,
                key: task.key,
            });
        }
        if self.scheduler.has_spill()
            && self.scheduler.waiting() < MAX_COMPLETE_WAITING
            && !self
                .pending
                .iter()
                .any(|pending| matches!(pending.meta, Meta::Take | Meta::Spill(_)))
        {
            let limit = MAX_COMPLETE_WAITING - self.scheduler.waiting();
            self.enqueue(Meta::Take, |request| Command::TakePending {
                request,
                limit,
            });
        }
    }
    pub fn submit(&mut self, client: &Client) {
        let mut waiting_for_save = Vec::new();
        let mut waiting_for_clear = false;
        for pending in &mut self.pending {
            if waiting_for_clear {
                break;
            }
            if let Meta::Save(task) = &pending.meta {
                waiting_for_save.push((task.key.session_id, task.key.sentence_id));
            }
            // Submitted is not a durable acknowledgement. In particular a
            // blocked write may fail after Stop has already been requested.
            if matches!(pending.meta, Meta::Fence(_) | Meta::Delete) && !waiting_for_save.is_empty()
                || matches!(&pending.command, Command::Revise { key, .. } if waiting_for_save.contains(&(key.session_id, key.sentence_id)))
            {
                break;
            }
            if matches!(pending.meta, Meta::Delete) {
                waiting_for_clear = true;
            }
            if pending.sent == Sent::Failed {
                break;
            }
            if pending.sent == Sent::Submitted {
                continue;
            }
            match client.submit(pending.command.clone()) {
                Ok(()) => pending.sent = Sent::Submitted,
                Err(error) if error.queue_full => break,
                Err(error) => {
                    pending.sent = Sent::Failed;
                    self.blocked = true;
                    self.error = Some(error.to_string());
                    break;
                }
            }
        }
    }
    pub fn retry(&mut self) {
        for pending in &mut self.pending {
            if pending.sent == Sent::Failed {
                pending.sent = Sent::Queued;
            }
        }
        self.blocked = false;
    }
    fn release(&mut self, key: &WorkKey) {
        self.scheduler.finished(key);
        self.session.forget(key);
        if self.tracking.get(&key.sentence_id) == Some(key) {
            self.tracking.remove(&key.sentence_id);
        }
    }
    fn finished(&mut self, key: WorkKey) {
        self.release(&key);
        self.enqueue(Meta::Noop, |request| Command::Finished { request, key });
    }
    pub fn completed(&mut self, completion: Completion) {
        if self.active
            && self.sample_pending
            && self
                .observed
                .as_ref()
                .is_some_and(|key| key.sentence_id == completion.task.key.sentence_id)
        {
            assert!(self.deferred.len() < captions_core::scheduler::MAX_IN_FLIGHT);
            self.deferred.push_back(completion);
            return;
        }
        let key = &completion.task.key;
        if !self.session.accepts(key) || completion.result.is_none() {
            self.finished(key.clone());
            return;
        }
        let translated = match completion.result.unwrap() {
            Ok(text) => text,
            Err(error) => {
                if self.session.may_display(key) {
                    self.view.notice = format!("翻译失败：{error}");
                    self.view_dirty = true;
                    // Caption status clears with a successful current reply;
                    // do not leave this in the unrelated settings-save banner.
                }
                self.finished(key.clone());
                return;
            }
        };
        if !self.log_only {
            self.contexts.upsert(
                key.sentence_id,
                key.revision,
                Context {
                    source: completion.task.source.clone(),
                    translation: translated.clone(),
                },
            );
            if self.session.may_display(key) {
                self.displayed = Some((key.clone(), translated.clone()));
                self.view.notice.clear();
                self.view.latency_ms = completion.latency_ms;
            }
            self.refresh_translation();
        }
        let settings = self.settings.as_ref().unwrap();
        let value = Record {
            timestamp: completion.task.created_at,
            source: completion.task.source.clone(),
            translation: translated,
            target_language: if self.log_only {
                "N/A"
            } else {
                settings.target_language()
            }
            .into(),
            api_used: if self.log_only {
                "LogOnly"
            } else {
                settings.api_name()
            }
            .into(),
        };
        let task = completion.task;
        self.enqueue(Meta::Save(task.clone()), |request| Command::Save {
            request,
            key: task.key,
            value,
        });
    }
    fn refresh_translation(&mut self) {
        let before = self
            .observed
            .as_ref()
            .map_or(u64::MAX, |key| key.sentence_id);
        let count = self.settings.as_ref().map_or(0, |settings| {
            settings.json()["DisplaySentences"].as_u64().unwrap() as usize
        });
        self.view.cards = self.contexts.before(before, count).cloned().collect();
        self.view.cards.reverse();
        self.view.translation.clear();
        self.view.overlay_translation.clear();
        // completed() gates new replies with may_display(). This is the last
        // accepted presentation, retained through source revisions and waits;
        // stop/reconfigure/delete still clear it through reset().
        if let Some((key, text)) = &self.displayed {
            let count = self.settings.as_ref().unwrap().json()["DisplaySentences"]
                .as_u64()
                .unwrap() as usize;
            self.view.translation.clone_from(text);
            self.view.overlay_translation = format!(
                "{}{}",
                self.contexts
                    .previous_text_before(key.sentence_id, count, true),
                text
            );
        }
        self.view_dirty = true;
    }
    pub fn data_event(&mut self, event: &Event) -> bool {
        let request = match event {
            Event::FenceSet { request }
            | Event::Expected { request, .. }
            | Event::Saved { request, .. }
            | Event::PendingStored { request, .. }
            | Event::PendingTaken { request, .. }
            | Event::Flushed { request }
            | Event::Deleted { request }
            | Event::Error { request, .. } => *request,
            _ => return false,
        };
        if request < 1 << 63 {
            return false;
        }
        let Some(position) = self
            .pending
            .iter()
            .position(|pending| pending.request == request)
        else {
            return true;
        };
        if let Event::Error { message, .. } = event {
            if matches!(self.pending[position].meta, Meta::Delete) {
                self.pending.remove(position);
                self.cleared(Err(message.clone()));
                return true;
            }
            self.pending[position].sent = Sent::Failed;
            self.blocked = true;
            self.error = Some(format!(
                "数据保存失败，已暂停接收新任务，现有内容保留：{message}"
            ));
            return true;
        }
        let pending = self.pending.remove(position).unwrap();
        match (pending.meta, event) {
            (Meta::Delete, Event::Deleted { .. }) => self.cleared(Ok(())),
            (Meta::Fence(epoch), Event::FenceSet { .. }) if epoch == self.epoch => {
                self.fence_ready = true
            }
            (Meta::Expect(task), Event::Expected { accepted, .. }) => {
                if !self.session.accepts(&task.key) {
                    self.finished(task.key);
                } else if !accepted {
                    self.blocked = true;
                    self.error = Some("数据会话尚未就绪，任务仍保留，请重试保存。".into());
                    self.pending.insert(
                        position,
                        Pending {
                            request,
                            command: Command::Expect {
                                request,
                                key: task.key.clone(),
                            },
                            meta: Meta::Expect(task),
                            sent: Sent::Failed,
                        },
                    );
                } else if self.log_only {
                    self.completed(Completion {
                        task,
                        result: Some(Ok("N/A".into())),
                        latency_ms: 0,
                    });
                } else {
                    let settings = self.settings.as_ref().unwrap().clone();
                    let aware = settings.json()["ContextAware"].as_bool().unwrap();
                    let count = settings.json()["NumContexts"].as_u64().unwrap() as usize;
                    let llm = providers::is_llm(settings.api_name());
                    let input = if llm {
                        task.source.clone()
                    } else {
                        self.contexts.non_llm_input_before(
                            task.key.sentence_id,
                            &task.source,
                            count,
                            aware,
                        )
                    };
                    let contexts = if llm && aware {
                        self.contexts
                            .before(task.key.sentence_id, count)
                            .cloned()
                            .collect()
                    } else {
                        Vec::new()
                    };
                    self.actions.push_back(Action::Dispatch(Box::new(Request {
                        task,
                        settings,
                        input,
                        context_aware: aware,
                        contexts,
                    })));
                }
            }
            (
                Meta::Save(task),
                Event::Saved {
                    result: Save::Written(_) | Save::AlreadyPresent(_),
                    ..
                },
            ) => self.finished(task.key),
            (
                Meta::Save(task),
                Event::Saved {
                    result: Save::Ignored,
                    ..
                },
            ) => {
                self.blocked = true;
                self.error = Some("保存被会话状态拒绝，成功译文仍保留，请重试保存。".into());
                self.pending.insert(
                    position,
                    Pending {
                        request,
                        command: pending.command,
                        meta: Meta::Save(task),
                        sent: Sent::Failed,
                    },
                );
            }
            (Meta::Spill(task), Event::PendingStored { accepted, .. }) => {
                if *accepted {
                    self.session.forget(&task.key);
                    if self.tracking.get(&task.key.sentence_id) == Some(&task.key) {
                        self.tracking.remove(&task.key.sentence_id);
                    }
                } else if self.session.accepts(&task.key) {
                    self.blocked = true;
                    self.error = Some("待翻译内容未能登记，任务仍保留，请重试保存。".into());
                    self.pending.insert(
                        position,
                        Pending {
                            request,
                            command: Command::Pending {
                                request,
                                task: task.clone(),
                            },
                            meta: Meta::Spill(task),
                            sent: Sent::Failed,
                        },
                    );
                }
            }
            (
                Meta::Take,
                Event::PendingTaken {
                    tasks, remaining, ..
                },
            ) => {
                for task in tasks {
                    if self.session.retain(&task.key) {
                        self.tracking.insert(task.key.sentence_id, task.key.clone());
                        // Capture is held while Take is outstanding, reserving
                        // exactly these free waiting slots for the DB response.
                        assert!(self.scheduler.restore(task.clone()).is_ok());
                    }
                }
                if *remaining == 0
                    && !self
                        .pending
                        .iter()
                        .any(|pending| matches!(pending.meta, Meta::Spill(_)))
                {
                    self.scheduler.storage_drained();
                }
            }
            _ => {}
        }
        true
    }
    pub fn actions(&mut self) -> Vec<Action> {
        self.actions.drain(..).collect()
    }
    pub fn view(&mut self) -> Option<View> {
        if self.view_dirty {
            self.view_dirty = false;
            Some(self.view.clone())
        } else {
            None
        }
    }
    pub fn error(&mut self) -> Option<String> {
        self.error.take()
    }
    pub fn pending_saves(&self) -> usize {
        self.pending.len()
    }
    pub fn is_blocked(&self) -> bool {
        self.blocked
    }
}

// Recover complete sentences skipped when the latest-slot capture coalesces
// cumulative UIA snapshots. A matching current sentence also anchors a rolling
// buffer; it is explicit proof of a new boundary even if EOS counts decreased.
fn logical_tail(full: &str) -> &str {
    let mut marks = full
        .char_indices()
        .rev()
        .filter(|(_, mark)| text::is_eos(*mark));
    if full.chars().next_back().is_some_and(text::is_eos) {
        marks.next();
    }
    let start = marks
        .next()
        .map_or(0, |(position, mark)| position + mark.len_utf8());
    &full[start..]
}
pub fn translation_changed(previous: &Settings, next: &Settings) -> bool {
    [
        "ApiName",
        "TargetLanguage",
        "Prompt",
        "NumContexts",
        "ContextAware",
        "MaxSyncInterval",
    ]
    .iter()
    .any(|field| previous.json()[field] != next.json()[field])
        || previous.active_config() != next.active_config()
        || previous.json()["ConfigIndices"][previous.api_name()]
            != next.json()["ConfigIndices"][next.api_name()]
}
