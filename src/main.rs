#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::sync::{Arc, Mutex};

mod diagnostics;
mod hardware;
mod server;
mod state;
mod ui;

use state::AppState;

fn main() {
    diagnostics::clear_log();
    diagnostics::append_log("机器码获取工具 Native 版启动");

    let state = Arc::new(Mutex::new(AppState::new()));
    server::start_server(Arc::clone(&state));

    if let Err(error) = ui::run(state) {
        diagnostics::append_log(format!("UI 启动失败: {}", error));
        eprintln!("UI 启动失败: {}", error);
    }

    diagnostics::append_log("程序退出");
}
