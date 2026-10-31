use super::*;

pub(super) fn run(
    root: &std::path::Path,
    screen: Screen,
    ui: &MainWindow,
    jobs: &mpsc::Receiver<Job>,
) {
    for mode in ["existing", "first-use", "recovery"] {
        let directory = root.join(format!("pending-start-{mode}"));
        fs::create_dir(&directory).unwrap();
        match mode {
            "existing" => fs::write(
                directory.join("setting.json"),
                serde_json::to_vec(captions_core::settings::Settings::defaults(screen).json())
                    .unwrap(),
            )
            .unwrap(),
            "recovery" => fs::write(directory.join("setting.json"), b"{broken fixture").unwrap(),
            _ => {}
        }
        ui.hide().unwrap();
        ui.set_data_state(0);
        let data = Data::start(directory.clone(), screen, ui.as_weak()).unwrap();
        let (bridge, _host) = Bridge::fixture();
        let mut controller = Controller::new(bridge, ui.as_weak(), data, screen);
        // The actor may have queued Ready already, but the UI owner has not
        // consumed it. This deterministically reproduces the host's early Start.
        controller.test_wait_for_first_use();
        assert_eq!(
            controller.test_windows(),
            0,
            "loading is not first use: {mode}"
        );
        assert!(!ui.window().is_visible());
        if mode != "recovery" {
            controller.test_stop_engine(); // stop before queued Ready must cancel pending start
        } else {
            until(&mut controller, jobs, |_| ui.get_data_state() == 4);
            assert_eq!(
                controller.test_windows(),
                1,
                "a real decision must still show its UI"
            );
            assert!(ui.window().is_visible());
            ui.hide().unwrap();
            controller.main_closed();
        }
        until(&mut controller, jobs, |controller| {
            controller.test_data_stopped()
        });
        assert_eq!(controller.test_windows(), 0);
        assert!(!ui.window().is_visible());
        assert_eq!(
            controller.test_pending_start_and_engine(),
            (false, protocol::EngineState::Disabled)
        );
    }
    for synchronous in [false, true] {
        let directory = root.join(format!("pending-start-show-failure-{synchronous}"));
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("setting.json"), b"{broken fixture").unwrap();
        ui.hide().unwrap();
        ui.set_data_state(0);
        let data = Data::start(directory.clone(), screen, ui.as_weak()).unwrap();
        let (bridge, _host) = Bridge::fixture();
        let mut controller = Controller::new(bridge, ui.as_weak(), data, screen);
        if synchronous {
            until(&mut controller, jobs, |_| ui.get_data_state() == 4);
        }
        controller.test_fail_show();
        assert_eq!(controller.test_try_pending_start().is_err(), synchronous);
        until(&mut controller, jobs, |controller| {
            controller.test_data_stopped()
        });
        assert_eq!(
            controller.test_pending_start_and_engine(),
            (false, protocol::EngineState::Disabled)
        );
        assert_eq!(controller.test_windows(), 0);
        assert!(!ui.window().is_visible());
        assert_eq!(
            fs::read(directory.join("setting.json")).unwrap(),
            b"{broken fixture"
        );
    }
    println!(
        "Pending startup: loading no window, pre-Ready Stop cancels, damaged-data recovery prompts preserved, synchronous/asynchronous show failure leaves no pending start/busy state and exits PASS; virtual UI, no system caption launch"
    );
}
