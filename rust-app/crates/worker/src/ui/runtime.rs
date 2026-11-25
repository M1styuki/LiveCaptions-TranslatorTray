use futures::{StreamExt, channel::mpsc};
use gpui::{App, AsyncApp, Task};
use std::{cell::RefCell, collections::VecDeque, rc::Rc, sync::OnceLock, time::Duration};

type Job = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct UiJobs {
    scheduled: bool,
    pending: VecDeque<Box<dyn FnOnce()>>,
}
static WORKER_JOBS: OnceLock<mpsc::UnboundedSender<Job>> = OnceLock::new();

thread_local! {
    static UI_JOBS: RefCell<UiJobs> = RefCell::default();
    static APP: RefCell<Option<AsyncApp>> = const { RefCell::new(None) };
}

pub fn start(cx: &App) {
    let (sender, mut receiver) = mpsc::unbounded::<Job>();
    assert!(WORKER_JOBS.set(sender).is_ok());
    APP.with_borrow_mut(|app| *app = Some(cx.to_async()));
    cx.spawn(async move |_| {
        while let Some(job) = receiver.next().await {
            job();
        }
    })
    .detach();
}

pub fn app() -> Option<AsyncApp> {
    APP.with_borrow(Clone::clone)
}

/// Controller callbacks run between GPUI updates. In particular they can open a
/// second window or restore a native window's geometry.
pub fn dispatch(job: impl FnOnce() + 'static) {
    let Some(app) = app() else {
        job();
        return;
    };
    // GPUI may draw between input events. Commit their snapshots in order.
    let schedule = UI_JOBS.with_borrow_mut(|ui| {
        ui.pending.push_back(Box::new(job));
        !std::mem::replace(&mut ui.scheduled, true)
    });
    if schedule {
        app.spawn(async move |_| {
            loop {
                let next = UI_JOBS.with_borrow_mut(|ui| {
                    let next = ui.pending.pop_front();
                    if next.is_none() {
                        ui.scheduled = false;
                    }
                    next
                });
                let Some(job) = next else {
                    break;
                };
                job();
            }
        })
        .detach();
    }
}

pub fn invoke_from_event_loop(job: impl FnOnce() + Send + 'static) -> Result<(), ()> {
    WORKER_JOBS
        .get()
        .ok_or(())?
        .unbounded_send(Box::new(job))
        .map_err(|_| ())
}

pub fn quit_event_loop() -> Result<(), ()> {
    #[cfg(not(test))]
    if let Some(app) = app() {
        app.update(|cx| cx.quit()).map_err(|_| ())?;
    }
    Ok(())
}

#[derive(Default)]
pub struct Timer {
    task: RefCell<Option<Task<()>>>,
    pending: Rc<std::cell::Cell<bool>>,
}

impl Timer {
    pub fn start(&self, delay: Duration, callback: impl FnOnce() + 'static) {
        self.stop();
        self.pending.set(true);
        let Some(app) = app() else {
            return;
        };
        let timer = app.background_executor().timer(delay);
        let pending = self.pending.clone();
        *self.task.borrow_mut() = Some(app.spawn(async move |_| {
            timer.await;
            pending.set(false);
            callback();
        }));
    }

    pub fn stop(&self) {
        self.task.borrow_mut().take();
        self.pending.set(false);
    }

    #[cfg(test)]
    pub fn running(&self) -> bool {
        self.pending.get()
    }
}
