//! Business integration with the real storage thread/SQLite database.
//! Controlled HTTP results below do not count as real ASR/service verification.
use crate::{
    engine::{Completion, Mailbox, Request},
    flow::{Action, Flow},
};
use captions_core::{
    session::WorkKey,
    settings::{Screen, Settings},
};
use std::{
    collections::VecDeque,
    fs,
    path::PathBuf,
    sync::{Arc, atomic::AtomicU64, mpsc},
    time::{Duration, Instant},
};
use storage::actor::{Command, Event, Storage};
use windows_platform::captions::CaptionSample;

struct Fixture {
    flow: Flow,
    mailbox: Arc<Mailbox>,
    capture_revision: Arc<AtomicU64>,
    owner: Option<Storage>,
    client: storage::actor::Client,
    events: mpsc::Receiver<Event>,
    network: Vec<Request>,
    cancelled: Vec<WorkKey>,
    user_events: VecDeque<Event>,
    root: PathBuf,
    parent: PathBuf,
    allow_blocked: bool,
}
mod deletion;
#[test]
fn controller_normalizes_once_like_the_fixed_original_for_acronyms() {
    for (raw, expected, idle) in [
        ("U. S. A.", "US. A.", 0),
        ("U.S.A. words", " A.", 0),
        ("A . B . C is here", " C is here", 50),
    ] {
        let mut fixture = Fixture::new(false);
        fixture.capture(raw.into(), 1, 0);
        if idle != 0 {
            fixture.capture(raw.into(), 1, idle);
        }
        fixture.until(|fixture| fixture.network.len() == 1);
        assert_eq!(fixture.network[0].task.source, expected);
        fixture.finish(0, Some(Ok("公开缩写测试译文".into())));
        fixture.until(|fixture| fixture.flow.pending_saves() == 0);
        assert_eq!(fixture.page().rows[0].source, expected);
        fixture.close();
    }
}
#[test]
fn public_uia_number_reformat_does_not_replay_the_unchanged_prefix() {
    let input: serde_json::Value = serde_json::from_str(include_str!(
        "flow_tests/fixtures/public-retained-prefix.json"
    ))
    .unwrap();
    let mut fixture = Fixture::new(false);
    fixture.capture(input["previous"].as_str().unwrap().into(), 1, 0);
    assert!(
        fixture.network.is_empty(),
        "the first partial sentence has not reached the sync gate"
    );
    fixture.capture(input["current"].as_str().unwrap().into(), 2, 0);
    fixture.until(|fixture| {
        !fixture.network.is_empty() && fixture.network.len() == fixture.flow.test_processing()
    });
    assert_eq!(
        fixture.network.len(),
        1,
        "reformatted earlier numbers must not enqueue historical sentences"
    );
    assert_eq!(
        fixture.network[0].task.source.trim(),
        input["expected_source"].as_str().unwrap()
    );
    fixture.finish(0, Some(Ok("公共当前句译文。".into())));
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    assert_eq!(fixture.page().total, 1);
    fixture.close();
}
#[test]
fn translation_failure_is_presentation_status_only_and_pause_resets_it() {
    let mut fixture = Fixture::new(false);
    let first = "The first public sentence is complete.";
    fixture.capture(first.into(), 1, 0);
    fixture.until(|f| f.network.len() == 1);
    fixture.finish(0, Some(Ok("保留的成功译文".into())));
    fixture.until(|f| f.flow.pending_saves() == 0);
    let second = format!("{first} The second public sentence is complete.");
    fixture.capture(second.clone(), 2, 0);
    fixture.until(|f| f.network.len() == 1);
    let third = format!("{second} The third public sentence is complete.");
    fixture.capture(third.clone(), 3, 0);
    fixture.until(|f| f.network.len() == 2);
    fixture.finish(0, Some(Err("迟到的旧句失败".into())));
    assert!(fixture.flow.test_view().notice.is_empty());
    fixture.finish(0, Some(Err("服务不可用".into())));
    assert_eq!(fixture.flow.test_view().notice, "翻译失败：服务不可用");
    assert!(fixture.flow.error().is_none());
    assert_eq!(fixture.flow.test_view().translation, "保留的成功译文");
    assert_eq!(fixture.flow.test_context_count(), 1);
    assert_eq!(
        fixture.page().total,
        1,
        "errors must not become successful history"
    );
    fixture.capture(
        format!("{third} A fourth public sentence is complete."),
        4,
        0,
    );
    fixture.until(|f| f.network.len() == 1);
    fixture.finish(0, Some(Ok("恢复后的成功译文".into())));
    fixture.until(|f| f.flow.pending_saves() == 0);
    assert!(fixture.flow.test_view().notice.is_empty());
    assert_eq!(fixture.flow.test_view().translation, "恢复后的成功译文");
    fixture.flow.reconfigure(
        Settings::defaults(Screen::new(1920.0, 1080.0).unwrap()),
        true,
    );
    assert_eq!(fixture.flow.test_view().notice, "翻译已暂停");
    fixture.flow.stop();
    assert!(fixture.flow.test_view().notice.is_empty());
    fixture.close();
}
#[test]
fn visible_translation_stays_until_a_current_result_replaces_it() {
    let mut fixture = Fixture::new(false);
    let first = "This is the first complete public test sentence.";
    fixture.capture(first.into(), 1, 0);
    fixture.until(|fixture| fixture.network.len() == 1);
    fixture.finish(0, Some(Ok("第一句已显示的译文。".into())));
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    let shown = fixture.flow.test_view().overlay_translation.clone();
    assert!(!shown.is_empty());
    fixture.capture(
        format!("{first} This second public sentence is complete."),
        2,
        0,
    );
    fixture.until(|fixture| fixture.network.len() == 1);
    assert_eq!(
        fixture.flow.test_view().overlay_translation,
        shown,
        "a new source must not erase an already presented translation while HTTP is pending"
    );
    fixture.capture(
        format!(
            "{first} This second public sentence is complete. A third public sentence is complete."
        ),
        3,
        0,
    );
    fixture.until(|fixture| fixture.network.len() == 2);
    assert_eq!(fixture.flow.test_view().overlay_translation, shown);
    let source = fixture.flow.test_view().source.clone();
    fixture.capture(String::new(), 4, 0);
    assert_eq!(
        fixture.flow.test_view().source,
        source,
        "empty UIA samples are silence, as in the original loop"
    );
    assert_eq!(fixture.flow.test_view().overlay_translation, shown);
    fixture.finish(0, Some(Ok("迟到的第二句。".into())));
    assert_eq!(
        fixture.flow.test_view().overlay_translation,
        shown,
        "retaining a displayed result must not let an older response replace it"
    );
    fixture.finish(0, Some(Ok("第三句的新译文。".into())));
    assert_eq!(fixture.flow.test_view().translation, "第三句的新译文。");
    assert!(
        fixture
            .flow
            .test_view()
            .overlay_translation
            .ends_with("第三句的新译文。")
    );
    fixture.flow.stop();
    assert!(fixture.flow.test_view().overlay_translation.is_empty());
    fixture.close();
}
#[test]
fn inactive_service_edits_preserve_the_active_translation_configuration() {
    let screen = Screen::new(1920.0, 1080.0).unwrap();
    let google = Settings::defaults(screen);
    let other = google
        .edited(screen, |value| {
            value["Configs"]["OpenAI"][0]["ApiKey"] = "fixture-other".into()
        })
        .unwrap()
        .0;
    assert!(!crate::flow::translation_changed(&google, &other));
    let switched = other
        .edited(screen, |value| value["ApiName"] = "OpenAI".into())
        .unwrap()
        .0;
    assert!(crate::flow::translation_changed(&other, &switched));
    let changed = switched
        .edited(screen, |value| {
            value["Configs"]["OpenAI"][0]["ApiKey"] = "fixture-new".into()
        })
        .unwrap()
        .0;
    assert!(crate::flow::translation_changed(&switched, &changed));
}
#[test]
fn display_only_edits_keep_context_and_current_translation_without_another_http_request() {
    let mut fixture = Fixture::new(false);
    for sentence in 1..=3 {
        fixture.capture(
            (1..=sentence)
                .map(|index| format!("This is public complete sentence number {index}."))
                .collect::<Vec<_>>()
                .join(" "),
            sentence,
            0,
        );
        fixture.until(|fixture| fixture.network.len() == 1);
        fixture.finish(0, Some(Ok(format!("译文{sentence}"))));
        fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    }
    let screen = Screen::new(1920.0, 1080.0).unwrap();
    let original = Settings::defaults(screen);
    let updated = original
        .edited(screen, |value| {
            value["DisplaySentences"] = 2.into();
            value["MainWindow"]["LatencyShow"] = true.into();
        })
        .unwrap()
        .0;
    assert!(!crate::flow::translation_changed(&original, &updated));
    fixture.flow.presentation_settings(updated);
    fixture.until(|fixture| !fixture.flow.test_sample_pending());
    assert!(fixture.network.is_empty());
    assert_eq!(fixture.flow.test_view().translation, "译文3");
    assert_eq!(
        fixture
            .flow
            .test_view()
            .cards
            .iter()
            .map(|card| card.translation.as_str())
            .collect::<Vec<_>>(),
        vec!["译文2", "译文1"]
    );
    assert_eq!(fixture.page().total, 3);
    fixture.flow.stop();
    assert!(fixture.flow.test_view().cards.is_empty());
    fixture.close();
}
impl Fixture {
    fn new(log_only: bool) -> Self {
        let parent =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../scratch/flow-fixtures");
        fs::create_dir_all(&parent).unwrap();
        let parent = parent.canonicalize().unwrap();
        let root = parent.join(windows_platform::desktop::instance_id().unwrap());
        fs::create_dir(&root).unwrap();
        let legacy = root.join("legacy");
        fs::create_dir(&legacy).unwrap();
        let screen = Screen::new(1920.0, 1080.0).unwrap();
        let (sender, events) = mpsc::sync_channel(64);
        let owner = Storage::start(
            root.join("data"),
            legacy,
            screen,
            "flow-instance".into(),
            Arc::new(move |event| {
                sender.send(event).unwrap();
            }),
        )
        .unwrap();
        let client = owner.client();
        assert!(matches!(
            events.recv_timeout(Duration::from_secs(3)).unwrap(),
            Event::FirstUse
        ));
        client.submit(Command::UseDefaults { request: 1 }).unwrap();
        assert!(matches!(
            events.recv_timeout(Duration::from_secs(3)).unwrap(),
            Event::Ready { .. }
        ));
        let mut flow = Flow::new("flow-instance".into());
        flow.begin(Settings::defaults(screen), log_only);
        let mailbox = Mailbox::new(slint::Weak::default());
        mailbox.test_begin_capture(1);
        let mut fixture = Self {
            flow,
            mailbox,
            capture_revision: Arc::new(AtomicU64::new(0)),
            owner: Some(owner),
            client,
            events,
            network: Vec::new(),
            cancelled: Vec::new(),
            user_events: VecDeque::new(),
            root,
            parent,
            allow_blocked: false,
        };
        fixture.until(|fixture| fixture.flow.test_fence_ready());
        fixture
    }
    fn pump(&mut self) {
        if self.flow.can_capture()
            && let Some((_, revision, sample)) = self.mailbox.take_source()
        {
            self.flow.capture(sample, revision);
        }
        self.flow.drive(2usize.saturating_sub(self.network.len()));
        self.flow.submit(&self.client);
        for action in self.flow.actions() {
            match action {
                Action::Cancel(key) => self.cancelled.push(key),
                Action::Dispatch(request) => {
                    assert!(self.network.len() < 2);
                    self.network.push(*request);
                }
            }
        }
        while let Ok(event) = self.events.try_recv() {
            if !self.flow.data_event(&event) {
                self.user_events.push_back(event);
            }
        }
        assert!(self.flow.test_waiting() <= 32);
        assert!(self.flow.test_processing() <= 2);
        if !self.allow_blocked {
            assert!(!self.flow.is_blocked(), "{:?}", self.flow.error());
        }
    }
    fn until(&mut self, condition: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.pump();
            if condition(self) {
                return;
            }
            assert!(Instant::now() < deadline, "flow transition timed out");
            if let Ok(event) = self.events.recv_timeout(Duration::from_millis(2))
                && !self.flow.data_event(&event)
            {
                self.user_events.push_back(event);
            }
        }
    }
    fn capture(&mut self, text: String, changes: u64, unchanged: u16) {
        self.queue_capture(text, changes, unchanged);
        self.until(|fixture| {
            fixture.mailbox.test_source_count() == 0
                && !fixture.flow.test_sample_pending()
                && !fixture.flow.test_spill_pending()
        });
    }
    fn queue_capture(&self, text: String, changes: u64, unchanged: u16) {
        self.mailbox.capture(
            1,
            &self.capture_revision,
            CaptionSample {
                text,
                changes,
                unchanged,
            },
        );
    }
    fn finish(&mut self, position: usize, result: Option<Result<String, String>>) {
        let request = self.network.remove(position);
        self.flow.completed(Completion {
            task: request.task,
            result,
            latency_ms: 12,
        });
        self.pump();
    }
    fn page(&mut self) -> storage::history::Page {
        self.client
            .submit(Command::Page {
                request: 10,
                query: String::new(),
                page: 1,
                size: 100,
            })
            .unwrap();
        self.until(|fixture| {
            fixture
                .user_events
                .iter()
                .any(|event| matches!(event, Event::Page { request: 10, .. }))
        });
        let position = self
            .user_events
            .iter()
            .position(|event| matches!(event, Event::Page { request: 10, .. }))
            .unwrap();
        let Event::Page { value, .. } = self.user_events.remove(position).unwrap() else {
            unreachable!()
        };
        value
    }
    fn disk_count(&mut self) -> u64 {
        self.client
            .submit(Command::TakePending {
                request: 11,
                limit: 0,
            })
            .unwrap();
        self.until(|fixture| {
            fixture
                .user_events
                .iter()
                .any(|event| matches!(event, Event::PendingTaken { request: 11, .. }))
        });
        let position = self
            .user_events
            .iter()
            .position(|event| matches!(event, Event::PendingTaken { request: 11, .. }))
            .unwrap();
        let Event::PendingTaken { remaining, .. } = self.user_events.remove(position).unwrap()
        else {
            unreachable!()
        };
        remaining
    }
    fn forty(&mut self) {
        for sentence in 1..=40 {
            let text = (1..=sentence)
                .map(|index| format!("This is complete sentence number {index}."))
                .collect::<Vec<_>>()
                .join(" ");
            self.capture(text, sentence, 0);
            self.until(|fixture| fixture.network.len() == 2 || sentence < 2);
        }
    }
    fn close(mut self) {
        self.flow.stop();
        self.until(|fixture| fixture.flow.pending_saves() == 0);
        self.client
            .submit(Command::Shutdown { request: 12 })
            .unwrap();
        self.until(|fixture| {
            fixture
                .user_events
                .iter()
                .any(|event| matches!(event, Event::Stopped { .. }))
        });
        self.owner.take().unwrap().join().unwrap();
        let actual = self.root.canonicalize().unwrap();
        assert_eq!(actual.parent(), Some(self.parent.as_path()));
        fs::remove_dir_all(actual).unwrap();
    }
}

