use super::*;
use protocol::{Empty, EngineState, Message};

pub(super) fn run(root: &std::path::Path, screen: Screen, ui: &MainWindow, jobs: &mpsc::Receiver<Job>) {
    let directory = root.join("caption-toggle");
    fs::create_dir(&directory).unwrap();
    ui.set_data_state(0);
    let data = Data::start(directory, screen, ui.as_weak()).unwrap();
    let (bridge, host) = Bridge::fixture();
    let owner = Rc::new(std::cell::RefCell::new(Controller::new(
        bridge,
        ui.as_weak(),
        data,
        screen,
    )));
    Controller::bind_caption_toggle(&owner);
    // Exercise the production callback before settings have arrived: no system
    // caption service is launched. The host and CC observe the same transition.
    ui.invoke_captions_changed(true);
    assert!(ui.get_engine_busy());
    assert!(!ui.get_captions_enabled());
    assert_eq!(states(&host), [EngineState::Starting]);
    // Process Stop before Ready can start a capture session.
    owner.borrow_mut().test_stop_engine();
    owner.borrow_mut().test_show_settings();
    until_owner(&owner, jobs, || ui.get_data_state() == 2);
    assert!(!ui.get_engine_busy());
    assert!(!ui.get_captions_enabled());
    states(&host);

    for via_tray in [false, true] {
        owner.borrow_mut().test_start_network_fixture(
            providers::Translator::local_fixture("Google", "http://127.0.0.1:1".into()),
            "Google",
        );
        assert!(ui.get_captions_enabled());
        assert_eq!(states(&host), [EngineState::Running]);
        if via_tray {
            owner
                .borrow()
                .bridge
                .queue_command(1, Message::StopEngine(Empty {}));
            owner.borrow_mut().wakeup();
        } else {
            ui.invoke_captions_changed(false);
        }
        assert!(!ui.get_captions_enabled());
        assert!(!ui.get_engine_busy());
        assert!(!ui.get_overlay_visible());
        assert_eq!(
            states(&host),
            [EngineState::Stopping, EngineState::Disabled]
        );
    }
    owner.borrow_mut().test_start_network_fixture(
        providers::Translator::local_fixture("Google", "http://127.0.0.1:1".into()),
        "Google",
    );
    states(&host);
    owner.borrow_mut().test_caption_fault("fixture");
    assert!(!ui.get_captions_enabled());
    assert!(!ui.get_engine_busy());
    assert_eq!(states(&host).last(), Some(&EngineState::Disabled));
    owner.borrow_mut().main_closed();
    until_owner(&owner, jobs, || owner.borrow().test_data_stopped());
    ui.hide().unwrap();
}

fn states(host: &mpsc::Receiver<crate::ui_bridge::Outbound>) -> Vec<EngineState> {
    host.try_iter()
        .filter_map(|event| match event {
            crate::ui_bridge::Outbound::Message(protocol::Envelope {
                message: Message::EngineStateChanged { state },
                ..
            }) => Some(state),
            _ => None,
        })
        .collect()
}
