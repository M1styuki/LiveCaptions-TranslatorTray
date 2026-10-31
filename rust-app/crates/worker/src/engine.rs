//! Capture/network transport only. The UI controller owns sentence identity,
//! scheduling, contexts, history commands and final acceptance of every result.
use crate::MainWindow;
use captions_core::{
    scheduler::{MAX_IN_FLIGHT, Task},
    session::WorkKey,
    settings::Settings,
    text,
};
use providers::Translator;
use std::{
    collections::{HashMap, VecDeque},
    io,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{runtime::Runtime, task::AbortHandle};
use windows_platform::captions::{CaptionEvent, CaptionSample, CaptionSession};

pub enum Status {
    Ready,
    Fault(String),
}
pub struct Completion {
    pub task: Task,
    pub result: Option<Result<String, String>>,
    pub latency_ms: u64,
}
pub struct Request {
    pub task: Task,
    pub settings: Settings,
    pub input: String,
    pub context_aware: bool,
    pub contexts: Vec<text::Context>,
}
#[derive(Default)]
pub struct Updates {
    pub completions: VecDeque<Completion>,
    pub status: Option<(u64, Status)>,
}
const MAX_CAPTURE_WAITING: usize = 64;
#[derive(Default)]
struct Sources {
    session: Option<u64>,
    epoch: u64,
    latest: Option<(u64, u64, CaptionSample)>,
    queue: VecDeque<(u64, u64, CaptionSample)>,
}
pub struct Mailbox {
    updates: Mutex<Updates>,
    sources: Mutex<Sources>,
    source_space: Condvar,
    wake_pending: AtomicBool,
    ui: slint::Weak<MainWindow>,
}
impl Mailbox {
    #[cfg(test)]
    pub fn test_begin_capture(&self, session: u64) {
        self.begin_capture(session);
    }
    #[cfg(test)]
    pub fn test_source_count(&self) -> usize {
        self.sources.lock().unwrap().queue.len()
    }
    #[cfg(test)]
    pub fn test_fault(&self, session: u64, message: String) {
        self.post(|updates| updates.status = Some((session, Status::Fault(message))));
    }
    pub fn new(ui: slint::Weak<MainWindow>) -> Arc<Self> {
        Arc::new(Self {
            updates: Mutex::default(),
            sources: Mutex::default(),
            source_space: Condvar::new(),
            wake_pending: AtomicBool::new(false),
            ui,
        })
    }
    fn post(&self, update: impl FnOnce(&mut Updates)) {
        update(&mut self.updates.lock().unwrap());
        self.wake();
    }
    fn wake(&self) {
        if !self.wake_pending.swap(true, Ordering::AcqRel) {
            let ui = self.ui.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui.upgrade() {
                    ui.invoke_host_wakeup();
                }
            });
        }
    }
    pub fn take(&self) -> Updates {
        let mut updates = self.updates.lock().unwrap();
        self.wake_pending.store(false, Ordering::Release);
        std::mem::take(&mut updates)
    }
    fn begin_capture(&self, session: u64) {
        let mut sources = self.sources.lock().unwrap();
        sources.session = Some(session);
        sources.epoch += 1;
        sources.latest = None;
        sources.queue.clear();
        self.source_space.notify_all();
    }
    fn stop_capture(&self, session: u64) {
        let mut sources = self.sources.lock().unwrap();
        if sources.session == Some(session) {
            sources.session = None;
            sources.latest = None;
            sources.queue.clear();
            self.source_space.notify_all();
        }
    }
    pub fn take_source(&self) -> Option<(u64, u64, CaptionSample)> {
        let source = self.sources.lock().unwrap().queue.pop_front();
        self.source_space.notify_one();
        source
    }
    pub fn wake_source(&self) {
        if !self.sources.lock().unwrap().queue.is_empty() {
            self.wake();
        }
    }
    /// Configuration/pause switches start from the latest observed caption.
    /// Discard pre-switch transport backlog rather than translating it using
    /// the new settings. This does not affect already accepted save commands.
    pub fn latest_source(&self) -> Option<(u64, u64, CaptionSample)> {
        let mut sources = self.sources.lock().unwrap();
        let latest = sources.latest.clone();
        sources.epoch += 1;
        sources.queue.clear();
        self.source_space.notify_all();
        latest
    }
    pub(crate) fn capture(&self, session: u64, revision: &AtomicU64, sample: CaptionSample) {
        // Silence must not advance the controller's presentation revision or
        // overwrite its latest real source slot while a translation completes.
        if sample.text.trim().is_empty() {
            return;
        }
        let mut sources = self.sources.lock().unwrap();
        if sources.session != Some(session) {
            return;
        }
        let epoch = sources.epoch;
        let current = revision.fetch_add(1, Ordering::AcqRel) + 1;
        sources.latest = Some((session, current, sample.clone()));
        while sources.session == Some(session)
            && sources.epoch == epoch
            && sources.queue.len() == MAX_CAPTURE_WAITING
        {
            sources = self.source_space.wait(sources).unwrap();
        }
        if sources.session != Some(session) || sources.epoch != epoch {
            return;
        }
        sources.queue.push_back((session, current, sample));
        drop(sources);
        self.wake();
    }
}
pub struct Engine {
    session: u64,
    captions: Option<CaptionSession>,
    runtime: Option<Runtime>,
    provider: Arc<Translator>,
    mailbox: Arc<Mailbox>,
    jobs: HashMap<WorkKey, AbortHandle>,
    pub revision: Arc<AtomicU64>,
}
impl Engine {
    #[cfg(test)]
    pub fn local_network_fixture(
        session: u64,
        provider: Translator,
        mailbox: Arc<Mailbox>,
    ) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        mailbox.begin_capture(session);
        Self {
            session,
            captions: None,
            runtime: Some(runtime),
            provider: Arc::new(provider),
            mailbox,
            jobs: HashMap::new(),
            revision: Arc::new(AtomicU64::new(0)),
        }
    }
    pub fn start(session: u64, hidden: bool, mailbox: Arc<Mailbox>) -> io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("translation")
            .enable_all()
            .build()?;
        let provider = Arc::new(Translator::new().map_err(io::Error::other)?);
        let revision = Arc::new(AtomicU64::new(0));
        let capture_revision = revision.clone();
        let capture_mailbox = mailbox.clone();
        let callback = Arc::new(move |event| match event {
            CaptionEvent::Ready => {
                capture_mailbox.post(|updates| updates.status = Some((session, Status::Ready)))
            }
            CaptionEvent::Text(sample) => {
                capture_mailbox.capture(session, &capture_revision, sample);
            }
            CaptionEvent::Fault(message) => capture_mailbox
                .post(|updates| updates.status = Some((session, Status::Fault(message)))),
        });
        mailbox.begin_capture(session);
        let captions = match CaptionSession::start(hidden, callback) {
            Ok(captions) => captions,
            Err(error) => {
                mailbox.stop_capture(session);
                return Err(error);
            }
        };
        Ok(Self {
            session,
            captions: Some(captions),
            runtime: Some(runtime),
            provider,
            mailbox,
            jobs: HashMap::new(),
            revision,
        })
    }
    pub fn available(&self) -> usize {
        MAX_IN_FLIGHT.saturating_sub(self.jobs.len())
    }
    pub fn set_hidden(&self, hidden: bool) {
        if let Some(captions) = &self.captions {
            captions.set_hidden(hidden);
        }
    }
    pub fn invalidate_display(&self) -> u64 {
        self.revision.fetch_add(1, Ordering::AcqRel) + 1
    }
    pub fn dispatch(&mut self, request: Request) -> io::Result<()> {
        if self.available() == 0 {
            return Err(io::Error::other("翻译请求仍在结束，请稍候。"));
        }
        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| io::Error::other("翻译引擎已停止。"))?;
        let provider = self.provider.clone();
        let key = request.task.key.clone();
        let task = request.task.clone();
        let started = Instant::now();
        let job = runtime.spawn(async move { translate(&provider, &request).await });
        self.jobs.insert(key, job.abort_handle());
        let mailbox = self.mailbox.clone();
        runtime.spawn(async move {
            let result = match job.await {
                Ok(result) => Some(result),
                Err(error) if error.is_cancelled() => None,
                Err(_) => Some(Err("翻译任务异常结束，请重新开启。".into())),
            };
            mailbox.post(|updates| {
                // A slot remains occupied until the controller consumes this
                // completion, including cancellation. Thus at most two exist.
                assert!(updates.completions.len() < MAX_IN_FLIGHT);
                updates.completions.push_back(Completion {
                    task,
                    result,
                    latency_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
                });
            });
        });
        Ok(())
    }
    pub fn cancel(&self, key: &WorkKey) {
        if let Some(job) = self.jobs.get(key) {
            job.abort();
        }
    }
    pub fn cancel_all(&self) {
        for job in self.jobs.values() {
            job.abort();
        }
    }
    pub fn finished(&mut self, key: &WorkKey) {
        self.jobs.remove(key);
    }
    pub fn stop(&mut self) {
        self.invalidate_display();
        // A full capture inbox can suspend the MTA producer. Wake it before
        // joining that thread; Stop deliberately invalidates queued sources.
        self.mailbox.stop_capture(self.session);
        self.cancel_all();
        if let Some(mut captions) = self.captions.take() {
            captions.stop();
        }
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_timeout(Duration::from_millis(200));
        }
        self.jobs.clear();
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn translate(provider: &Translator, request: &Request) -> Result<String, String> {
    let raw = provider
        .translate(&request.settings, &request.input, &request.contexts)
        .await
        .map_err(|error| error.to_string())?;
    let current = if request.context_aware && !providers::is_llm(request.settings.api_name()) {
        if let Some(marked) = text::extract_marked(&raw) {
            marked.to_owned()
        } else {
            // One explicit fallback only, still inside the same cancellable job.
            provider
                .translate(&request.settings, &request.task.source, &[])
                .await
                .map_err(|error| error.to_string())?
        }
    } else {
        raw
    };
    text::clean_translation(&current).ok_or_else(|| "翻译服务返回了空译文。".into())
}