#[test]
fn forty_complete_captions_use_real_disk_overflow_and_late_first_reply_does_not_replace_the_last() {
    let mut fixture = Fixture::new(false);
    fixture.forty();
    assert_eq!(fixture.flow.test_processing(), 2);
    assert_eq!(fixture.flow.test_waiting(), 32);
    assert_eq!(fixture.disk_count(), 6);
    for sentence in 2..=40 {
        fixture.until(|fixture| {
            fixture
                .network
                .iter()
                .any(|request| request.task.key.sentence_id == sentence)
        });
        let position = fixture
            .network
            .iter()
            .position(|request| request.task.key.sentence_id == sentence)
            .unwrap();
        fixture.finish(position, Some(Ok(format!("译文{sentence}"))));
    }
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    assert_eq!(fixture.flow.test_view().translation, "译文40");
    fixture.finish(0, Some(Ok("迟到的第一句".into())));
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    assert_eq!(fixture.flow.test_view().translation, "译文40");
    let page = fixture.page();
    assert_eq!(page.total, 40);
    for sentence in 1..=40 {
        assert_eq!(
            page.rows
                .iter()
                .filter(|row| row.source.trim()
                    == format!("This is complete sentence number {sentence}."))
                .count(),
            1
        );
    }
    assert_eq!(fixture.disk_count(), 0);
    fixture.close();
}
#[test]
fn revised_cancelled_request_never_writes_history_or_overwrites_current_translation() {
    let mut fixture = Fixture::new(false);
    fixture.capture("The original sentence is incomplete".into(), 1, 50);
    fixture.until(|fixture| fixture.network.len() == 1);
    let old_key = fixture.network[0].task.key.clone();
    fixture.capture("The original sentence is now revised.".into(), 2, 0);
    assert!(fixture.cancelled.contains(&old_key));
    assert_eq!(
        fixture.network.len(),
        1,
        "cancelled physical slot is not prematurely reused"
    );
    fixture.finish(0, Some(Ok("不应写入的旧译文".into())));
    fixture.until(|fixture| fixture.network.len() == 1);
    assert_eq!(fixture.network[0].task.key.sentence_id, old_key.sentence_id);
    assert!(fixture.network[0].task.key.revision > old_key.revision);
    fixture.finish(0, Some(Ok("新译文".into())));
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    let page = fixture.page();
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].translation, "新译文");
    assert_eq!(fixture.flow.test_view().translation, "新译文");
    fixture.close();
}
#[test]
fn stopping_clears_real_pending_rows_and_rejects_late_results_then_log_only_records_without_http() {
    let mut fixture = Fixture::new(false);
    fixture.forty();
    assert_eq!(fixture.disk_count(), 6);
    fixture.flow.stop();
    while !fixture.network.is_empty() {
        fixture.finish(0, Some(Ok("停止后的旧回包".into())));
    }
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    assert_eq!(fixture.disk_count(), 0);
    assert_eq!(fixture.page().total, 0);
    fixture.flow.begin(
        Settings::defaults(Screen::new(1920.0, 1080.0).unwrap()),
        true,
    );
    fixture.until(|fixture| fixture.flow.test_fence_ready());
    fixture.capture(
        "This source is recorded while translation is paused.".into(),
        1,
        0,
    );
    fixture
        .until(|fixture| fixture.flow.pending_saves() == 0 && fixture.flow.test_processing() == 0);
    assert!(fixture.network.is_empty());
    let page = fixture.page();
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].translation, "N/A");
    assert_eq!(page.rows[0].target_language, "N/A");
    assert_eq!(page.rows[0].api_used, "LogOnly");
    fixture.close();
}

