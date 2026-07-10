use serde::Deserialize;
use serde_json::json;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};

use crate::diagnostics;
use crate::hardware::collect_machine_info;
use crate::state::AppState;

const DEFAULT_PORT: u16 = 18888;
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
struct HttpRequest {
    method: String,
    raw_path: String,
    path: String,
    body: Vec<u8>,
}

#[derive(Debug, Deserialize)]
struct SetAuthRequest {
    authorized: bool,
}

pub fn start_server(state: Arc<Mutex<AppState>>) {
    let endpoints = [
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), DEFAULT_PORT),
        SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), DEFAULT_PORT),
    ];

    for endpoint in endpoints {
        let state = Arc::clone(&state);
        thread::spawn(move || {
            diagnostics::append_log(format!("localhost API 准备绑定: endpoint={}", endpoint));
            if let Err(error) = run_server(state, endpoint) {
                diagnostics::append_log(format!("localhost API 启动失败: {}: {}", endpoint, error));
            }
        });
    }
}

fn run_server(state: Arc<Mutex<AppState>>, endpoint: SocketAddr) -> std::io::Result<()> {
    let listener = TcpListener::bind(endpoint)?;
    diagnostics::append_log(format!("localhost API 已启动: http://{}", endpoint));

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state = Arc::clone(&state);
                thread::spawn(move || {
                    if let Err(error) = handle_connection(stream, state) {
                        diagnostics::append_log(format!("HTTP 请求处理失败: {}", error));
                    }
                });
            }
            Err(error) => diagnostics::append_log(format!("HTTP 连接失败: {}", error)),
        }
    }

    Ok(())
}

fn handle_connection(mut stream: TcpStream, state: Arc<Mutex<AppState>>) -> std::io::Result<()> {
    let request_id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let started = Instant::now();
    let peer = stream
        .peer_addr()
        .map(|addr| addr.to_string())
        .unwrap_or_else(|error| format!("unknown: {}", error));
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(error) => {
            diagnostics::append_log(format!(
                "HTTP 请求读取失败: request_id={}, peer={}, error={}",
                request_id, peer, error
            ));
            write_response(
                &mut stream,
                400,
                "Bad Request",
                "text/plain; charset=utf-8",
                error.to_string().as_bytes(),
            )?;
            diagnostics::append_log(format!(
                "HTTP 响应: request_id={}, peer={}, status=400 Bad Request, body_bytes={}, elapsed_ms={}",
                request_id,
                peer,
                error.to_string().len(),
                started.elapsed().as_millis()
            ));
            return Ok(());
        }
    };

    if request.method == "OPTIONS" {
        write_response(&mut stream, 204, "No Content", "text/plain", b"")?;
        diagnostics::append_log(format!(
            "HTTP 响应: request_id={}, peer={}, method=OPTIONS, path={}, status=204, elapsed_ms={}",
            request_id,
            peer,
            request.path,
            started.elapsed().as_millis()
        ));
        return Ok(());
    }

    diagnostics::append_log(format!(
        "HTTP 请求: request_id={}, peer={}, method={}, raw_path={}, path={}, body_bytes={}",
        request_id,
        peer,
        request.method,
        request.raw_path,
        request.path,
        request.body.len()
    ));
    let response = route_request(request, state);
    let body_bytes = write_json(&mut stream, response.0, response.1, &response.2)?;
    diagnostics::append_log(format!(
        "HTTP 响应: request_id={}, peer={}, status={} {}, body_bytes={}, elapsed_ms={}",
        request_id,
        peer,
        response.0,
        response.1,
        body_bytes,
        started.elapsed().as_millis()
    ));
    Ok(())
}

