use crate::diagnostics;
use crate::hardware::MachineInfo;

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

    pub fn set_machine_info(&mut self, machine_info: MachineInfo) {
        self.machine_info = Some(machine_info);
        diagnostics::append_log("机器码缓存已更新");
    }
}
