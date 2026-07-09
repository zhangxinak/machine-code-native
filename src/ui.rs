use anyhow::{anyhow, Result};
use std::cell::RefCell;
use std::sync::{Arc, Mutex, OnceLock};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, LoadCursorW, MessageBoxW,
    PostQuitMessage, RegisterClassW, SetWindowTextW, ShowWindow, TranslateMessage, UpdateWindow,
    COLOR_WINDOW, CW_USEDEFAULT, HMENU, IDC_ARROW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MSG,
    SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_COMMAND, WM_CREATE, WM_DESTROY, WNDCLASSW,
    WS_BORDER, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

use crate::diagnostics;
use crate::hardware::{FieldResult, MachineInfo};
use crate::state::AppState;

const ID_AUTH: i32 = 1001;
const ID_CHECK_UPDATE: i32 = 1002;
const ID_USER_AGREEMENT: i32 = 1003;
const ID_PRIVACY_POLICY: i32 = 1004;
const DISPLAY_VERSION: &str = "2.1.0";

static APP_STATE: OnceLock<Arc<Mutex<AppState>>> = OnceLock::new();

thread_local! {
    static HANDLES: RefCell<Option<UiHandles>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy)]
struct UiHandles {
    auth_button: HWND,
    mac: HWND,
    motherboard: HWND,
    cpu: HWND,
    disk: HWND,
    software_version: HWND,
}

pub fn run(state: Arc<Mutex<AppState>>) -> Result<()> {
    APP_STATE
        .set(state)
        .map_err(|_| anyhow!("UI state has already been initialized"))?;

    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class_name = to_wide("MachineCodeNativeWindow");
        let title = to_wide("机器码获取工具");

        let wnd_class = WNDCLASSW {
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hInstance: instance.into(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            lpfnWndProc: Some(wnd_proc),
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as isize),
            ..Default::default()
        };

        let atom = RegisterClassW(&wnd_class);
        if atom == 0 {
            return Err(anyhow!("RegisterClassW failed"));
        }

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            790,
            490,
            None,
            None,
            Some(instance.into()),
            None,
        )?;

        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd)?;

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    Ok(())
}

extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            if let Err(error) = unsafe { create_controls(hwnd) } {
                diagnostics::append_log(format!("创建窗口控件失败: {}", error));
                show_message(hwnd, "错误", &format!("创建窗口控件失败: {}", error), true);
            }
            update_ui();
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xffff) as i32;
            handle_command(hwnd, id);
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

unsafe fn create_controls(hwnd: HWND) -> Result<()> {
    create_value(hwnd, 0, 0, 790, 2)?;

    create_label(hwnd, "机器码信息", 20, 18, 160, 28)?;
    let auth_button = create_button(hwnd, "开启授权", ID_AUTH, 650, 16, 100, 34)?;

    create_label(hwnd, "网卡MAC地址", 36, 72, 120, 26)?;
    let mac = create_value(hwnd, 170, 68, 560, 32)?;

    create_label(hwnd, "主板序列号", 36, 116, 120, 26)?;
    let motherboard = create_value(hwnd, 170, 112, 560, 32)?;

    create_label(hwnd, "CPU序列号", 36, 160, 120, 26)?;
    let cpu = create_value(hwnd, 170, 156, 560, 32)?;

    create_label(hwnd, "硬盘序列号", 36, 204, 120, 26)?;
    let disk = create_value(hwnd, 170, 200, 560, 32)?;

    create_label(hwnd, "软件信息", 20, 268, 160, 28)?;
    create_label(hwnd, "版本:", 36, 314, 70, 26)?;
    let software_version = create_value(hwnd, 104, 310, 120, 32)?;
    create_button(hwnd, "检查更新", ID_CHECK_UPDATE, 238, 310, 92, 32)?;

    create_button(hwnd, "用户协议", ID_USER_AGREEMENT, 292, 398, 92, 30)?;
    create_button(hwnd, "隐私策略", ID_PRIVACY_POLICY, 406, 398, 92, 30)?;

    HANDLES.with(|handles| {
        *handles.borrow_mut() = Some(UiHandles {
            auth_button,
            mac,
            motherboard,
            cpu,
            disk,
            software_version,
        });
    });

    Ok(())
}

