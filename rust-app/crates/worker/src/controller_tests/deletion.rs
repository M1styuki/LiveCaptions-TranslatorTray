use super::*;
pub(super) fn run(
    root: &std::path::Path,
    screen: Screen,
    ui: &MainWindow,
    jobs: &TestAppContext,
    window: &mut VisualTestContext,
) {
    let directory = root.join("delete-ui-data");
    fs::create_dir(&directory).unwrap();
    ui.set_data_state(0);
    ui.set_history_query("".into());
    ui.set_export_message("".into());
    ui.set_delete_message("".into());
    let data = Data::start(directory.clone(), screen, ui.as_weak()).unwrap();
    let (bridge, _host) = Bridge::fixture();
    let mut controller = Controller::new(bridge, ui.as_weak(), data, screen);
    until(&mut controller, jobs, |_| ui.get_data_state() == 2);
    let original_config = fs::read(directory.join("setting.json")).unwrap();
    let database = rusqlite::Connection::open(directory.join("translation_history.db")).unwrap();
    for index in 1..=3 {
        database.execute("INSERT INTO TranslationHistory(Timestamp,SourceText,TranslatedText) VALUES('1700000000',?1,'公开删除测试')",[format!("public deletion fixture {index}")]).unwrap();
    }
    let untouched = root.join("old-user-history-fixture.db");
    fs::write(&untouched, b"old unrelated database fixture").unwrap();
    let count = || {
        database
            .query_row("SELECT COUNT(*) FROM TranslationHistory", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
    };
    controller.test_show_history();
    let owner = Rc::new(std::cell::RefCell::new(controller));
    Controller::bind_history(&owner);
    Controller::bind_delete(&owner);
    until_owner(&owner, jobs, || owner.borrow().test_history_idle());
    assert_eq!(ui.get_history_total(), "3");
    ui.invoke_delete_action(2);
    owner.borrow_mut().wakeup();
    assert!(
        !ui.get_delete_busy(),
        "confirm callback alone is not authorization"
    );
    assert_eq!(count(), 3);
    ui.invoke_delete_action(0);
    assert!(ui.get_delete_confirm());
    for dark in [false, true] {
        ui.set_frame_dark(dark);
        redraw(window, 800., 420.);
    }
    window.simulate_keystrokes("enter");
    assert!(!ui.get_delete_confirm(), "default Enter cancels");
    assert_eq!(count(), 3);
    ui.invoke_delete_action(0);
    redraw(window, 800., 420.);
    window.simulate_keystrokes("escape");
    assert!(!ui.get_delete_confirm());
    assert_eq!(count(), 3);
    ui.invoke_history_action(2, 0); // An older page result cannot restore rows.
    ui.invoke_delete_action(0);
    redraw(window, 800., 420.);
    window.simulate_keystrokes("tab enter");
    assert!(
        ui.get_delete_busy(),
        "Tab from cancel reaches explicit confirmation"
    );
    until_owner(&owner, jobs, || {
        !ui.get_delete_busy() && owner.borrow().test_history_idle()
    });
    assert_eq!(count(), 0);
    assert_eq!(ui.get_history_total(), "0");
    assert_eq!((ui.get_history_page(), ui.get_history_pages()), (1, 1));
    assert_eq!(ui.get_history_rows().row_count(), 0);
    assert_eq!(
        fs::read(directory.join("setting.json")).unwrap(),
        original_config
    );
    assert_eq!(
        fs::read(&untouched).unwrap(),
        b"old unrelated database fixture"
    );
    database.execute("INSERT INTO TranslationHistory(Timestamp,SourceText) VALUES('1700000000','retained after deletion failure')",[]).unwrap();
    database.execute_batch("CREATE TRIGGER fixture_delete_failure BEFORE DELETE ON TranslationHistory BEGIN SELECT RAISE(ABORT,'controlled delete failure'); END;").unwrap();
    ui.invoke_delete_action(0);
    ui.invoke_delete_action(2);
    owner.borrow_mut().test_host_exit();
    until_owner(&owner, jobs, || {
        !ui.get_delete_busy() && ui.get_delete_message().starts_with("删除未完成")
    });
    assert_eq!(count(), 1);
    assert!(ui.window().is_visible());
    assert!(
        !owner.borrow().test_data_stopped(),
        "failure during Exit reopens recovery before Shutdown"
    );
    owner
        .borrow_mut()
        .prompt_changed("Independent setting after failed delete {0}");
    until_owner(&owner, jobs, || ui.get_data_message() == "设置已保存。");
    assert!(
        ui.get_delete_message().starts_with("删除未完成"),
        "settings acknowledgement cannot hide the deletion error"
    );
    for _ in 0..10 {
        owner.borrow_mut().wakeup();
    }
    assert_eq!(count(), 1, "no automatic retry");
    database
        .execute_batch("DROP TRIGGER fixture_delete_failure")
        .unwrap();
    ui.invoke_delete_action(0);
    ui.invoke_delete_action(2);
    until_owner(&owner, jobs, || {
        !ui.get_delete_busy() && owner.borrow().test_history_idle()
    });
    assert_eq!(count(), 0);
    assert_eq!(ui.get_delete_message(), "已清空全部历史。");
    drop(database);
    owner.borrow_mut().main_closed();
    until_owner(&owner, jobs, || owner.borrow().test_data_stopped());
}
