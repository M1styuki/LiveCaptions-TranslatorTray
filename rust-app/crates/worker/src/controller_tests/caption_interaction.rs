use super::*;
use gpui::{ScrollDelta, ScrollWheelEvent};

pub(super) fn run(
    root: &std::path::Path,
    screen: Screen,
    ui: &MainWindow,
    jobs: &TestAppContext,
    window: &mut VisualTestContext,
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
    for dark in [false, true] {
        ui.set_frame_dark(dark);
        redraw(window, 900., 240.);
    }
    let source = center(window, "source");
    let translation = center(window, "translation");
    let plain = Modifiers::none();
    let ctrl = Modifiers {
        control: true,
        ..plain
    };
    let both = Modifiers {
        shift: true,
        ..ctrl
    };
    let scroll = |window: &mut VisualTestContext, position, delta, modifiers| {
        window.simulate_mouse_move(position, None, modifiers);
        window.simulate_event(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(0.), px(delta))),
            modifiers,
            ..Default::default()
        });
    };
    scroll(window, source, 120., plain);
    assert_eq!(ui.get_original_font_size(), 15);
    scroll(window, source, 120., ctrl);
    assert_eq!(
        (ui.get_original_font_size(), ui.get_translated_font_size()),
        (16, 18)
    );
    scroll(window, source, 120., both);
    assert_eq!(ui.get_original_font_size(), 16);
    scroll(window, translation, -120., ctrl);
    assert_eq!(
        (ui.get_original_font_size(), ui.get_translated_font_size()),
        (16, 17)
    );
    window.simulate_click(source, plain);
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
    window.run_until_parked();
    window.executor().advance_clock(Duration::from_millis(2050));
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
    redraw(window, 900., 240.);
    let translation = center(window, "translation");
    window.simulate_click(translation, plain);
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
    ui.set_caption_cards(crate::ui::Rows::new(vec![crate::HistoryItem {
        source: "历史原文 😀".into(),
        translation: "历史译文 quoted".into(),
        ..Default::default()
    }]));
    ui.set_caption_log_enabled(true);
    redraw(window, 900., 360.);
    let before = copied.borrow().len();
    for y in (38..330).step_by(8) {
        window.simulate_click(point(px(210.), px(y as f32)), plain);
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
    ui.set_caption_cards(crate::ui::Rows::default());
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
}
