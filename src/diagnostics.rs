use chrono::Local;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

pub fn app_data_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("machine-code-native")
}

pub fn log_path() -> PathBuf {
    app_data_dir().join("startup.log")
}

pub fn begin_session() {
    append_log("============================================================");
    append_log("新的程序启动会话");
}

pub fn append_log(message: impl AsRef<str>) {
    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let pid = std::process::id();
        let thread_id = format!("{:?}", std::thread::current().id());
        for line in message.as_ref().lines() {
            let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
            let _ = writeln!(
                file,
                "[{}] [pid:{}] [tid:{}] {}",
                timestamp, pid, thread_id, line
            );
        }
    }
}

pub fn read_log() -> String {
    fs::read_to_string(log_path()).unwrap_or_else(|_| "暂无日志".to_string())
}
