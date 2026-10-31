use super::*;
use slint::platform::{Key, PointerEventButton, WindowEvent};

slint::slint! {
    import { ChoiceBox, IntegerBox } from "../../ui/controls.slint";
    export { ModelLookup } from "../../ui/controls.slint";
    export component FlatControlsTest inherits Window {
        width:360px;height:280px;
        in-out property <string> selected:"First";
        in-out property <int> interval:3;
        ChoiceBox {x:20px;y:20px;width:200px;model:["First","Second","Third"];current-value <=> root.selected;}
        IntegerBox {x:20px;y:220px;width:200px;minimum:0;maximum:10;number <=> root.interval;}
    }
}

pub(super) fn run(windows: &Rc<std::cell::RefCell<Vec<Rc<MinimalSoftwareWindow>>>>) {
    for scale in [1.0, 1.5] {
        run_at_scale(windows, scale);
    }
}

fn run_at_scale(windows: &Rc<std::cell::RefCell<Vec<Rc<MinimalSoftwareWindow>>>>, scale: f32) {
    let ui = FlatControlsTest::new().unwrap();
    ui.global::<ModelLookup>().on_index_of(crate::model_index);
    ui.show().unwrap();
    let window = windows.borrow().last().unwrap().clone();
    ui.window().dispatch_event(WindowEvent::ScaleFactorChanged {
        scale_factor: scale,
    });
    let (width, height) = ((360.0 * scale) as u32, (280.0 * scale) as u32);
    window.set_size(slint::PhysicalSize::new(width, height));
    let mut pixels = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(width, height);
    window.draw_if_needed(|renderer| {
        renderer.render(pixels.make_mut_slice(), width as usize);
    });
    let click = |x, y| {
        let position = slint::LogicalPosition::new(x, y);
        for event in [
            WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            },
            WindowEvent::PointerReleased {
                position,
                button: PointerEventButton::Left,
            },
        ] {
            ui.window().dispatch_event(event);
        }
    };
    let key = |text: slint::SharedString| {
        ui.window()
            .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        ui.window()
            .dispatch_event(WindowEvent::KeyReleased { text });
    };
    click(100.0, 36.0);
    key(Key::DownArrow.into());
    key(Key::Return.into());
    assert_eq!(ui.get_selected(), "Second");
    key("x".into());
    assert_eq!(
        ui.get_selected(),
        "Second",
        "selector must not accept typing"
    );
    click(100.0, 36.0);
    key(Key::DownArrow.into());
    key(Key::Escape.into());
    assert_eq!(
        ui.get_selected(),
        "Second",
        "escape cancels pending selection"
    );
    let position = slint::LogicalPosition::new(178.0, 235.0);
    ui.window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    assert_eq!(ui.get_interval(), 4, "press increments once immediately");
    std::thread::sleep(Duration::from_millis(550));
    slint::platform::update_timers_and_animations();
    assert_eq!(ui.get_interval(), 5, "held button begins repeating");
    std::thread::sleep(Duration::from_millis(70));
    slint::platform::update_timers_and_animations();
    assert_eq!(ui.get_interval(), 6, "held button continues repeating");
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
    std::thread::sleep(Duration::from_millis(70));
    slint::platform::update_timers_and_animations();
    assert_eq!(ui.get_interval(), 6, "release stops without an extra step");
    ui.set_interval(3);
    click(178.0, 235.0);
    assert_eq!(ui.get_interval(), 4, "increase arrow steps by one");
    click(206.0, 235.0);
    assert_eq!(ui.get_interval(), 3, "decrease arrow steps by one");
    key(Key::UpArrow.into());
    assert_eq!(ui.get_interval(), 4);
    key(Key::DownArrow.into());
    assert_eq!(ui.get_interval(), 3);
    for _ in 0..15 {
        key(Key::DownArrow.into());
    }
    assert_eq!(ui.get_interval(), 0);
    for _ in 0..15 {
        key(Key::UpArrow.into());
    }
    assert_eq!(ui.get_interval(), 10);
    ui.hide().unwrap();
}
