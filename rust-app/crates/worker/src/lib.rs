mod controller;
#[cfg(test)]
mod controller_tests;
mod data;
mod engine;
mod flow;
#[cfg(test)]
mod flow_tests;
mod models;
mod shutdown;
mod ui_bridge;

use controller::Controller;
use protocol::{Envelope, Message, PROTOCOL_VERSION, Ready};
use slint::ComponentHandle;
use std::{cell::RefCell, io, path::PathBuf, rc::Rc};
use ui_bridge::Bridge;

slint::include_modules!();

pub fn run(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut pipe = None;
    let mut host_pid = None;
    let mut data_dir = None;
    while let Some(argument) = args.next() {
        match argument.to_str() {
            Some("--pipe") => pipe = args.next().and_then(|value| value.into_string().ok()),
            Some("--host-pid") => {
                host_pid = args
                    .next()
                    .and_then(|value| value.to_str().and_then(|value| value.parse::<u32>().ok()))
            }
            Some("--data-dir") => data_dir = args.next().map(PathBuf::from),
            _ => return Err(io::Error::other("Invalid worker arguments").into()),
        }
    }
    let (pipe, host_pid, data_dir) = match (pipe, host_pid, data_dir) {
        (Some(pipe), Some(pid), Some(path)) if path.is_absolute() => (pipe, pid, path),
        _ => return Err(io::Error::other("Worker must be launched by the tray host").into()),
    };
    let pipe = windows_platform::pipe_client::connect(&pipe, host_pid)?;
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .select()?;
    let ui = MainWindow::new()?;
    let (width, height) = windows_platform::desktop::screen_size();
    let screen = captions_core::settings::Screen::new(width, height)
        .ok_or_else(|| io::Error::other("无法读取屏幕尺寸。"))?;
    let autostart = windows_platform::autostart::Registration::current(&data_dir);
    let data = data::Data::start(data_dir, screen, ui.as_weak())?;
    let bridge = Bridge::new(pipe, ui.as_weak())?;
    let controller = Rc::new(RefCell::new(Controller::new(
        bridge,
        ui.as_weak(),
        data,
        screen,
    )));
    match autostart {
        Ok(registration) => controller.borrow_mut().autostart = Some(registration),
        Err(error) => ui.set_autostart_message(format!("无法读取启动位置：{error}").into()),
    }
    let weak = Rc::downgrade(&controller);
    ui.on_host_wakeup(move || {
        if let Some(controller) = weak.upgrade() {
            controller.borrow_mut().wakeup();
        }
    });
    let weak = Rc::downgrade(&controller);
    ui.on_pause_changed(move |value| {
        if let Some(controller) = weak.upgrade() {
            controller.borrow_mut().pause_changed(value);
        }
    });
    Controller::bind_caption_toggle(&controller);
    let weak = Rc::downgrade(&controller);
    ui.on_autostart_changed(move |value| {
        if let Some(controller) = weak.upgrade() {
            controller.borrow_mut().autostart_changed(value);
        }
    });
    let weak = Rc::downgrade(&controller);
    ui.on_data_action(move |action, path| {
        if let Some(controller) = weak.upgrade() {
            controller.borrow_mut().data_action(action, path.as_str());
        }
    });
    let weak = Rc::downgrade(&controller);
    ui.on_prompt_changed(move |prompt| {
        if let Some(controller) = weak.upgrade() {
            controller.borrow_mut().prompt_changed(prompt.as_str());
        }
    });
    let weak = Rc::downgrade(&controller);
    ui.on_setting_edited(move |field, value| {
        if let Some(controller) = weak.upgrade() {
            controller
                .borrow_mut()
                .setting_edited(field.as_str(), value.as_str());
        }
    });
    let weak = Rc::downgrade(&controller);
    ui.on_topmost_changed(move |value| {
        if let Some(controller) = weak.upgrade() {
            controller
                .borrow_mut()
                .setting_edited("Topmost", if value { "true" } else { "false" });
        }
    });
    let weak = Rc::downgrade(&controller);
    ui.on_captions_visibility_changed(move |hidden| {
        if let Some(controller) = weak.upgrade() {
            controller.borrow_mut().captions_visibility_changed(hidden);
        }
    });
    let weak = Rc::downgrade(&controller);
    ui.on_api_requested(move || {
        if let Some(controller) = weak.upgrade() {
            Controller::open_api(&controller);
        }
    });
    let weak = Rc::downgrade(&controller);
    let ui_weak = ui.as_weak();
    ui.on_window_action(move |action| {
        use slint::winit_030::{WinitWindowAccessor, winit::window::ResizeDirection};
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        if action == 1 {
            if let Some(controller) = weak.upgrade() {
                controller.borrow_mut().main_closed();
            }
            let _ = ui.hide();
        } else {
            ui.window().with_winit_window(|window| match action {
                0 => window.set_minimized(true),
                2 => {
                    let _ = window.drag_window();
                }
                3 => {
                    let _ = window.drag_resize_window(ResizeDirection::SouthEast);
                }
                _ => {}
            });
        }
    });
    Controller::bind_history(&controller);
    Controller::bind_files(&controller);
    Controller::bind_caption(&controller);
    Controller::bind_delete(&controller);
    Controller::bind_main_geometry(&controller);
    let weak = Rc::downgrade(&controller);
    ui.window().on_close_requested(move || {
        if let Some(controller) = weak.upgrade() {
            controller.borrow_mut().main_closed();
        }
        slint::CloseRequestResponse::HideWindow
    });
    controller.borrow().bridge.send(Envelope::new(
        0,
        Message::Ready(Ready {
            worker_pid: std::process::id(),
            protocol_version: PROTOCOL_VERSION,
        }),
    ))?;
    // Engine may run while all Slint windows are hidden; controller alone decides exit.
    slint::run_event_loop_until_quit()?;
    controller.borrow().bridge.finish();
    Ok(())
}
