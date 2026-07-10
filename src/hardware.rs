use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::process::Command;
use std::time::Instant;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::diagnostics;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldResult {
    pub label: String,
    pub value: String,
    pub ok: bool,
    pub source: String,
    pub error: String,
}

impl FieldResult {
    fn ok(label: &str, value: String, source: &str) -> Self {
        Self {
            label: label.to_string(),
            value,
            ok: true,
            source: source.to_string(),
            error: String::new(),
        }
    }

    fn fail(label: &str, source: &str, error: impl Into<String>) -> Self {
        Self {
            label: label.to_string(),
            value: "————".to_string(),
            ok: false,
            source: source.to_string(),
            error: error.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineInfo {
    pub mac: FieldResult,
    pub motherboard: FieldResult,
    pub cpu: FieldResult,
    pub disk: FieldResult,
    pub system_uuid: FieldResult,
    pub bios: FieldResult,
    pub computer_name: FieldResult,
    pub machine_id: String,
    pub confidence: String,
    pub version: String,
}

impl MachineInfo {
    pub fn simple_json(&self) -> serde_json::Value {
        serde_json::json!({
            "mac": self.mac.value,
            "motherboard": self.motherboard.value,
            "cpu": self.cpu.value,
            "disk": self.disk.value,
            "system_uuid": self.system_uuid.value,
            "bios": self.bios.value,
            "computer_name": self.computer_name.value,
            "machine_id": self.machine_id,
            "confidence": self.confidence,
            "version": self.version,
            "details": self,
        })
    }
}

pub fn collect_machine_info() -> MachineInfo {
    let started = Instant::now();
    diagnostics::append_log("开始采集机器码");

    let mac = collect_mac();
    let motherboard = collect_baseboard_serial();
    let cpu = collect_cpu_id();
    let disk = collect_disk_serial();
    let system_uuid = collect_system_uuid();
    let bios = collect_bios_serial();
    let computer_name = collect_computer_name();

    let machine_id = generate_machine_id([
        &mac,
        &motherboard,
        &cpu,
        &disk,
        &system_uuid,
        &bios,
        &computer_name,
    ]);
    let confidence = confidence(&mac, &motherboard, &cpu, &disk, &system_uuid);

    let info = MachineInfo {
        mac,
        motherboard,
        cpu,
        disk,
        system_uuid,
        bios,
        computer_name,
        machine_id,
        confidence,
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    diagnostics::append_log(format!(
        "机器码采集完成: confidence={}, machine_id={}, elapsed_ms={}",
        info.confidence,
        info.machine_id,
        started.elapsed().as_millis()
    ));
    info
}

fn confidence(
    mac: &FieldResult,
    motherboard: &FieldResult,
    cpu: &FieldResult,
    disk: &FieldResult,
    system_uuid: &FieldResult,
) -> String {
    let strong_count = [motherboard, cpu, disk, system_uuid]
        .iter()
        .filter(|field| field.ok)
        .count();

    match (mac.ok, strong_count) {
        (true, 3..) => "high",
        (true, 1..) => "medium",
        (true, 0) => "low",
        (false, 2..) => "medium",
        (false, 1) => "low",
        _ => "invalid",
    }
    .to_string()
}

fn generate_machine_id<const N: usize>(fields: [&FieldResult; N]) -> String {
    let mut components = Vec::new();
    for field in fields {
        if field.ok {
            components.push(format!(
                "{}={}",
                field.label,
                normalize_for_hash(&field.value)
            ));
        }
    }

    if components.is_empty() {
        return "————".to_string();
    }

    components.sort();
    let joined = components.join("|");
    let digest = Sha256::digest(joined.as_bytes());
    format!("sha256:{}", hex::encode(digest))
}

fn normalize_for_hash(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '-' && *ch != ':')
        .collect::<String>()
        .to_uppercase()
}

fn valid_hardware_value(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_matches('\0').trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    let invalids = [
        "to be filled by o.e.m.",
        "to be filled by oem",
        "default string",
        "default",
        "none",
        "unknown",
        "not specified",
        "not available",
        "system serial number",
        "serial number",
        "0",
        "00000000",
        "0000000000000000",
        "ffffffff",
        "ffffffffffffffff",
    ];

    if invalids.iter().any(|item| lower == *item) {
        return None;
    }
    if lower.chars().all(|ch| ch == '0' || ch == 'f' || ch == '-') {
        return None;
    }

    Some(trimmed.to_string())
}

fn first_valid(label: &str, attempts: &[(&str, Result<String>)]) -> FieldResult {
    let mut errors = Vec::new();
    for (source, result) in attempts {
        match result {
            Ok(value) => {
                if let Some(valid) = valid_hardware_value(value) {
                    diagnostics::append_log(format!(
                        "{} 获取成功: source={}, value={}",
                        label,
                        source,
                        mask_hardware_value(&valid)
                    ));
                    return FieldResult::ok(label, valid, source);
                }
                let invalid = value.trim();
                diagnostics::append_log(format!(
                    "{} 来源返回无效值: source={}, value={}",
                    label,
                    source,
                    mask_hardware_value(invalid)
                ));
                errors.push(format!(
                    "{} 返回无效值: {}",
                    source,
                    mask_hardware_value(invalid)
                ));
            }
            Err(error) => {
                diagnostics::append_log(format!(
                    "{} 来源失败: source={}, error={}",
                    label, source, error
                ));
                errors.push(format!("{} 失败: {}", source, error));
            }
        }
    }

    let error = errors.join("; ");
    diagnostics::append_log(format!("{} 获取失败: {}", label, error));
    FieldResult::fail(label, "multi-source", error)
}

fn timed_attempt(
    label: &str,
    source: &'static str,
    f: impl FnOnce() -> Result<String>,
) -> Result<String> {
    let started = Instant::now();
    diagnostics::append_log(format!("{} 来源开始: source={}", label, source));
    let result = f();
    match &result {
        Ok(value) => diagnostics::append_log(format!(
            "{} 来源完成: source={}, ok=true, value={}, elapsed_ms={}",
            label,
            source,
            mask_hardware_value(value),
            started.elapsed().as_millis()
        )),
        Err(error) => diagnostics::append_log(format!(
            "{} 来源完成: source={}, ok=false, error={}, elapsed_ms={}",
            label,
            source,
            error,
            started.elapsed().as_millis()
        )),
    }
    result
}

fn mask_hardware_value(value: &str) -> String {
    let trimmed = value.trim();
    let char_count = trimmed.chars().count();
    if char_count <= 8 {
        return trimmed.to_string();
    }

    let prefix: String = trimmed.chars().take(4).collect();
    let suffix: String = trimmed
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{}***{}(len={})", prefix, suffix, char_count)
}

fn collect_mac() -> FieldResult {
    first_valid(
        "mac",
        &[(
            "ip-helper",
            timed_attempt("mac", "ip-helper", native_mac_address),
        )],
    )
}

fn collect_baseboard_serial() -> FieldResult {
    first_valid(
        "motherboard",
        &[
            (
                "wmi:Win32_BaseBoard.SerialNumber",
                timed_attempt("motherboard", "wmi:Win32_BaseBoard.SerialNumber", || {
                    wmi_string("Win32_BaseBoard", "SerialNumber")
                }),
            ),
            (
                "smbios:baseboard.serial",
                timed_attempt("motherboard", "smbios:baseboard.serial", || {
                    smbios_string(2, 4)
                }),
            ),
            (
                "wmic:baseboard",
                timed_attempt("motherboard", "wmic:baseboard", || {
                    wmic_value(
                        &["baseboard", "get", "serialnumber", "/value"],
                        "SerialNumber",
                    )
                }),
            ),
        ],
    )
}

fn collect_cpu_id() -> FieldResult {
    first_valid(
        "cpu",
        &[
            (
                "wmi:Win32_Processor.ProcessorId",
                timed_attempt("cpu", "wmi:Win32_Processor.ProcessorId", || {
                    wmi_string("Win32_Processor", "ProcessorId")
                }),
            ),
            (
                "wmic:cpu",
                timed_attempt("cpu", "wmic:cpu", || {
                    wmic_value(&["cpu", "get", "processorid", "/value"], "ProcessorId")
                }),
            ),
        ],
    )
}

fn collect_disk_serial() -> FieldResult {
    first_valid(
        "disk",
        &[
            (
                "wmi:Win32_DiskDrive.SerialNumber",
                timed_attempt("disk", "wmi:Win32_DiskDrive.SerialNumber", || {
                    wmi_string("Win32_DiskDrive", "SerialNumber")
                }),
            ),
            (
                "device-io-control:PhysicalDrive",
                timed_attempt("disk", "device-io-control:PhysicalDrive", || {
                    device_io_disk_serial()
                }),
            ),
            (
                "wmic:diskdrive",
                timed_attempt("disk", "wmic:diskdrive", || {
                    wmic_value(
                        &["diskdrive", "get", "serialnumber", "/value"],
                        "SerialNumber",
                    )
                }),
            ),
        ],
    )
}

fn collect_system_uuid() -> FieldResult {
    first_valid(
        "system_uuid",
        &[
            (
                "wmi:Win32_ComputerSystemProduct.UUID",
                timed_attempt(
                    "system_uuid",
                    "wmi:Win32_ComputerSystemProduct.UUID",
                    || wmi_string("Win32_ComputerSystemProduct", "UUID"),
                ),
            ),
            (
                "wmic:csproduct",
                timed_attempt("system_uuid", "wmic:csproduct", || {
                    wmic_value(&["csproduct", "get", "uuid", "/value"], "UUID")
                }),
            ),
        ],
    )
}

fn collect_bios_serial() -> FieldResult {
    first_valid(
        "bios",
        &[
            (
                "wmi:Win32_BIOS.SerialNumber",
                timed_attempt("bios", "wmi:Win32_BIOS.SerialNumber", || {
                    wmi_string("Win32_BIOS", "SerialNumber")
                }),
            ),
            (
                "smbios:bios.serial",
                timed_attempt("bios", "smbios:bios.serial", || smbios_string(0, 7)),
            ),
            (
                "wmic:bios",
                timed_attempt("bios", "wmic:bios", || {
                    wmic_value(&["bios", "get", "serialnumber", "/value"], "SerialNumber")
                }),
            ),
        ],
    )
}

fn collect_computer_name() -> FieldResult {
    let name = timed_attempt("computer_name", "env:COMPUTERNAME", || {
        std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .map_err(|e| anyhow!("读取环境变量失败: {}", e))
    });
    first_valid("computer_name", &[("env:COMPUTERNAME", name)])
}

#[cfg(windows)]
fn native_mac_address() -> Result<String> {
    use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR};
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
        GAA_FLAG_SKIP_MULTICAST, IF_TYPE_ETHERNET_CSMACD, IF_TYPE_IEEE80211,
        IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::Networking::WinSock::AF_UNSPEC;

    unsafe {
        let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
        let mut size: u32 = 16 * 1024;
        let mut buffer = vec![0u8; size as usize];

        let mut ret = GetAdaptersAddresses(
            AF_UNSPEC.0 as u32,
            flags,
            None,
            Some(buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
            &mut size,
        );

        if ret == ERROR_BUFFER_OVERFLOW.0 {
            buffer.resize(size as usize, 0);
            ret = GetAdaptersAddresses(
                AF_UNSPEC.0 as u32,
                flags,
                None,
                Some(buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
                &mut size,
            );
        }

        if ret != NO_ERROR.0 {
            return Err(anyhow!("GetAdaptersAddresses 返回错误码 {}", ret));
        }

        let mut current = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        while !current.is_null() {
            let adapter = &*current;
            let if_type = adapter.IfType;
            let physical_len = adapter.PhysicalAddressLength as usize;
            if physical_len == 6
                && (if_type == IF_TYPE_ETHERNET_CSMACD || if_type == IF_TYPE_IEEE80211)
            {
                let bytes = &adapter.PhysicalAddress[..physical_len];
                if bytes.iter().any(|b| *b != 0) {
                    return Ok(bytes
                        .iter()
                        .map(|b| format!("{:02X}", b))
                        .collect::<Vec<_>>()
                        .join(":"));
                }
            }
            current = adapter.Next;
        }
    }

    Err(anyhow!("未找到有效物理网卡 MAC"))
}

#[cfg(not(windows))]
fn native_mac_address() -> Result<String> {
    Err(anyhow!("当前平台未实现原生 MAC 采集"))
}

#[cfg(windows)]
fn smbios_string(struct_type: u8, string_index_offset: usize) -> Result<String> {
    use windows::Win32::System::SystemInformation::{GetSystemFirmwareTable, RSMB};

    unsafe {
        let size = GetSystemFirmwareTable(RSMB, 0, None);
        if size == 0 {
            return Err(anyhow!("GetSystemFirmwareTable(RSMB) 返回 0"));
        }

        let mut buffer = vec![0u8; size as usize];
        let read = GetSystemFirmwareTable(RSMB, 0, Some(&mut buffer));
        if read == 0 {
            return Err(anyhow!("读取 SMBIOS 表失败"));
        }

        parse_smbios_string(&buffer, struct_type, string_index_offset)
    }
}

#[cfg(not(windows))]
fn smbios_string(_struct_type: u8, _string_index_offset: usize) -> Result<String> {
    Err(anyhow!("当前平台未实现 SMBIOS 采集"))
}

fn parse_smbios_string(data: &[u8], struct_type: u8, string_index_offset: usize) -> Result<String> {
    if data.len() < 8 {
        return Err(anyhow!("SMBIOS 数据过短"));
    }

    let mut offset = 8usize; // RawSMBIOSData header
    while offset + 4 <= data.len() {
        let ty = data[offset];
        let len = data[offset + 1] as usize;
        if len < 4 || offset + len > data.len() {
            break;
        }

        let strings_start = offset + len;
        let strings_end = find_smbios_strings_end(data, strings_start)
            .ok_or_else(|| anyhow!("SMBIOS 字符串区域未结束"))?;

        if ty == struct_type {
            if string_index_offset >= len {
                return Err(anyhow!("SMBIOS 字段偏移超出结构长度"));
            }
            let index = data[offset + string_index_offset];
            if index == 0 {
                return Err(anyhow!("SMBIOS 字符串索引为 0"));
            }
            return smbios_get_string(&data[strings_start..strings_end], index);
        }

        offset = strings_end + 2;
    }

    Err(anyhow!("未找到 SMBIOS type {}", struct_type))
}

fn find_smbios_strings_end(data: &[u8], mut offset: usize) -> Option<usize> {
    while offset + 1 < data.len() {
        if data[offset] == 0 && data[offset + 1] == 0 {
            return Some(offset);
        }
        offset += 1;
    }
    None
}

fn smbios_get_string(strings: &[u8], index: u8) -> Result<String> {
    let mut current = 1u8;
    let mut start = 0usize;

    for pos in 0..=strings.len() {
        if pos == strings.len() || strings[pos] == 0 {
            if current == index {
                return Ok(String::from_utf8_lossy(&strings[start..pos])
                    .trim()
                    .to_string());
            }
            current = current.saturating_add(1);
            start = pos + 1;
        }
    }

    Err(anyhow!("SMBIOS 字符串索引 {} 不存在", index))
}

#[cfg(windows)]
fn device_io_disk_serial() -> Result<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    for index in 0..8 {
        let path = format!("\\\\.\\PhysicalDrive{}", index);
        let path_wide = path.encode_utf16().chain(Some(0)).collect::<Vec<_>>();

        unsafe {
            let handle = match CreateFileW(
                PCWSTR(path_wide.as_ptr()),
                GENERIC_READ.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                HANDLE::default(),
            ) {
                Ok(handle) => handle,
                Err(error) => {
                    diagnostics::append_log(format!("打开 {} 失败: {}", path, error));
                    continue;
                }
            };
            diagnostics::append_log(format!("打开 {} 成功", path));

            let result = query_storage_serial(handle);
            let _ = CloseHandle(handle);
            match result {
                Ok(serial) => {
                    diagnostics::append_log(format!(
                        "查询 {} 序列号成功: value={}",
                        path,
                        mask_hardware_value(&serial)
                    ));
                    return Ok(serial);
                }
                Err(error) => {
                    diagnostics::append_log(format!("查询 {} 序列号失败: {}", path, error))
                }
            }
        }
    }

    Err(anyhow!("DeviceIoControl 未读取到有效硬盘序列号"))
}

#[cfg(not(windows))]
fn device_io_disk_serial() -> Result<String> {
    Err(anyhow!("当前平台未实现 DeviceIoControl 磁盘采集"))
}

#[cfg(windows)]
unsafe fn query_storage_serial(handle: windows::Win32::Foundation::HANDLE) -> Result<String> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use windows::Win32::System::Ioctl::{
        PropertyStandardQuery, StorageDeviceProperty, IOCTL_STORAGE_QUERY_PROPERTY,
        STORAGE_DEVICE_DESCRIPTOR, STORAGE_PROPERTY_QUERY,
    };
    use windows::Win32::System::IO::DeviceIoControl;

    let mut query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceProperty,
        QueryType: PropertyStandardQuery,
        AdditionalParameters: [0],
    };
    let mut buffer = vec![0u8; 1024];
    let mut returned = 0u32;

    DeviceIoControl(
        handle,
        IOCTL_STORAGE_QUERY_PROPERTY,
        Some((&mut query as *mut STORAGE_PROPERTY_QUERY).cast::<c_void>()),
        size_of::<STORAGE_PROPERTY_QUERY>() as u32,
        Some(buffer.as_mut_ptr().cast::<c_void>()),
        buffer.len() as u32,
        Some(&mut returned),
        None,
    )
    .map_err(|e| anyhow!("IOCTL_STORAGE_QUERY_PROPERTY 失败: {}", e))?;

    if returned < size_of::<STORAGE_DEVICE_DESCRIPTOR>() as u32 {
        return Err(anyhow!("STORAGE_DEVICE_DESCRIPTOR 返回长度过短"));
    }

    let descriptor = &*(buffer.as_ptr() as *const STORAGE_DEVICE_DESCRIPTOR);
    let offset = descriptor.SerialNumberOffset as usize;
    if offset == 0 || offset >= buffer.len() {
        return Err(anyhow!("SerialNumberOffset 无效: {}", offset));
    }

    let end = buffer[offset..]
        .iter()
        .position(|byte| *byte == 0)
        .map(|pos| offset + pos)
        .unwrap_or(buffer.len());

    Ok(String::from_utf8_lossy(&buffer[offset..end])
        .trim()
        .to_string())
}

#[cfg(windows)]
fn wmi_string(class: &str, property: &str) -> Result<String> {
    use std::collections::HashMap;
    use wmi::{COMLibrary, Variant, WMIConnection};

    #[derive(Debug, Deserialize)]
    struct Row {
        #[serde(flatten)]
        values: HashMap<String, Variant>,
    }

    let com = COMLibrary::new().map_err(|e| anyhow!("初始化 COM/WMI 失败: {}", e))?;
    let wmi = WMIConnection::new(com.into()).map_err(|e| anyhow!("连接 WMI 失败: {}", e))?;
    let query = format!("SELECT {} FROM {}", property, class);
    diagnostics::append_log(format!("WMI 查询开始: query={}", query));
    let rows: Vec<Row> = wmi
        .raw_query(&query)
        .map_err(|e| anyhow!("执行 WMI 查询失败 [{}]: {}", query, e))?;
    diagnostics::append_log(format!(
        "WMI 查询完成: query={}, rows={}",
        query,
        rows.len()
    ));

    for row in rows {
        if let Some(value) = row.values.get(property) {
            if let Some(text) = variant_to_string(value) {
                return Ok(text);
            }
        }
    }
    Err(anyhow!("WMI 查询无结果: {}", query))
}

#[cfg(not(windows))]
fn wmi_string(_class: &str, _property: &str) -> Result<String> {
    Err(anyhow!("当前平台不支持 WMI"))
}

#[cfg(windows)]
fn variant_to_string(value: &wmi::Variant) -> Option<String> {
    match value {
        wmi::Variant::String(text) => Some(text.clone()),
        wmi::Variant::UI1(number) => Some(number.to_string()),
        wmi::Variant::UI2(number) => Some(number.to_string()),
        wmi::Variant::UI4(number) => Some(number.to_string()),
        wmi::Variant::UI8(number) => Some(number.to_string()),
        wmi::Variant::I1(number) => Some(number.to_string()),
        wmi::Variant::I2(number) => Some(number.to_string()),
        wmi::Variant::I4(number) => Some(number.to_string()),
        wmi::Variant::I8(number) => Some(number.to_string()),
        _ => None,
    }
}

fn wmic_value(args: &[&str], key: &str) -> Result<String> {
    let mut command = Command::new("wmic");
    command.args(args);
    hide_child_console_window(&mut command);

    diagnostics::append_log(format!("wmic 命令开始: args={}", args.join(" ")));
    let output = command
        .output()
        .map_err(|e| anyhow!("启动 wmic 失败: {}", e))?;
    diagnostics::append_log(format!(
        "wmic 命令完成: args={}, status={}, stdout_bytes={}, stderr_bytes={}",
        args.join(" "),
        output.status,
        output.stdout.len(),
        output.stderr.len()
    ));

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "wmic 退出码: {}, stderr={}",
            output.status,
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix(&format!("{}=", key)) {
            return Ok(value.trim().to_string());
        }
    }

    let fallback = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.eq_ignore_ascii_case(key))
        .collect::<Vec<_>>()
        .join(" ");
    if fallback.is_empty() {
        Err(anyhow!("wmic 未返回 {}", key))
    } else {
        Ok(fallback)
    }
}

#[cfg(windows)]
fn hide_child_console_window(command: &mut Command) {
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_child_console_window(_command: &mut Command) {}