#[test]
fn failed_disk_spill_keeps_the_accepted_sentence_until_explicit_retry() {
    let mut fixture = Fixture::new(false);
    for sentence in 1..=34 {
        let text = (1..=sentence)
            .map(|index| format!("This is complete sentence number {index}."))
            .collect::<Vec<_>>()
            .join(" ");
        fixture.capture(text, sentence, 0);
        fixture.until(|fixture| fixture.network.len() == 2 || sentence < 2);
    }
    let lock =
        rusqlite::Connection::open(fixture.root.join("data/translation_history.db")).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE;").unwrap();
    fixture.allow_blocked = true;
    fixture.flow.capture(
        CaptionSample {
            text: (1..=35)
                .map(|index| format!("This is complete sentence number {index}."))
                .collect::<Vec<_>>()
                .join(" "),
            changes: 35,
            unchanged: 0,
        },
        35,
    );
    fixture.until(|fixture| fixture.flow.is_blocked());
    assert_eq!(
        fixture.flow.test_retained_spill().unwrap().key.sentence_id,
        35
    );
    assert_eq!(fixture.flow.test_waiting(), 32);
    for _ in 0..20 {
        fixture.pump();
    }
    assert!(
        fixture.flow.is_blocked(),
        "a failure must not start an automatic retry loop"
    );
    lock.execute_batch("ROLLBACK;").unwrap();
    drop(lock);
    fixture.flow.retry();
    fixture.until(|fixture| !fixture.flow.is_blocked() && !fixture.flow.test_spill_pending());
    assert_eq!(fixture.disk_count(), 1);
    for sentence in 2..=35 {
        fixture.until(|fixture| {
            fixture
                .network
                .iter()
                .any(|request| request.task.key.sentence_id == sentence)
        });
        let position = fixture
            .network
            .iter()
            .position(|request| request.task.key.sentence_id == sentence)
            .unwrap();
        fixture.finish(position, Some(Ok(format!("译文{sentence}"))));
    }
    fixture.finish(0, Some(Ok("译文1".into())));
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    assert_eq!(fixture.page().total, 35);
    fixture.close();
}

