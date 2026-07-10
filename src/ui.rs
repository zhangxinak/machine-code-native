use anyhow::{anyhow, Result};
use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::{Arc, Mutex, OnceLock};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, UpdateWindow, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, COLOR_WINDOW,
    DEFAULT_CHARSET, DEFAULT_PITCH, FF_DONTCARE, FW_BOLD, FW_NORMAL, HBRUSH, HFONT,
    OUT_DEFAULT_PRECIS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::EM_SETMARGINS;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, LoadCursorW, MessageBoxW,
    PostQuitMessage, RegisterClassW, SendMessageW, SetWindowTextW, ShowWindow, TranslateMessage,
    BS_GROUPBOX, BS_PUSHBUTTON, CW_USEDEFAULT, EC_LEFTMARGIN, EC_RIGHTMARGIN, ES_AUTOHSCROLL,
    ES_READONLY, HMENU, IDC_ARROW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MSG, SW_SHOW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_SETFONT, WNDCLASSW,
    WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP,
    WS_VISIBLE,
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
        let module = GetModuleHandleW(None)?;
        let instance = HINSTANCE(module.0);
        let class_name = to_wide("MachineCodeNativeWindow");
        let title = to_wide("机器码获取工具");

        let wnd_class = WNDCLASSW {
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hInstance: instance,
            lpszClassName: PCWSTR(class_name.as_ptr()),
            lpfnWndProc: Some(wnd_proc),
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as isize as *mut c_void),
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
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            790,
            470,
            HWND::default(),
            HMENU::default(),
            instance,
            None,
        )?;

        ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);

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
    let font = create_font(-15, FW_NORMAL.0 as i32, "Microsoft YaHei");
    let section_font = create_font(-17, FW_BOLD.0 as i32, "Microsoft YaHei");
    let value_font = create_font(-14, FW_NORMAL.0 as i32, "Microsoft YaHei");

    let machine_group = create_group(hwnd, "机器码信息", 24, 22, 732, 250)?;
    set_font(machine_group, section_font);

    let auth_button = create_button(hwnd, "开启授权", ID_AUTH, 638, 48, 92, 32)?;
    set_font(auth_button, font);

    let mac_label = create_label(hwnd, "网卡MAC地址", 52, 72, 102, 24)?;
    set_font(mac_label, font);
    let mac = create_value(hwnd, 170, 68, 540, 30)?;
    set_font(mac, value_font);

    let motherboard_label = create_label(hwnd, "主板序列号", 52, 116, 102, 24)?;
    set_font(motherboard_label, font);
    let motherboard = create_value(hwnd, 170, 112, 540, 30)?;
    set_font(motherboard, value_font);

    let cpu_label = create_label(hwnd, "CPU序列号", 52, 160, 102, 24)?;
    set_font(cpu_label, font);
    let cpu = create_value(hwnd, 170, 156, 540, 30)?;
    set_font(cpu, value_font);

    let disk_label = create_label(hwnd, "硬盘序列号", 52, 204, 102, 24)?;
    set_font(disk_label, font);
    let disk = create_value(hwnd, 170, 200, 540, 30)?;
    set_font(disk, value_font);

    let software_group = create_group(hwnd, "软件信息", 24, 294, 360, 82)?;
    set_font(software_group, section_font);

    let version_label = create_label(hwnd, "版本:", 52, 329, 46, 24)?;
    set_font(version_label, font);
    let software_version = create_value(hwnd, 106, 325, 112, 30)?;
    set_font(software_version, value_font);

    let check_update = create_button(hwnd, "检查更新", ID_CHECK_UPDATE, 236, 325, 96, 30)?;
    set_font(check_update, font);

    let agreement = create_button(hwnd, "用户协议", ID_USER_AGREEMENT, 526, 325, 92, 30)?;
    set_font(agreement, font);
    let privacy = create_button(hwnd, "隐私策略", ID_PRIVACY_POLICY, 638, 325, 92, 30)?;
    set_font(privacy, font);

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
    let hwnd = create_control_ex(
        hwnd,
        "EDIT",
        "",
        0,
        x,
        y,
        width,
        height,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE((ES_READONLY | ES_AUTOHSCROLL) as u32),
        WS_EX_CLIENTEDGE,
    )?;
    set_edit_margins(hwnd, 8);
    Ok(hwnd)
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
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32),
    )
}

unsafe fn create_group(
    hwnd: HWND,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<HWND> {
    create_control(
        hwnd,
        "BUTTON",
        text,
        0,
        x,
        y,
        width,
        height,
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_GROUPBOX as u32),
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
    create_control_ex(
        parent,
        class,
        text,
        id,
        x,
        y,
        width,
        height,
        style,
        WINDOW_EX_STYLE::default(),
    )
}

unsafe fn create_control_ex(
    parent: HWND,
    class: &str,
    text: &str,
    id: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    style: WINDOW_STYLE,
    ex_style: WINDOW_EX_STYLE,
) -> Result<HWND> {
    let class = to_wide(class);
    let text = to_wide(text);
    let module = GetModuleHandleW(None)?;
    let instance = HINSTANCE(module.0);
    let hwnd = CreateWindowExW(
        ex_style,
        PCWSTR(class.as_ptr()),
        PCWSTR(text.as_ptr()),
        style,
        x,
        y,
        width,
        height,
        parent,
        HMENU(id as usize as *mut c_void),
        instance,
        None,
    )?;
    Ok(hwnd)
}

unsafe fn create_font(height: i32, weight: i32, face_name: &str) -> HFONT {
    let face_name = to_wide(face_name);
    CreateFontW(
        height,
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32,
        CLIP_DEFAULT_PRECIS.0 as u32,
        CLEARTYPE_QUALITY.0 as u32,
        (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
        PCWSTR(face_name.as_ptr()),
    )
}

unsafe fn set_font(hwnd: HWND, font: HFONT) {
    let _ = SendMessageW(hwnd, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
}

unsafe fn set_edit_margins(hwnd: HWND, margin: i32) {
    let packed = ((margin as u32) & 0xffff) | (((margin as u32) & 0xffff) << 16);
    let _ = SendMessageW(
        hwnd,
        EM_SETMARGINS,
        WPARAM((EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize),
        LPARAM(packed as isize),
    );
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
        MessageBoxW(hwnd, PCWSTR(text.as_ptr()), PCWSTR(title.as_ptr()), flags);
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
