use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    thread,
};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use super::{
    errors::ControlError,
    router,
    types::{ToolInvokeRequest, ToolInvokeResponse},
    ControlServiceState, CONTROL_PORT_RANGE,
};

pub fn start(app: AppHandle) -> Result<String, String> {
    let (listener, base_url) = bind_loopback()?;
    let control_state: tauri::State<'_, ControlServiceState> = app.state();
    control_state.set_bind_address(base_url.clone())?;

    thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let handle = app.clone();
                    thread::spawn(move || {
                        let _ = handle_connection(&handle, stream);
                    });
                }
                Err(error) => {
                    eprintln!("PenguinPal control service accept error: {error}");
                }
            }
        }
    });

    Ok(base_url)
}

fn bind_loopback() -> Result<(TcpListener, String), String> {
    for port in CONTROL_PORT_RANGE {
        let address = format!("127.0.0.1:{port}");
        if let Ok(listener) = TcpListener::bind(&address) {
            return Ok((listener, format!("http://{address}")));
        }
    }

    Err("无法为本地控制服务绑定 127.0.0.1 端口。".to_string())
}

fn handle_connection(app: &AppHandle, mut stream: TcpStream) -> Result<(), String> {
    let (method, path, body) = read_request(&mut stream)?;
    let (status, payload) = route(app, &method, &path, body);
    write_json_response(&mut stream, status, &payload)
}

fn read_request(stream: &mut TcpStream) -> Result<(String, String, Vec<u8>), String> {
    let cloned = stream.try_clone().map_err(|error| error.to_string())?;
    let mut reader = BufReader::new(cloned);

    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|error| error.to_string())?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| "缺少 HTTP method".to_string())?
        .to_string();
    let path = parts
        .next()
        .ok_or_else(|| "缺少 HTTP path".to_string())?
        .to_string();

    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|error| error.to_string())?;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }

        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse::<usize>().unwrap_or_default();
            }
        }
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader
            .read_exact(&mut body)
            .map_err(|error| error.to_string())?;
    }

    Ok((method, path, body))
}

fn route(app: &AppHandle, method: &str, path: &str, body: Vec<u8>) -> (u16, Value) {
    if method.eq_ignore_ascii_case("OPTIONS") {
        return (204, json!({}));
    }

    match (method, path) {
        ("GET", "/healthz") => match router::service_status(app) {
            Ok(status) => (200, json!(status)),
            Err(error) => (500, error_payload(error)),
        },
        ("GET", "/v1/tools") => (200, json!(router::list_tools())),
        ("GET", "/v1/pending") => match router::list_pending(app) {
            Ok(items) => (200, json!(items)),
            Err(error) => (500, error_payload(error)),
        },
        ("POST", "/v1/tools/invoke") => match parse_json::<ToolInvokeRequest>(&body) {
            Ok(request) => match router::invoke(app, request) {
                Ok(response) => (200, json!(response)),
                Err(error) => (400, error_payload(error)),
            },
            Err(error) => (400, error_payload(ControlError::invalid_argument(error))),
        },
        _ => match pending_route(app, method, path) {
            Some(result) => result,
            None => (
                404,
                error_payload(ControlError::not_found(
                    "route_not_found",
                    "未找到控制服务路由。",
                )),
            ),
        },
    }
}

fn pending_route(app: &AppHandle, method: &str, path: &str) -> Option<(u16, Value)> {
    let segments: Vec<&str> = path.split('/').collect();
    if segments.len() != 5 || segments[1] != "v1" || segments[2] != "pending" {
        return None;
    }

    let pending_id = segments[3];
    let action = segments[4];
    let response = match (method, action) {
        ("POST", "confirm") => router::confirm(app, pending_id),
        ("POST", "cancel") => router::cancel(app, pending_id),
        _ => return None,
    };

    Some(match response {
        Ok(payload) => (200, json!(payload)),
        Err(error) => (400, error_payload(error)),
    })
}

fn parse_json<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, String> {
    serde_json::from_slice(body).map_err(|error| format!("请求 JSON 无法解析：{error}"))
}

fn write_json_response(stream: &mut TcpStream, status: u16, payload: &Value) -> Result<(), String> {
    let status_text = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    };

    let body = if status == 204 {
        Vec::new()
    } else {
        serde_json::to_vec(payload).map_err(|error| error.to_string())?
    };

    let headers = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json; charset=utf-8\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status,
        status_text,
        body.len()
    );

    stream
        .write_all(headers.as_bytes())
        .map_err(|error| error.to_string())?;
    if !body.is_empty() {
        stream.write_all(&body).map_err(|error| error.to_string())?;
    }
    stream.flush().map_err(|error| error.to_string())
}

fn error_payload(error: ControlError) -> Value {
    json!(ToolInvokeResponse {
        status: "error".to_string(),
        result: None,
        message: Some(error.to_string()),
        pending_request: None,
        error: Some(error.payload()),
    })
}
