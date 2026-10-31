use super::*;
use slint::platform::{Key, PointerEventButton, WindowEvent};

pub(super) fn run(
    owner: &Rc<std::cell::RefCell<Controller>>,
    api: &crate::ApiWindow,
    window: &MinimalSoftwareWindow,
) {
    let model = |service: &str, index: usize| {
        owner.borrow().test_settings().json()["Configs"][service][index]["ModelName"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let append = |suffix: &str| {
        window.set_size(slint::PhysicalSize::new(950, 600));
        std::thread::sleep(Duration::from_millis(2));
        slint::platform::update_timers_and_animations();
        std::thread::sleep(Duration::from_millis(200));
        slint::platform::update_timers_and_animations();
        window.window().request_redraw();
        let mut pixels = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(950, 600);
        window.draw_if_needed(|renderer| {
            renderer.render(pixels.make_mut_slice(), 950);
        });
        let position = slint::LogicalPosition::new(350.0, 262.0);
        api.window().dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        api.window().dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
        api.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::End.into(),
        });
        api.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::End.into(),
        });
        for ch in suffix.chars() {
            let text: slint::SharedString = ch.to_string().into();
            api.window()
                .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
            api.window()
                .dispatch_event(WindowEvent::KeyReleased { text });
        }
    };
    let openai_second = model("OpenAI", 1);
    let lmstudio_second = model("LMStudio", 1);
    api.invoke_section_changed(3);
    append("-typed");
    assert_eq!(model("OpenAI", 1), format!("{openai_second}-typed"));
    api.invoke_section_changed(2);
    append("-edited");
    assert_eq!(model("LMStudio", 1), format!("{lmstudio_second}-edited"));
    assert_eq!(model("OpenAI", 1), format!("{openai_second}-typed"));
    api.invoke_section_changed(3);
    api.invoke_config_action(0);
    let openai_first = model("OpenAI", 0);
    append("-first");
    assert_eq!(model("OpenAI", 0), format!("{openai_first}-first"));
    api.invoke_config_action(1);
    append("-second");
    assert_eq!(model("OpenAI", 1), format!("{openai_second}-typed-second"));
    assert_eq!(model("OpenAI", 0), format!("{openai_first}-first"));
    assert_eq!(owner.borrow().test_settings().api_name(), "Google");
    println!(
        "API actual virtual input: service/config switching refreshes edited values and preserves unrelated configurations PASS"
    );
}