#[test]
fn failed_completed_history_save_is_retained_across_stop_and_saved_before_the_stop_fence() {
    let mut fixture = Fixture::new(false);
    fixture.capture("A successfully translated public sentence.".into(), 1, 0);
    fixture.until(|fixture| fixture.network.len() == 1);
    let lock =
        rusqlite::Connection::open(fixture.root.join("data/translation_history.db")).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE;").unwrap();
    fixture.allow_blocked = true;
    fixture.finish(0, Some(Ok("必须保留的成功译文".into())));
    fixture.flow.stop();
    fixture.pump();
    fixture.until(|fixture| fixture.flow.is_blocked());
    assert!(fixture.flow.pending_saves() > 0);
    lock.execute_batch("ROLLBACK;").unwrap();
    drop(lock);
    assert_eq!(fixture.page().total, 0);
    fixture.flow.retry();
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    let page = fixture.page();
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].translation, "必须保留的成功译文");
    fixture.close();
}

#[test]
fn observed_complete_snapshots_stay_in_fifo_while_a_real_spill_write_is_blocked() {
    let mut fixture = Fixture::new(false);
    let snapshot = |sentence| {
        (1..=sentence)
            .map(|index| format!("This is complete sentence number {index}."))
            .collect::<Vec<_>>()
            .join(" ")
    };
    for sentence in 1..=34 {
        fixture.capture(snapshot(sentence), sentence, 0);
        fixture.until(|fixture| fixture.network.len() == 2 || sentence < 2);
    }
    let lock =
        rusqlite::Connection::open(fixture.root.join("data/translation_history.db")).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE;").unwrap();
    fixture.queue_capture(snapshot(35), 35, 0);
    fixture.pump();
    assert_eq!(
        fixture.flow.test_retained_spill().unwrap().key.sentence_id,
        35
    );
    fixture.queue_capture(snapshot(36), 36, 0);
    fixture.queue_capture(snapshot(37), 37, 0);
    fixture.pump();
    fixture.pump();
    assert_eq!(fixture.flow.test_spill_ids(), vec![35, 36, 37]);
    assert_eq!(fixture.flow.test_waiting(), 32);
    lock.execute_batch("ROLLBACK;").unwrap();
    drop(lock);
    fixture.until(|fixture| !fixture.flow.test_spill_pending());
    assert_eq!(fixture.disk_count(), 3);
    for sentence in 2..=37 {
        fixture.until(|fixture| {
            fixture
                .network
                .iter()
                .any(|request| request.task.key.sentence_id == sentence)
        });
        let position = fixture
            .network
            .iter()
            .position(|request| request.task.key.sentence_id == sentence)
            .unwrap();
        fixture.finish(position, Some(Ok(format!("译文{sentence}"))));
    }
    fixture.finish(0, Some(Ok("译文1".into())));
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    assert_eq!(fixture.page().total, 37);
    fixture.close();
}

