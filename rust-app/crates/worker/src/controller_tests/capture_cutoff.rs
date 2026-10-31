//! Actual controller callbacks/SQLite with a virtual UI and no ASR/HTTP.
use super::*;

pub(super) fn run(
    root: &std::path::Path,
    screen: Screen,
    ui: &MainWindow,
    jobs: &mpsc::Receiver<Job>,
) {
    let directory = root.join("capture-cutoff-data");
    fs::create_dir(&directory).unwrap();
    ui.set_data_state(0);
    ui.set_log_only(false);
    let data = Data::start(directory.clone(), screen, ui.as_weak()).unwrap();
    let (bridge, _host) = Bridge::fixture();
    let mut controller = Controller::new(bridge, ui.as_weak(), data, screen);
    until(&mut controller, jobs, |_| ui.get_data_state() == 2);
    controller.test_show_caption();
    // No request can be dispatched before the first pause callback. These
    // observations deliberately accumulate without controller wakeups.
    controller.test_start_network_fixture(
        providers::Translator::local_fixture("Google", "http://127.0.0.1:1".into()),
        "Google",
    );
    let snapshot = |end: u64| {
        (1..=end)
            .map(|n| format!("Public cutoff sentence number {n}."))
            .collect::<Vec<_>>()
            .join(" ")
    };
    for index in 1..=10 {
        controller.test_queue_capture(&snapshot(index));
    }
    ui.set_log_only(true);
    controller.pause_changed(true);
    let database = rusqlite::Connection::open(directory.join("translation_history.db")).unwrap();
    let sources = || {
        database
            .prepare("SELECT SourceText FROM TranslationHistory ORDER BY Id")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>()
    };
    until(&mut controller, jobs, |_| sources().len() == 1);
    assert_eq!(sources()[0].trim(), "Public cutoff sentence number 10.");
    for index in 11..=20 {
        controller.test_queue_capture(&snapshot(index));
    }
    controller.setting_edited("TargetLanguage", "ja-JP");
    until(&mut controller, jobs, |_| sources().len() == 2);
    assert_eq!(sources()[1].trim(), "Public cutoff sentence number 20.");
    for index in 21..=30 {
        controller.test_queue_capture(&snapshot(index));
    }
    controller.delete_action(0);
    controller.delete_action(2);
    until(&mut controller, jobs, |_| !ui.get_delete_busy());
    assert!(
        sources().is_empty(),
        "pre-delete backlog must not recreate history"
    );
    controller.test_capture(&snapshot(30));
    for _ in 0..8 {
        controller.wakeup();
    }
    assert!(
        sources().is_empty(),
        "the current deleted sentence remains suppressed"
    );
    controller.test_capture(&snapshot(31));
    until(&mut controller, jobs, |_| sources().len() == 1);
    assert_eq!(sources()[0].trim(), "Public cutoff sentence number 31.");
    let translated: String = database
        .query_row("SELECT TranslatedText FROM TranslationHistory", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(translated, "N/A");
    controller.test_stop_engine();
    controller.main_closed();
    until(&mut controller, jobs, |controller| {
        controller.test_data_stopped()
    });
    println!(
        "Capture cutoff: pause/config/delete adopt latest observation, discard pre-action backlog, preserve genuinely new sentence; virtual UI/real SQLite, no ASR/HTTP PASS"
    );
}