fn route_request(
    request: HttpRequest,
    state: Arc<Mutex<AppState>>,
) -> (u16, &'static str, serde_json::Value) {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/health") => (
            200,
            "OK",
            json!({
                "status": "ok",
                "version": env!("CARGO_PKG_VERSION"),
                "log_path": diagnostics::log_path().display().to_string(),
            }),
        ),
        ("GET", "/api/auth-status") => {
            let state = state.lock().expect("state lock poisoned");
            diagnostics::append_log(format!(
                "HTTP auth-status: authorized={}, has_machine_info={}",
                state.authorized,
                state.machine_info.is_some()
            ));
            (200, "OK", json!({ "authorized": state.authorized }))
        }
        ("POST", "/api/set-auth") => {
            match serde_json::from_slice::<SetAuthRequest>(&request.body) {
                Ok(payload) => {
                    diagnostics::append_log(format!(
                        "HTTP set-auth: requested_authorized={}",
                        payload.authorized
                    ));
                    let mut state = state.lock().expect("state lock poisoned");
                    state.set_authorized(payload.authorized);
                    (
                        200,
                        "OK",
                        json!({
                            "success": true,
                            "authorized": state.authorized,
                        }),
                    )
                }
                Err(error) => {
                    diagnostics::append_log(format!("HTTP set-auth 参数错误: {}", error));
                    (
                        400,
                        "Bad Request",
                        json!({
                            "success": false,
                            "message": format!("JSON 参数错误: {}", error),
                        }),
                    )
                }
            }
        }
        ("GET", "/api/machine-code") => {
            let (authorized, cached_info) = {
                let state = state.lock().expect("state lock poisoned");
                (state.authorized, state.machine_info.clone())
            };
            diagnostics::append_log(format!(
                "HTTP machine-code: authorized={}, cache_hit={}",
                authorized,
                cached_info.is_some()
            ));

            if !authorized {
                return (
                    403,
                    "Forbidden",
                    json!({
                        "success": false,
                        "authorized": false,
                        "message": "未开启授权，请先在机器码工具中开启授权",
                    }),
                );
            }

            let info = match cached_info {
                Some(info) => info,
                None => {
                    diagnostics::append_log("HTTP machine-code: 缓存为空，开始采集机器码");
                    let info = collect_machine_info();
                    let mut state = state.lock().expect("state lock poisoned");
                    state.set_machine_info(info.clone());
                    info
                }
            };
            let mut value = info.simple_json();
            if let Some(object) = value.as_object_mut() {
                object.insert("success".to_string(), json!(true));
                object.insert("authorized".to_string(), json!(true));
            }
            (200, "OK", value)
        }
        _ => (
            404,
            "Not Found",
            json!({
                "success": false,
                "message": "接口不存在",
            }),
        ),
    }
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<HttpRequest> {
    let mut buffer = Vec::new();
    let mut temp = [0u8; 1024];
    let header_end;

    loop {
        let n = stream.read(&mut temp)?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "连接提前关闭",
            ));
        }
        buffer.extend_from_slice(&temp[..n]);
        if let Some(pos) = find_header_end(&buffer) {
            header_end = pos;
            break;
        }
        if buffer.len() > 64 * 1024 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "HTTP 头过大",
            ));
        }
    }

    let headers = String::from_utf8_lossy(&buffer[..header_end]);
    let mut lines = headers.lines();
    let request_line = lines
        .next()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "缺少请求行"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let raw_path = parts.next().unwrap_or("/").to_string();
    let path = raw_path.split('?').next().unwrap_or("/").to_string();

    let content_length = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);

    let body_start = header_end + 4;
    let mut body = buffer[body_start..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut temp)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&temp[..n]);
    }
    body.truncate(content_length);

    Ok(HttpRequest {
        method,
        raw_path,
        path,
        body,
    })
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn write_json(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    value: &serde_json::Value,
) -> std::io::Result<usize> {
    let body = serde_json::to_vec_pretty(value).unwrap_or_else(|_| b"{}".to_vec());
    write_response(
        stream,
        status,
        reason,
        "application/json; charset=utf-8",
        &body,
    )?;
    Ok(body.len())
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let headers = format!(
        "HTTP/1.1 {} {}\r\n\
         Content-Type: {}\r\n\
         Content-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: *\r\n\
         Access-Control-Allow-Private-Network: true\r\n\
         Connection: close\r\n\
         \r\n",
        status,
        reason,
        content_type,
        body.len()
    );
    stream.write_all(headers.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}
