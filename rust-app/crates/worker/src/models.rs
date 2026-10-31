//! A user-initiated model-list request has its own short-lived owner. Closing
//! or changing the editor cancels and joins it; no settings-only idle runtime.
use crate::MainWindow;
use providers::Model;
use std::{
    io,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};
pub struct ResultEvent {
    pub generation: u64,
    pub result: Result<Vec<Model>, String>,
}
pub type Slot = Arc<Mutex<Option<ResultEvent>>>;
pub struct Job {
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}
impl Job {
    pub fn start(
        generation: u64,
        url: String,
        slot: Slot,
        ui: slint::Weak<MainWindow>,
    ) -> io::Result<Self> {
        let (cancel, receiver) = tokio::sync::oneshot::channel();
        let thread=thread::Builder::new().name("model-list".into()).spawn(move || {
            let result = (|| {
                let runtime=tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_|"无法启动模型列表请求。".to_owned())?;
                let provider=providers::Translator::new().map_err(|error|error.to_string())?;
                let result=runtime.block_on(async {
                    tokio::select! {
                        result=provider.models("LMStudio",&url)=>Some(result.map_err(|error|error.to_string())),
                        _=receiver=>None,
                    }
                });
                runtime.shutdown_timeout(Duration::from_millis(200));
                Ok::<_,String>(result)
            })();
            let result=match result {Ok(Some(result))=>result,Ok(None)=>return,Err(error)=>Err(error)};
            *slot.lock().unwrap()=Some(ResultEvent {generation,result});
            let _=slint::invoke_from_event_loop(move || { if let Some(ui)=ui.upgrade() {ui.invoke_host_wakeup();} });
        })?;
        Ok(Self {
            cancel: Some(cancel),
            thread: Some(thread),
        })
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