#[test]
fn rejected_save_keeps_its_position_before_stop_fence_until_retry_is_durable() {
    let mut fixture = Fixture::new(false);
    fixture.capture(
        "A completed public sentence must survive a refused save.".into(),
        1,
        0,
    );
    fixture.until(|fixture| fixture.network.len() == 1);
    let request = fixture.network.remove(0);
    fixture.flow.completed(Completion {
        task: request.task,
        result: Some(Ok("拒绝后重试仍须保存".into())),
        latency_ms: 1,
    });
    fixture.flow.stop();
    let (request, key) = fixture.flow.test_save_request();
    // Deliberately deliver the supported refusal event, rather than pretending
    // an SQLite lock timeout yields Ignored (it yields Event::Error).
    fixture.flow.data_event(&Event::Saved {
        request,
        key,
        result: storage::history::Save::Ignored,
    });
    fixture.allow_blocked = true;
    fixture.pump();
    assert!(fixture.flow.is_blocked());
    assert_eq!(fixture.page().total, 0);
    fixture.flow.retry();
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    let page = fixture.page();
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].translation, "拒绝后重试仍须保存");
    fixture.close();
}

#[test]
fn rolling_capture_buffer_keeps_explicit_new_complete_sentences_separate() {
    let mut fixture = Fixture::new(false);
    fixture.capture(
        "A prior sentence. The same public sentence number 34.".into(),
        1,
        0,
    );
    fixture.until(|fixture| fixture.network.len() == 1);
    fixture.capture(
        "The same public sentence number 34. The same public sentence number 35.".into(),
        2,
        0,
    );
    fixture.capture("The same public sentence number 34. The same public sentence number 35. The same public sentence number 36.".into(), 3, 0);
    fixture.until(|fixture| fixture.network.len() == 2);
    assert!(fixture.network[1].task.source.trim().ends_with("35."));
    fixture.finish(1, Some(Ok("译文35".into())));
    fixture.until(|fixture| fixture.network.len() == 2);
    assert!(fixture.network[1].task.source.trim().ends_with("36."));
    fixture.finish(1, Some(Ok("译文36".into())));
    fixture.finish(0, Some(Ok("迟到译文34".into())));
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    assert_eq!(fixture.flow.test_view().translation, "译文36");
    assert_eq!(fixture.page().total, 3);
    fixture.close();
}

