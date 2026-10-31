use super::*;
use captions_core::settings::{Bounds, WindowKind};

pub(super) fn run(
    root: &std::path::Path,
    screen: Screen,
    ui: &MainWindow,
    jobs: &mpsc::Receiver<Job>,
) {
    let directory = root.join("main-geometry-data");
    fs::create_dir(&directory).unwrap();
    for pass in 0..2 {
        ui.set_data_state(0);
        ui.set_page(0);
        let data = Data::start(directory.clone(), screen, ui.as_weak()).unwrap();
        let (bridge, _host) = Bridge::fixture();
        let mut controller = Controller::new(bridge, ui.as_weak(), data, screen);
        controller.test_main_geometry_fixture();
        until(&mut controller, jobs, |_| ui.get_data_state() == 2);
        if pass == 0 {
            controller.test_main_fail_next_apply();
        }
        controller.test_show_caption();
        let owner = Rc::new(std::cell::RefCell::new(controller));
        Controller::bind_main_geometry(&owner);
        if pass == 0 {
            assert_eq!(owner.borrow().test_main_geometry().bounds.height, 240.0);
            assert_eq!(
                owner.borrow().test_main_geometry().bounds.width,
                900.0,
                "failed initial native apply must retry saved bounds, not adopt the temporary window"
            );
            owner
                .borrow_mut()
                .setting_edited("CaptionLogEnabled", "true");
            owner.borrow_mut().setting_edited("DisplaySentences", "3");
            assert_eq!(owner.borrow().test_main_geometry().bounds.height, 440.0);
            assert_eq!(ui.window().size().height, 440);
            owner.borrow_mut().setting_edited("DisplaySentences", "6");
            owner.borrow_mut().test_main_work_height(540.0);
            assert_eq!(owner.borrow().test_main_geometry().bounds.height, 540.0);
            assert_eq!(ui.window().size().height, 540);
            owner.borrow_mut().test_main_work_height(1040.0);
            owner
                .borrow_mut()
                .setting_edited("CaptionLogEnabled", "false");
            assert_eq!(owner.borrow().test_main_geometry().bounds.height, 240.0);
            owner.borrow_mut().page_changed(2);
            assert_eq!(owner.borrow().test_main_geometry().bounds.height, 420.0);
            owner.borrow_mut().page_changed(0);
            assert_eq!(owner.borrow().test_main_geometry().bounds.height, 240.0);
            owner.borrow_mut().test_main_native_resize(
                Bounds {
                    x: 80.0,
                    y: 790.0,
                    width: 900.0,
                    height: 240.0,
                },
                false,
            );
            owner.borrow_mut().page_changed(1);
            let expanded = owner.borrow().test_main_geometry().bounds;
            assert_eq!(expanded.height, 600.0);
            assert!(
                expanded.y + expanded.height <= 1040.0,
                "expanding settings stays above the taskbar"
            );
            owner.borrow_mut().test_main_native_resize(
                Bounds {
                    x: 80.0,
                    y: 90.0,
                    width: 900.0,
                    height: 500.0,
                },
                false,
            );
            owner.borrow_mut().page_changed(2);
            assert_eq!(
                owner.borrow().test_main_geometry().bounds.height,
                500.0,
                "page change before the geometry debounce must preserve the user's pending resize"
            );
            owner.borrow_mut().page_changed(0);
            owner.borrow_mut().test_main_native_resize(
                Bounds {
                    x: 80.0,
                    y: 90.0,
                    width: 900.0,
                    height: 500.0,
                },
                true,
            );
            assert!(
                owner
                    .borrow()
                    .test_settings()
                    .window_manual_size(WindowKind::Main)
            );
            owner.borrow_mut().setting_edited("DisplaySentences", "5");
            owner
                .borrow_mut()
                .setting_edited("CaptionLogEnabled", "true");
            assert_eq!(owner.borrow().test_main_geometry().bounds.height, 660.0);
            owner
                .borrow_mut()
                .setting_edited("CaptionLogEnabled", "false");
            assert_eq!(owner.borrow().test_main_geometry().bounds.height, 660.0);
            // Close before any debounce tick: final physical bounds still save.
            owner.borrow_mut().test_main_native_resize(
                Bounds {
                    x: 80.0,
                    y: 90.0,
                    width: 1150.0,
                    height: 660.0,
                },
                false,
            );
        } else {
            let actual = owner.borrow().test_main_geometry();
            assert_eq!(
                actual.bounds,
                Bounds {
                    x: 80.0,
                    y: 90.0,
                    width: 1150.0,
                    height: 660.0
                }
            );
            owner.borrow_mut().page_changed(2);
            assert_eq!(owner.borrow().test_main_geometry().bounds.height, 660.0);
            owner.borrow_mut().page_changed(0);
            assert_eq!(owner.borrow().test_main_geometry().bounds.height, 660.0);
        }
        if pass == 0 {
            Controller::open_api(&owner); // Keep the same worker/controller alive.
            owner.borrow_mut().main_closed();
            ui.hide().unwrap();
            owner.borrow_mut().test_main_recreated_native(2.0);
            owner.borrow_mut().test_show_caption();
            assert_eq!(
                owner.borrow().test_main_geometry().bounds,
                Bounds {
                    x: 80.0,
                    y: 90.0,
                    width: 1150.0,
                    height: 660.0
                },
                "same-controller reopen must restore logical saved bounds after DPI changes"
            );
            owner.borrow_mut().test_close_api();
        }
        owner.borrow_mut().main_closed();
        until_owner(&owner, jobs, || owner.borrow().test_data_stopped());
        let (saved, _) = captions_core::settings::Settings::parse(
            &fs::read_to_string(directory.join("setting.json")).unwrap(),
            screen,
        )
        .unwrap();
        assert_eq!(
            saved.window_bounds(WindowKind::Main),
            Bounds {
                x: 80.0,
                y: 90.0,
                width: 1150.0,
                height: 660.0
            }
        );
        assert!(saved.window_manual_size(WindowKind::Main));
        let anchor = saved.window_anchor(WindowKind::Main).unwrap();
        assert_eq!(anchor.monitor, "fixture-main");
        assert_eq!((anchor.x, anchor.y), (80.0, 90.0));
        ui.hide().unwrap();
    }
    println!(
        "Main geometry: injected native placement + actual virtual Slint and isolated settings; initial restore, card growth/shrink, manual preservation, necessary growth, close-before-debounce and reopened placement PASS; not visible desktop/Winit/DPI acceptance"
    );
}