fn handle_command(hwnd: HWND, id: i32) {
    match id {
        ID_AUTH => {
            with_state(|state| {
                let next = !state.authorized;
                state.set_authorized(next);
                if next {
                    let _ = state.machine_info(true);
                }
            });
            update_ui();
        }
        ID_CHECK_UPDATE => {
            show_message(hwnd, "检查更新", "当前已是最新版本", false);
        }
        ID_USER_AGREEMENT => {
            show_message(hwnd, "用户协议", user_agreement_text(), false);
        }
        ID_PRIVACY_POLICY => {
            show_message(hwnd, "隐私策略", privacy_policy_text(), false);
        }
        _ => {}
    }
}

fn update_ui() {
    let (authorized, info) = with_state(|state| (state.authorized, state.machine_info.clone()));

    HANDLES.with(|handles| {
        if let Some(handles) = *handles.borrow() {
            set_text(
                handles.auth_button,
                if authorized {
                    "取消授权"
                } else {
                    "开启授权"
                },
            );
            set_text(handles.software_version, DISPLAY_VERSION);

            if !authorized {
                set_text(handles.mac, "——未授权——");
                set_text(handles.motherboard, "——未授权——");
                set_text(handles.cpu, "——未授权——");
                set_text(handles.disk, "——未授权——");
                return;
            }

            if let Some(info) = info {
                set_machine_info(handles, &info);
            } else {
                set_text(handles.mac, "————");
                set_text(handles.motherboard, "————");
                set_text(handles.cpu, "————");
                set_text(handles.disk, "————");
            }
        }
    });
}

fn set_machine_info(handles: UiHandles, info: &MachineInfo) {
    set_text(handles.mac, &field_value(&info.mac));
    set_text(handles.motherboard, &field_value(&info.motherboard));
    set_text(handles.cpu, &field_value(&info.cpu));
    set_text(handles.disk, &field_value(&info.disk));
}

fn field_value(field: &FieldResult) -> String {
    if field.ok {
        field.value.clone()
    } else {
        "————".to_string()
    }
}

fn with_state<T>(f: impl FnOnce(&mut AppState) -> T) -> T {
    let state = APP_STATE.get().expect("APP_STATE initialized");
    let mut state = state.lock().expect("state lock poisoned");
    f(&mut state)
}

unsafe fn create_label(
    hwnd: HWND,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<HWND> {
    create_control(
        hwnd,
        "STATIC",
        text,
        0,
        x,
        y,
        width,
        height,
        WS_CHILD | WS_VISIBLE,
    )
}

unsafe fn create_value(hwnd: HWND, x: i32, y: i32, width: i32, height: i32) -> Result<HWND> {
    create_control(
        hwnd,
        "STATIC",
        "",
        0,
        x,
        y,
        width,
        height,
        WS_CHILD | WS_VISIBLE | WS_BORDER,
    )
}

unsafe fn create_button(
    hwnd: HWND,
    text: &str,
    id: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<HWND> {
    create_control(
        hwnd,
        "BUTTON",
        text,
        id,
        x,
        y,
        width,
        height,
        WS_CHILD | WS_VISIBLE,
    )
}

unsafe fn create_control(
    parent: HWND,
    class: &str,
    text: &str,
    id: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    style: WINDOW_STYLE,
) -> Result<HWND> {
    let class = to_wide(class);
    let text = to_wide(text);
    let instance = GetModuleHandleW(None)?;
    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        PCWSTR(class.as_ptr()),
        PCWSTR(text.as_ptr()),
        style,
        x,
        y,
        width,
        height,
        Some(parent),
        Some(HMENU(id as isize)),
        Some(instance.into()),
        None,
    )?;
    Ok(hwnd)
}

fn set_text(hwnd: HWND, text: &str) {
    let text = to_wide(text);
    unsafe {
        let _ = SetWindowTextW(hwnd, PCWSTR(text.as_ptr()));
    }
}

fn show_message(hwnd: HWND, title: &str, text: &str, error: bool) {
    let title = to_wide(title);
    let text = to_wide(text);
    let flags = if error {
        MB_OK | MB_ICONERROR
    } else {
        MB_OK | MB_ICONINFORMATION
    };
    unsafe {
        MessageBoxW(
            Some(hwnd),
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            flags,
        );
    }
}

fn user_agreement_text() -> &'static str {
    "用户协议\r\n\r\n欢迎使用机器码获取工具！\r\n\r\n本工具用于采集本机授权所需的硬件信息。点击开启授权即表示您同意工具读取必要的设备信息。"
}

fn privacy_policy_text() -> &'static str {
    "隐私策略\r\n\r\n本工具会读取网卡MAC地址、主板序列号、CPU序列号、硬盘序列号等设备信息，用于生成机器码和授权校验。"
}

fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
