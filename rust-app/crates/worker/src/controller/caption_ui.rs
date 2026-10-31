use super::Controller;
use slint::{
    ComponentHandle,
    winit_030::{
        WinitWindowAccessor,
        winit::raw_window_handle::{HasWindowHandle, RawWindowHandle},
    },
};
use std::{cell::RefCell, io, rc::Rc, time::Duration};
#[cfg(test)]
type Writer = Rc<dyn Fn(&str) -> io::Result<()>>;
#[derive(Default)]
pub(super) struct Interaction {
    timer: slint::Timer,
    #[cfg(test)]
    writer: Option<Writer>,
}
impl Controller {
    pub fn bind_caption(owner: &Rc<RefCell<Self>>) {
        let Some(ui) = owner.borrow().ui.upgrade() else {
            return;
        };
        let weak = Rc::downgrade(owner);
        ui.on_caption_copy_requested(move |text| {
            if let Some(owner) = weak.upgrade() {
                owner.borrow_mut().copy_caption(text.as_str());
            }
        });
        let weak = Rc::downgrade(owner);
        ui.on_caption_font_adjusted(move |role, delta| {
            if let Some(owner) = weak.upgrade() {
                owner.borrow_mut().adjust_caption_font(role, delta);
            }
        });
    }
    fn clipboard_write(&self, ui: &crate::MainWindow, text: &str) -> io::Result<()> {
        #[cfg(test)]
        if let Some(writer) = &self.caption.writer {
            return writer(text);
        }
        let mut result = Err(io::Error::other("字幕窗口尚未就绪。"));
        ui.window().with_winit_window(|window| {
            if let Ok(handle) = window.window_handle()
                && let RawWindowHandle::Win32(handle) = handle.as_raw()
            {
                result = windows_platform::clipboard::write_text(handle.hwnd.get() as usize, text);
            }
        });
        result
    }
    pub fn copy_caption(&mut self, text: &str) {
        if self.exiting || text.is_empty() {
            return;
        }
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let message = if self.clipboard_write(&ui, text).is_ok() {
            "已复制"
        } else {
            "复制失败，请稍后再试。"
        };
        ui.set_copy_message(message.into());
        let weak = self.ui.clone();
        self.caption.timer.start(
            slint::TimerMode::SingleShot,
            Duration::from_secs(2),
            move || {
                if let Some(ui) = weak.upgrade() {
                    ui.set_copy_message("".into());
                }
            },
        );
    }
    pub fn adjust_caption_font(&mut self, role: i32, delta: i32) {
        if self.exiting || delta == 0 {
            return;
        }
        let field = match role {
            0 => "OriginalFontSize",
            1 => "TranslatedFontSize",
            _ => return,
        };
        let Some(settings) = &self.settings else {
            return;
        };
        let current = settings.json()["MainWindow"][field].as_i64().unwrap();
        let next = (current + i64::from(delta.signum())).clamp(8, 40);
        if next == current {
            return;
        }
        if let Ok((value, _)) = settings.edited(self.screen, |root| {
            root["MainWindow"][field] = next.into();
        }) {
            self.save_settings(value);
        }
    }
    pub(super) fn clear_copy_feedback(&mut self) {
        self.caption.timer.stop();
        if let Some(ui) = self.ui.upgrade() {
            ui.set_copy_message("".into());
        }
    }
    #[cfg(test)]
    pub fn test_clipboard_writer(&mut self, writer: Writer) {
        self.caption.writer = Some(writer);
    }
    #[cfg(test)]
    pub fn test_copy_timer_running(&self) -> bool {
        self.caption.timer.running()
    }
    #[cfg(test)]
    pub fn test_show_caption(&mut self) {
        self.show(0, self.hidden).unwrap();
    }
}
