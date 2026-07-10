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
        let previous = self.authorized;
        let had_machine_info = self.machine_info.is_some();
        self.authorized = authorized;
        diagnostics::append_log(format!(
            "授权状态已变更: previous={}, current={}, had_machine_info={}",
            previous, authorized, had_machine_info
        ));
        if !authorized {
            self.machine_info = None;
            diagnostics::append_log("授权关闭: 已清空机器码缓存");
        }
    }

    pub fn set_machine_info(&mut self, machine_info: MachineInfo) {
        let confidence = machine_info.confidence.clone();
        let machine_id = machine_info.machine_id.clone();
        self.machine_info = Some(machine_info);
        diagnostics::append_log(format!(
            "机器码缓存已更新: confidence={}, machine_id={}",
            confidence, machine_id
        ));
    }
}
