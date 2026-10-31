use protocol::EngineState;

#[derive(Default)]
pub struct ShutdownState {
    pub visible_windows: u32,
    pub overlay_open: bool,
    pub pending_saves: usize,
}

impl ShutdownState {
    pub fn may_exit(&self, engine: EngineState) -> bool {
        engine == EngineState::Disabled
            && self.visible_windows == 0
            && !self.overlay_open
            && self.pending_saves == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_requires_disabled_engine_no_windows_no_overlay_and_saved_data() {
        let mut state = ShutdownState::default();
        assert!(state.may_exit(EngineState::Disabled));
        for engine in [
            EngineState::Running,
            EngineState::Starting,
            EngineState::Stopping,
            EngineState::Faulted,
        ] {
            assert!(!state.may_exit(engine));
        }
        state.visible_windows = 1;
        assert!(!state.may_exit(EngineState::Disabled));
        state.visible_windows = 0;
        state.overlay_open = true;
        assert!(!state.may_exit(EngineState::Disabled));
        state.overlay_open = false;
        state.pending_saves = 1;
        assert!(!state.may_exit(EngineState::Disabled));
    }
}
