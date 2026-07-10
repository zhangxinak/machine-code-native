use anyhow::{anyhow, Result};
use serde_json::Value;
use std::process::Command;
use std::time::Instant;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::diagnostics;

const USER_AGREEMENT_URL: &str =
    "https://srm.cnzgc.com/base/sys/setting/findConfByKey?key=hardware_information_tool_user_agreement";
const PRIVACY_POLICY_URL: &str =
    "https://srm.cnzgc.com/base/sys/setting/findConfByKey?key=hardware_information_tool_privacy_policy";

const DEFAULT_USER_AGREEMENT: &str = "用户协议\r\n\r\n欢迎使用机器码获取工具。\r\n\r\n本工具用于采集本机必要的硬件识别信息，以便完成软件授权、设备识别和问题排查。请在确认同意后继续使用。";
const DEFAULT_PRIVACY_POLICY: &str = "隐私策略\r\n\r\n本工具可能读取网卡 MAC 地址、主板序列号、CPU 序列号、硬盘序列号等设备信息，用于生成机器码和完成授权校验。相关信息仅用于工具功能实现和技术支持。";

pub fn user_agreement_text() -> String {
    remote_content_dialog_text("用户协议", USER_AGREEMENT_URL, DEFAULT_USER_AGREEMENT)
}

pub fn privacy_policy_text() -> String {
    remote_content_dialog_text("隐私策略", PRIVACY_POLICY_URL, DEFAULT_PRIVACY_POLICY)
}

fn remote_content_dialog_text(title: &str, url: &str, fallback_content: &str) -> String {
    diagnostics::append_log(format!(
        "准备打开远程内容弹窗: title={}, url={}",
        title, url
    ));

    match fetch_remote_content(url) {
        Ok(result) => {
            if let Some(content) = extract_content(&result) {
                diagnostics::append_log(format!(
                    "远程内容解析成功: title={}, content_chars={}",
                    title,
                    content.chars().count()
                ));
                return content;
            }

            diagnostics::append_log(format!("远程内容响应未解析到正文: title={}", title));
            format!(
                "{}\r\n\r\n提示：在线内容为空或接口结构不符合预期，已显示本地兜底内容。\r\n\r\n响应预览：\r\n{}",
                fallback_content,
                format_result_preview(&result)
            )
        }
        Err(error) => {
            diagnostics::append_log(format!(
                "远程内容获取失败: title={}, error={}",
                title, error
            ));
            format!(
                "{}\r\n\r\n提示：无法加载在线内容，已显示本地兜底内容。\r\n失败原因：{}",
                fallback_content, error
            )
        }
    }
}

fn fetch_remote_content(url: &str) -> Result<Value> {
    let started = Instant::now();
    diagnostics::append_log(format!("开始获取远程内容: {}", url));

    let text = fetch_text_with_powershell(url).map_err(|error| {
        let message = format!("网络请求失败: {}", error);
        diagnostics::append_log(&message);
        anyhow!(message)
    })?;

    diagnostics::append_log(format!(
        "远程内容响应长度: {} 字节, elapsed_ms={}",
        text.len(),
        started.elapsed().as_millis()
    ));

    match serde_json::from_str::<Value>(&text) {
        Ok(value) => Ok(value),
        Err(error) => {
            let preview: String = text.chars().take(300).collect();
            diagnostics::append_log(format!(
                "JSON解析失败，按纯文本返回: {}; 响应预览: {}",
                error, preview
            ));
            Ok(serde_json::json!({ "v": text }))
        }
    }
}

fn fetch_text_with_powershell(url: &str) -> Result<String> {
    let script = format!(
        "$ErrorActionPreference='Stop';\
         $ProgressPreference='SilentlyContinue';\
         try {{ [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 }} catch {{ }};\
         [Console]::OutputEncoding = [Text.Encoding]::UTF8;\
         $request = [Net.HttpWebRequest]::Create('{}');\
         $request.Method = 'GET';\
         $request.UserAgent = 'Machine-Code-Tool/2.1.0';\
         $request.Timeout = 10000;\
         $request.ReadWriteTimeout = 10000;\
         $request.Accept = 'application/json,text/plain,*/*';\
         $response = $request.GetResponse();\
         try {{\
           $stream = $response.GetResponseStream();\
           $reader = New-Object IO.StreamReader($stream, [Text.Encoding]::UTF8);\
           [Console]::Write($reader.ReadToEnd());\
         }} finally {{\
           if ($null -ne $reader) {{ $reader.Dispose() }};\
           if ($null -ne $response) {{ $response.Close() }};\
         }}",
        powershell_single_quote(url)
    );

    let mut command = Command::new("powershell");
    command.args([
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        &script,
    ]);
    hide_child_console_window(&mut command);

    let output = command
        .output()
        .map_err(|error| anyhow!("启动 PowerShell 失败: {}", error))?;

    if !output.status.success() {
        return Err(anyhow!(
            "PowerShell 退出码: {}, stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn powershell_single_quote(value: &str) -> String {
    value.replace('\'', "''")
}

fn extract_content(result: &Value) -> Option<String> {
    if let Some(content) = normalize_content_value(result) {
        return Some(content);
    }

    let paths: &[&[&str]] = &[
        &["v"],
        &["value"],
        &["content"],
        &["text"],
        &["html"],
        &["message"],
        &["data", "v"],
        &["data", "value"],
        &["data", "content"],
        &["data", "text"],
        &["data", "html"],
        &["result", "v"],
        &["result", "value"],
        &["result", "content"],
        &["rows", "0", "v"],
        &["rows", "0", "value"],
        &["rows", "0", "content"],
        &["list", "0", "v"],
        &["list", "0", "value"],
        &["records", "0", "v"],
        &["records", "0", "value"],
    ];

    for path in paths {
        if let Some(content) = path_content(result, path).and_then(normalize_content_value) {
            return Some(content);
        }
    }

    search_content_deep(result, 0)
}

fn path_content<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for key in path {
        current = match current {
            Value::Object(map) => map.get(*key)?,
            Value::Array(items) => items.get(key.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(current)
}

fn search_content_deep(value: &Value, depth: usize) -> Option<String> {
    if depth > 4 {
        return None;
    }

    match value {
        Value::Object(map) => {
            let preferred_keys = [
                "v",
                "value",
                "content",
                "text",
                "html",
                "body",
                "configValue",
                "confValue",
            ];
            for key in preferred_keys {
                if let Some(content) = map.get(key).and_then(normalize_content_value) {
                    return Some(content);
                }
            }

            for child in map.values() {
                if let Some(content) = search_content_deep(child, depth + 1) {
                    return Some(content);
                }
            }
            None
        }
        Value::Array(items) => {
            for item in items {
                if let Some(content) = search_content_deep(item, depth + 1) {
                    return Some(content);
                }
            }
            None
        }
        _ => normalize_content_value(value),
    }
}

fn normalize_content_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.trim().is_empty() => Some(text.clone()),
        _ => None,
    }
}

fn format_result_preview(result: &Value) -> String {
    let text = serde_json::to_string_pretty(result).unwrap_or_else(|_| result.to_string());
    if text.is_empty() {
        return "空响应".to_string();
    }
    if text.chars().count() > 1200 {
        let preview: String = text.chars().take(1200).collect();
        format!("{}\r\n...(已截断)", preview)
    } else {
        text
    }
}

#[cfg(windows)]
fn hide_child_console_window(command: &mut Command) {
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_child_console_window(_command: &mut Command) {}
