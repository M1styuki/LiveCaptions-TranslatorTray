//! Bounded business-event inbox. Unlike caption rendering, save acknowledgements
//! are never replaced by a newer event. Only the storage thread may wait for room.
use crate::MainWindow;
use captions_core::settings::Screen;
use std::{
    io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
use storage::actor::{self, Command, Event, Storage};

pub struct Data {
    pub instance: String,
    owner: Option<Storage>,
    pub client: actor::Client,
    events: mpsc::Receiver<Event>,
    wake_pending: Arc<AtomicBool>,
}
impl Data {
    #[cfg(test)]
    pub fn is_stopped(&self) -> bool {
        self.owner.is_none()
    }
    pub fn start(
        directory: PathBuf,
        screen: Screen,
        ui: slint::Weak<MainWindow>,
    ) -> io::Result<Self> {
        // Only load this version's data directory. Do not discover/import files
        // beside the EXE; existing interrupted recovery still uses its manifest.
        let legacy = directory.clone();
        let instance = windows_platform::desktop::instance_id()?;
        let (sender, events) = mpsc::sync_channel(actor::CAPACITY);
        let wake_pending = Arc::new(AtomicBool::new(false));
        let wake = wake_pending.clone();
        let owner = Storage::start(
            directory,
            legacy,
            screen,
            instance.clone(),
            Arc::new(move |event| {
                if sender.send(event).is_err() {
                    return;
                }
                if !wake.swap(true, Ordering::AcqRel) {
                    let ui = ui.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui.upgrade() {
                            ui.invoke_host_wakeup();
                        }
                    });
                }
            }),
        )?;
        Ok(Self {
            instance,
            client: owner.client(),
            owner: Some(owner),
            events,
            wake_pending,
        })
    }
    pub fn submit(&self, command: Command) -> io::Result<()> {
        self.client.submit(command).map_err(io::Error::other)
    }
    pub fn take(&self) -> Vec<Event> {
        // Reset before draining: a racing producer either joins this batch or
        // queues another wake. No callback needs to acquire a UI-owned lock.
        self.wake_pending.store(false, Ordering::Release);
        self.events.try_iter().collect()
    }
    pub fn stopped(&mut self) -> io::Result<()> {
        if let Some(owner) = self.owner.take() {
            owner.join()?;
        }
        Ok(())
    }
}
