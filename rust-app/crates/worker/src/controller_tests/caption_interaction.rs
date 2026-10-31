use super::*;
use slint::platform::{Key, WindowEvent};

pub(super) fn run(
    root: &std::path::Path,
    screen: Screen,
    ui: &MainWindow,
    jobs: &mpsc::Receiver<Job>,
    window: &MinimalSoftwareWindow,
) {
    let directory = root.join("caption-interaction-data");
    fs::create_dir(&directory).unwrap();
    ui.set_data_state(0);
    let data = Data::start(directory.clone(), screen, ui.as_weak()).unwrap();
    let (bridge, _host) = Bridge::fixture();
    let mut controller = Controller::new(bridge, ui.as_weak(), data, screen);
    until(&mut controller, jobs, |_| ui.get_data_state() == 2);
    controller.test_show_caption();
    let copied = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let failed = Rc::new(std::cell::Cell::new(false));
    let sink = copied.clone();
    let fail = failed.clone();
    controller.test_clipboard_writer(Rc::new(move |text| {
        if fail.get() {
            return Err(std::io::Error::other("controlled fixture failure"));
        }
        sink.borrow_mut().push(text.into());
        Ok(())
    }));
    let owner = Rc::new(std::cell::RefCell::new(controller));
    Controller::bind_caption(&owner);
    assert_eq!(
        (ui.get_original_font_size(), ui.get_translated_font_size()),
        (15, 18)
    );
    ui.set_source_caption("Public original caption.".into());
    ui.set_translated_caption("公开译文。".into());
    let evidence = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../evidence/functional")
        .join(format!(
            "caption-offscreen-{}",
            windows_platform::desktop::instance_id().unwrap()
        ));
    fs::create_dir_all(&evidence).unwrap();
    fs::write(evidence.join("scope.json"),br#"{"scope":"virtual Slint input and software renderer; injected clipboard writer; no real clipboard/desktop interaction","size":[900,240],"card_size":[900,360],"fonts":[[15,18],[40,40]],"history_card_fonts":[12,16]}"#).unwrap();
    for (name, theme) in [
        ("dark", slint::language::ColorScheme::Dark),
        ("light", slint::language::ColorScheme::Light),
    ] {
        ui.global::<crate::Palette>().set_color_scheme(theme);
        std::thread::sleep(Duration::from_millis(180));
        slint::platform::update_timers_and_animations();
        snapshot(
            window,
            &evidence.join(format!("caption-default-{name}.bmp")),
            900,
            240,
        );
    }
    let source = slint::LogicalPosition::new(210.0, 65.0);
    let translation = slint::LogicalPosition::new(210.0, 111.0);
    let scroll = |position, delta| {
        ui.window().dispatch_event(WindowEvent::PointerScrolled {
            position,
            delta_x: 0.0,
            delta_y: delta,
        });
    };
    // Actual Slint TouchArea input; do not duplicate its modifier predicate.
    scroll(source, 120.0);
    assert_eq!(ui.get_original_font_size(), 15);
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Control.into(),
    });
    scroll(source, 120.0);
    assert_eq!(
        (ui.get_original_font_size(), ui.get_translated_font_size()),
        (16, 18)
    );
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Shift.into(),
    });
    scroll(source, 120.0);
    assert_eq!(ui.get_original_font_size(), 16);
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Shift.into(),
    });
    scroll(translation, -120.0);
    assert_eq!(
        (ui.get_original_font_size(), ui.get_translated_font_size()),
        (16, 17)
    );
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Control.into(),
    });
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position: source,
        button: slint::platform::PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position: source,
        button: slint::platform::PointerEventButton::Left,
    });
    assert_eq!(copied.borrow().last().unwrap(), "Public original caption.");
    assert_eq!(ui.get_copy_message(), "已复制");
    let exact = "  中文 😀, \"quoted\"\nline two\r\n  ";
    ui.invoke_caption_copy_requested(exact.into());
    assert_eq!(copied.borrow().last().unwrap(), exact);
    failed.set(true);
    ui.invoke_caption_copy_requested("private fixture must not appear in feedback".into());
    assert_eq!(ui.get_copy_message(), "复制失败，请稍后再试。");
    assert!(!ui.get_copy_message().contains("private fixture"));
    let before = copied.borrow().len();
    ui.invoke_caption_copy_requested("".into());
    assert_eq!(copied.borrow().len(), before);
    assert!(owner.borrow().test_copy_timer_running());
    std::thread::sleep(Duration::from_millis(2050));
    slint::platform::update_timers_and_animations();
    assert_eq!(ui.get_copy_message(), "");
    assert!(!owner.borrow().test_copy_timer_running());
    failed.set(false);
    ui.invoke_caption_font_adjusted(0, 120);
    assert_eq!(
        ui.get_original_font_size(),
        17,
        "wheel magnitude does not change the step"
    );
    for _ in 0..100 {
        ui.invoke_caption_font_adjusted(0, 1);
        ui.invoke_caption_font_adjusted(1, 1);
    }
    assert_eq!(
        (ui.get_original_font_size(), ui.get_translated_font_size()),
        (40, 40)
    );
    ui.set_source_caption("Long public caption for font forty. 原文应完整换行并可滚动访问。第二行与 emoji 😀 保持可读。".into());
    ui.set_translated_caption(
        "四十号字号的译文。长句应按宽度换行，并能滚动查看最后一行，不能被旧的固定高度直接裁掉。"
            .into(),
    );
    for (name, theme) in [
        ("dark", slint::language::ColorScheme::Dark),
        ("light", slint::language::ColorScheme::Light),
    ] {
        ui.global::<crate::Palette>().set_color_scheme(theme);
        std::thread::sleep(Duration::from_millis(180));
        slint::platform::update_timers_and_animations();
        scroll(source, 10000.0);
        snapshot(
            window,
            &evidence.join(format!("caption-forty-top-{name}.bmp")),
            900,
            240,
        );
        scroll(translation, -10000.0);
        snapshot(
            window,
            &evidence.join(format!("caption-forty-bottom-{name}.bmp")),
            900,
            240,
        );
    }
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position: translation,
        button: slint::platform::PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position: translation,
        button: slint::platform::PointerEventButton::Left,
    });
    assert_eq!(
        copied.borrow().last().unwrap(),
        ui.get_translated_caption().as_str(),
        "copy includes the whole long translation, beyond the visible fragment"
    );
    for _ in 0..25 {
        ui.invoke_caption_font_adjusted(0, -1);
    }
    for _ in 0..22 {
        ui.invoke_caption_font_adjusted(1, -1);
    }
    ui.set_copy_message("".into());
    ui.set_source_caption("当前原文".into());
    ui.set_translated_caption("当前译文".into());
    ui.set_caption_cards(slint::ModelRc::new(slint::VecModel::from(vec![
        crate::HistoryItem {
            source: "历史原文 😀".into(),
            translation: "历史译文 quoted".into(),
            ..Default::default()
        },
    ])));
    ui.set_caption_log_enabled(true);
    for (name, theme) in [
        ("dark", slint::language::ColorScheme::Dark),
        ("light", slint::language::ColorScheme::Light),
    ] {
        ui.global::<crate::Palette>().set_color_scheme(theme);
        std::thread::sleep(Duration::from_millis(180));
        slint::platform::update_timers_and_animations();
        scroll(source, 10000.0);
        snapshot(
            window,
            &evidence.join(format!("caption-cards-{name}.bmp")),
            900,
            360,
        );
    }
    // Sweep only this virtual column: every displayed text block must copy its
    // own whole value, independently of the exact wrapping/layout coordinates.
    let before = copied.borrow().len();
    for y in (46..330).step_by(8) {
        let position = slint::LogicalPosition::new(210.0, y as f32);
        ui.window().dispatch_event(WindowEvent::PointerPressed {
            position,
            button: slint::platform::PointerEventButton::Left,
        });
        ui.window().dispatch_event(WindowEvent::PointerReleased {
            position,
            button: slint::platform::PointerEventButton::Left,
        });
    }
    for expected in ["当前原文", "当前译文", "历史原文 😀", "历史译文 quoted"] {
        assert!(
            copied.borrow()[before..]
                .iter()
                .any(|value| value == expected),
            "each current/history block copies its own exact value"
        );
    }
    ui.set_caption_log_enabled(false);
    ui.set_caption_cards(slint::ModelRc::default());
    for _ in 0..100 {
        ui.invoke_caption_font_adjusted(0, -1);
    }
    ui.invoke_caption_font_adjusted(1, -1);
    assert_eq!(
        (ui.get_original_font_size(), ui.get_translated_font_size()),
        (8, 17)
    );
    ui.invoke_caption_font_adjusted(9, 1);
    ui.invoke_caption_font_adjusted(0, 0);
    assert_eq!(ui.get_original_font_size(), 8);
    ui.invoke_caption_copy_requested("final public copy".into());
    assert!(owner.borrow().test_copy_timer_running());
    owner.borrow_mut().main_closed();
    assert!(!owner.borrow().test_copy_timer_running());
    let before = copied.borrow().len();
    ui.invoke_caption_font_adjusted(0, 1);
    ui.invoke_caption_copy_requested("rejected after exit".into());
    assert_eq!(copied.borrow().len(), before);
    until_owner(&owner, jobs, || owner.borrow().test_data_stopped());
    drop(owner);
    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.join("setting.json")).unwrap()).unwrap();
    assert_eq!(settings["MainWindow"]["OriginalFontSize"], 8);
    assert_eq!(settings["MainWindow"]["TranslatedFontSize"], 17);
    ui.set_data_state(0);
    let data = Data::start(directory, screen, ui.as_weak()).unwrap();
    let (bridge, _reopened_host) = Bridge::fixture();
    let mut controller = Controller::new(bridge, ui.as_weak(), data, screen);
    until(&mut controller, jobs, |_| ui.get_data_state() == 2);
    assert_eq!(
        (ui.get_original_font_size(), ui.get_translated_font_size()),
        (8, 17)
    );
    controller.main_closed();
    until(&mut controller, jobs, |value| value.test_data_stopped());
    println!(
        "Caption interaction evidence: {}; virtual Ctrl/plain/Ctrl+Shift gestures, injected exact/failed clipboard, one-shot feedback, independent bounds, close-save/reload PASS; no real clipboard or desktop",
        evidence.display()
    );
}
