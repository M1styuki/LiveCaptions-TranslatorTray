use crate::MainWindow;
use protocol::{CHANNEL_CAPACITY, Empty, Envelope, Message, read_frame, write_frame};
use std::{
    collections::VecDeque,
    io,
    sync::{
        Arc, Mutex,
        mpsc::{self, SyncSender},
    },
    thread,
    time::Duration,
};

enum Outbound {
    Message(Envelope),
    Flush(SyncSender<()>),
}

pub struct Bridge {
    outbound: SyncSender<Outbound>,
    incoming: Arc<Mutex<VecDeque<Envelope>>>,
}

impl Bridge {
    #[cfg(test)]
    pub fn fixture() -> (Self, impl Sized) {
        let (outbound, receiver) = mpsc::sync_channel(CHANNEL_CAPACITY);
        (
            Self {
                outbound,
                incoming: Arc::default(),
            },
            receiver,
        )
    }
    pub fn new(
        file: windows_platform::pipe_stream::PipeStream,
        ui: slint::Weak<MainWindow>,
    ) -> io::Result<Self> {
        let mut reader = file.try_clone()?;
        let mut writer = file;
        let (outbound, receiver) = mpsc::sync_channel(CHANNEL_CAPACITY);
        thread::Builder::new()
            .name("worker-pipe-write".into())
            .spawn(move || {
                while let Ok(item) = receiver.recv() {
                    match item {
                        Outbound::Message(message) => {
                            if write_frame(&mut writer, &message).is_err() {
                                break;
                            }
                        }
                        Outbound::Flush(reply) => {
                            let _ = reply.send(());
                        }
                    }
                }
            })?;
        let incoming: Arc<Mutex<VecDeque<Envelope>>> = Arc::default();
        let queue = incoming.clone();
        thread::Builder::new()
            .name("worker-pipe-read".into())
            .spawn(move || {
                loop {
                    let (message, mut finished) = match read_frame(&mut reader) {
                        Ok(Some(message)) => (message, false),
                        Ok(None) | Err(_) => (Envelope::new(0, Message::Exit(Empty {})), true),
                    };
                    {
                        let mut queue = queue.lock().unwrap();
                        if matches!(message.message, Message::Exit(_)) {
                            queue.clear();
                        }
                        if queue.len() == CHANNEL_CAPACITY {
                            // A non-responsive UI cannot grow an unbounded command queue.
                            // Treat overflow as a lost host and follow the same save/exit path.
                            queue.clear();
                            queue.push_front(Envelope::new(0, Message::Exit(Empty {})));
                            finished = true;
                        } else {
                            queue.push_back(message);
                        }
                    }
                    let weak = ui.clone();
                    if slint::invoke_from_event_loop(move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.invoke_host_wakeup();
                        }
                    })
                    .is_err()
                    {
                        break;
                    }
                    if finished {
                        break;
                    }
                }
            })?;
        Ok(Self { outbound, incoming })
    }

    pub fn send(&self, message: Envelope) -> io::Result<()> {
        self.outbound
            .try_send(Outbound::Message(message))
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "Host message queue is unavailable",
                )
            })
    }

    pub fn take_commands(&self) -> Vec<Envelope> {
        self.incoming.lock().unwrap().drain(..).collect()
    }

    pub fn finish(&self) {
        let (reply, result) = mpsc::sync_channel(1);
        if self.outbound.try_send(Outbound::Flush(reply)).is_ok() {
            let _ = result.recv_timeout(Duration::from_millis(500));
        }
    }
}