#[test]
fn earlier_prefix_revision_does_not_replay_a_common_unfinished_tail() {
    for rolling in [false, true] {
        for repetitions in [1, 3] {
            let mut fixture = Fixture::new(false);
            let first = "This is test passage number two.";
            let second = format!("{first} An earlier sentence has enough words.");
            let third = format!("{second} This is test passage number four.");
            for (index, snapshot) in [first.to_string(), second, third.clone()]
                .into_iter()
                .enumerate()
            {
                fixture.capture(snapshot, index as u64 + 1, 0);
                fixture.until(|fixture| fixture.network.len() == 1);
                fixture.finish(0, Some(Ok(format!("译文{index}"))));
                fixture.until(|fixture| fixture.flow.pending_saves() == 0);
            }
            fixture.capture(format!("{third} This is test passage number"), 4, 0);
            // Reformatting an earlier number or rolling the buffer must not
            // locate this unfinished phrase in an already translated sentence.
            let retained = if rolling {
                third
                    .strip_prefix(&format!("{first} "))
                    .unwrap()
                    .to_string()
            } else {
                third.replace("number two", "number 2")
            };
            let newest = "This is test passage number five.";
            for count in 1..=repetitions {
                fixture.capture(
                    format!("{retained} {}", vec![newest; count].join(" ")),
                    4 + count as u64,
                    0,
                );
                fixture.until(|fixture| !fixture.network.is_empty());
                assert_eq!(fixture.network[0].task.source.trim(), newest);
                fixture.finish(0, Some(Ok("译文5".into())));
            }
            fixture.until(|fixture| fixture.flow.pending_saves() == 0);
            assert_eq!(
                fixture.page().total,
                3 + repetitions as u64,
                "skip old context but preserve every genuine repeated new sentence"
            );
            fixture.close();
        }
    }
}