#[cfg(test)]
mod capture_tests {
    use super::*;
    use std::sync::mpsc;
    fn filled() -> (Arc<Mailbox>, Arc<AtomicU64>) {
        let mailbox = Mailbox::new(slint::Weak::default());
        mailbox.begin_capture(1);
        let revision = Arc::new(AtomicU64::new(0));
        for index in 1..=MAX_CAPTURE_WAITING {
            mailbox.capture(
                1,
                &revision,
                CaptionSample {
                    text: format!("Public observed caption {index}."),
                    changes: index as u64,
                    unchanged: 0,
                },
            );
        }
        (mailbox, revision)
    }
    #[test]
    fn full_capture_fifo_preserves_order_and_stop_wakes_its_blocked_producer() {
        let (mailbox, _revision) = filled();
        for index in 1..=MAX_CAPTURE_WAITING {
            let (_, observed_revision, sample) = mailbox.take_source().unwrap();
            assert_eq!(observed_revision, index as u64);
            assert_eq!(sample.changes, index as u64);
        }
        let (mailbox, revision) = filled();
        let producer = mailbox.clone();
        let (sender, received) = mpsc::channel();
        let blocked = std::thread::spawn(move || {
            producer.capture(
                1,
                &revision,
                CaptionSample {
                    text: "The pending public caption.".into(),
                    changes: 65,
                    unchanged: 0,
                },
            );
            sender.send(()).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while mailbox
            .sources
            .lock()
            .unwrap()
            .latest
            .as_ref()
            .unwrap()
            .2
            .changes
            < 65
        {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        mailbox.stop_capture(1);
        received
            .recv_timeout(Duration::from_secs(2))
            .expect("Stop must unblock capture before MTA join");
        blocked.join().unwrap();
        assert!(mailbox.take_source().is_none());
    }
    #[test]
    fn action_cutoff_uses_latest_observation_and_cancels_older_blocked_enqueue() {
        let (mailbox, revision) = filled();
        let producer = mailbox.clone();
        let producer_revision = revision.clone();
        let blocked = std::thread::spawn(move || {
            producer.capture(
                1,
                &producer_revision,
                CaptionSample {
                    text: "The actual current public caption.".into(),
                    changes: 65,
                    unchanged: 0,
                },
            );
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while revision.load(Ordering::Acquire) < 65 {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        let (_, _, latest) = mailbox.latest_source().unwrap();
        assert_eq!(latest.changes, 65);
        blocked.join().unwrap();
        assert!(
            mailbox.take_source().is_none(),
            "pre-action producer must not recreate backlog"
        );
        mailbox.capture(
            1,
            &revision,
            CaptionSample {
                text: "A new post-action public caption.".into(),
                changes: 66,
                unchanged: 0,
            },
        );
        assert_eq!(mailbox.take_source().unwrap().2.changes, 66);
    }
}
