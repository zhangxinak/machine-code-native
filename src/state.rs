use crate::diagnostics;
use crate::hardware::{collect_machine_info, MachineInfo};

#[derive(Debug)]
pub struct AppState {
    pub authorized: bool,
    pub machine_info: Option<MachineInfo>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            authorized: false,
            machine_info: None,
        }
    }

    pub fn set_authorized(&mut self, authorized: bool) {
        self.authorized = authorized;
        diagnostics::append_log(format!("授权状态已变更: {}", authorized));
        if !authorized {
            self.machine_info = None;
        }
    }

    pub fn machine_info(&mut self, refresh: bool) -> MachineInfo {
        if refresh || self.machine_info.is_none() {
            self.machine_info = Some(collect_machine_info());
        }
        self.machine_info
            .clone()
            .expect("machine_info must be initialized")
    }
}