#[test]
fn rolling_tail_which_quotes_completed_context_keeps_both_new_sentences() {
    let mut fixture = Fixture::new(false);
    let opening = "Opening context.";
    let completed = format!("{opening} A prior sentence.");
    for (index, snapshot) in [opening.to_string(), completed.clone()]
        .into_iter()
        .enumerate()
    {
        fixture.capture(snapshot, index as u64 + 1, 0);
        fixture.until(|fixture| fixture.network.len() == 1);
        fixture.finish(0, Some(Ok("旧译文".into())));
        fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    }
    fixture.capture(format!("{completed} We now quote A prior"), 3, 0);
    fixture.capture("We now quote A prior sentence.".into(), 4, 0);
    fixture.capture(
        "We now quote A prior sentence. Another new sentence.".into(),
        5,
        0,
    );
    for source in ["We now quote A prior sentence.", "Another new sentence."] {
        fixture.until(|fixture| !fixture.network.is_empty());
        assert_eq!(fixture.network[0].task.source.trim(), source);
        fixture.finish(0, Some(Ok("新译文".into())));
    }
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    assert_eq!(fixture.page().total, 4);
    fixture.close();
}

#[test]
fn rolling_tail_overlapping_the_buffer_head_keeps_first_new_completion() {
    let mut fixture = Fixture::new(false);
    let opening = "This is test passage number two.";
    let completed = format!("{opening} Context sentence.");
    for (index, snapshot) in [opening.to_string(), completed.clone()]
        .into_iter()
        .enumerate()
    {
        fixture.capture(snapshot, index as u64 + 1, 0);
        fixture.until(|fixture| fixture.network.len() == 1);
        fixture.finish(0, Some(Ok("旧句译文".into())));
        fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    }
    fixture.capture(format!("{completed} This is test passage number"), 3, 0);
    fixture.capture("This is test passage number five.".into(), 4, 0);
    fixture.capture(
        "This is test passage number five. Another new sentence.".into(),
        5,
        0,
    );
    for source in ["This is test passage number five.", "Another new sentence."] {
        fixture.until(|fixture| !fixture.network.is_empty());
        assert_eq!(fixture.network[0].task.source.trim(), source);
        fixture.finish(0, Some(Ok("新句译文".into())));
    }
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    assert_eq!(fixture.page().total, 4);
    fixture.close();
}

