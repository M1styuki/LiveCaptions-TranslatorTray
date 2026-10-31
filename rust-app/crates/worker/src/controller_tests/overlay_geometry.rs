use super::*;
use captions_core::{
    geometry::{Anchor, Placement},
    settings::{Bounds, Settings, WindowKind},
};

pub(super) fn run(
    root: &std::path::Path,
    screen: Screen,
    ui: &MainWindow,
    jobs: &mpsc::Receiver<Job>,
) {
    let directory = root.join("overlay-geometry-data");
    fs::create_dir(&directory).unwrap();
    let mut original = Settings::defaults(screen).json().clone();
    original["FixtureUnknown"] = serde_json::json!({"keep": "unchanged"});
    fs::write(
        directory.join("setting.json"),
        serde_json::to_vec(&original).unwrap(),
    )
    .unwrap();
    ui.set_data_state(0);
    let data = Data::start(directory.clone(), screen, ui.as_weak()).unwrap();
    let (bridge, _host) = Bridge::fixture();
    let mut controller = Controller::new(bridge, ui.as_weak(), data, screen);
    until(&mut controller, jobs, |_| ui.get_data_state() == 2);
    let final_value = Placement {
        bounds: Bounds {
            x: -900.0,
            y: 80.0,
            width: 920.0,
            height: 310.0,
        },
        anchor: Anchor {
            monitor: "fixture-left".into(),
            x: 60.0,
            y: 80.0,
        },
    };
    let mut wrong = final_value.clone();
    wrong.bounds.width = 700.0;
    controller.test_queue_placement(wrong.clone(), false, false);
    controller.test_queue_style(windows_platform::overlay::Settings {
        font_size: 29,
        ..Default::default()
    });
    controller.test_queue_placement(final_value.clone(), true, false);
    controller.test_close_overlay(); // consumes latest final event before invalidating generation
    controller.test_queue_placement(wrong, false, true);
    controller.wakeup(); // old window cannot overwrite the accepted final rectangle
    controller.main_closed();
    until(&mut controller, jobs, |controller| {
        controller.test_data_stopped()
    });
    let bytes = fs::read_to_string(directory.join("setting.json")).unwrap();
    let (saved, _) = Settings::parse(&bytes, screen).unwrap();
    assert_eq!(saved.window_bounds(WindowKind::Overlay), final_value.bounds);
    assert_eq!(
        saved.window_anchor(WindowKind::Overlay),
        Some(final_value.anchor)
    );
    assert!(saved.window_manual_size(WindowKind::Overlay));
    let value: serde_json::Value = serde_json::from_str(&bytes).unwrap();
    assert_eq!(value["OverlayWindow"]["FontSize"], 29);
    assert_eq!(
        value["WindowBounds"]["MainWindow"],
        original["WindowBounds"]["MainWindow"]
    );
    for key in ["FixtureUnknown", "Configs"] {
        assert_eq!(
            value[key], original[key],
            "overlay geometry must preserve {key}"
        );
    }
    println!(
        "Overlay geometry controller: queued final style/placement, stale generation rejection, isolated durable file and unrelated fields PASS; no visible/native overlay opened"
    );
}