#[test]
fn rolling_genuine_repetition_is_not_mistaken_for_retained_context() {
    let mut fixture = Fixture::new(false);
    let completed = "Opening context. A prior sentence.";
    for (index, snapshot) in ["Opening context.", completed].into_iter().enumerate() {
        fixture.capture(snapshot.into(), index as u64 + 1, 0);
        fixture.until(|fixture| fixture.network.len() == 1);
        fixture.finish(0, Some(Ok("旧译文".into())));
        fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    }
    fixture.capture(format!("{completed} This unfinished"), 3, 0);
    fixture.capture("This unfinished now completes.".into(), 4, 0);
    fixture.capture(
        "This unfinished now completes. A prior sentence.".into(),
        5,
        0,
    );
    fixture.capture(
        "This unfinished now completes. A prior sentence. This unfinished repeated completion."
            .into(),
        6,
        0,
    );
    for source in [
        "This unfinished now completes.",
        "A prior sentence.",
        "This unfinished repeated completion.",
    ] {
        fixture.until(|fixture| !fixture.network.is_empty());
        assert_eq!(fixture.network[0].task.source.trim(), source);
        fixture.finish(0, Some(Ok("新译文".into())));
    }
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    assert_eq!(fixture.page().total, 5);
    fixture.close();
}

#[test]
fn rolling_tail_with_repeated_retained_context_keeps_new_sentences() {
    let mut fixture = Fixture::new(false);
    let snapshots = [
        "Context sentence.",
        "Context sentence. This is test passage number two.",
        "Context sentence. This is test passage number two. Context sentence.",
    ];
    for (index, snapshot) in snapshots.into_iter().enumerate() {
        fixture.capture(snapshot.into(), index as u64 + 1, 0);
        fixture.until(|fixture| fixture.network.len() == 1);
        fixture.finish(0, Some(Ok("先前上下文译文".into())));
        fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    }
    fixture.capture(
        format!("{} This is test passage number", snapshots[2]),
        4,
        0,
    );
    fixture.capture(
        "Context sentence. This is test passage number five.".into(),
        5,
        0,
    );
    fixture.capture(
        "Context sentence. This is test passage number five. Another new sentence.".into(),
        6,
        0,
    );
    for source in ["This is test passage number five.", "Another new sentence."] {
        fixture.until(|fixture| !fixture.network.is_empty());
        assert_eq!(fixture.network[0].task.source.trim(), source);
        fixture.finish(0, Some(Ok("新句译文".into())));
    }
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    assert_eq!(fixture.page().total, 5);
    fixture.close();
}

#[test]
fn bounded_fifo_preserves_observed_captions_while_storage_and_the_buffer_roll_forward() {
    let mut fixture = Fixture::new(false);
    let snapshot = |start, end| {
        (start..=end)
            .map(|index| format!("This is complete sentence number {index}."))
            .collect::<Vec<_>>()
            .join(" ")
    };
    for sentence in 1..=34 {
        fixture.capture(snapshot(1, sentence), sentence, 0);
        fixture.until(|fixture| fixture.network.len() == 2 || sentence < 2);
    }
    let lock =
        rusqlite::Connection::open(fixture.root.join("data/translation_history.db")).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE;").unwrap();
    fixture.queue_capture(snapshot(1, 35), 35, 0);
    fixture.pump();
    let mailbox = fixture.mailbox.clone();
    let revision = fixture.capture_revision.clone();
    let producer = std::thread::spawn(move || {
        for sentence in 36..=121 {
            mailbox.capture(
                1,
                &revision,
                CaptionSample {
                    text: snapshot(if sentence >= 120 { 90 } else { 1 }, sentence),
                    changes: sentence,
                    unchanged: 0,
                },
            );
        }
    });
    // Do not sleep or require the MTA to poll through a full input FIFO.
    // Releasing this real SQLite lock lets the bounded pipeline advance.
    lock.execute_batch("ROLLBACK;").unwrap();
    drop(lock);
    fixture.until(|fixture| {
        producer.is_finished()
            && fixture.mailbox.test_source_count() == 0
            && !fixture.flow.test_sample_pending()
            && !fixture.flow.test_spill_pending()
    });
    producer.join().unwrap();
    assert_eq!(fixture.disk_count(), 87);
    for sentence in 2..=121 {
        fixture.until(|fixture| {
            fixture.network.iter().any(|request| {
                request.task.source.trim()
                    == format!("This is complete sentence number {sentence}.")
            })
        });
        let position = fixture
            .network
            .iter()
            .position(|request| {
                request.task.source.trim()
                    == format!("This is complete sentence number {sentence}.")
            })
            .unwrap();
        fixture.finish(position, Some(Ok(format!("译文{sentence}"))));
    }
    fixture.finish(0, Some(Ok("译文1".into())));
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    assert_eq!(fixture.page().total, 121);
    fixture.close();
}
