#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use chrono::Timelike;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::Manager;

// Global state
static LOCK_ACTIVE: AtomicBool = AtomicBool::new(false);
static LOCK_END_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

const PROXY_PORT: u16 = 31415;
const EXTENSION_WS_PORT: u16 = 8766;

#[cfg(windows)]
static SAVED_PROXY: Mutex<Option<(u32, String)>> = Mutex::new(None);

// ============================================================================
// DATA STRUCTURES
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    pub id: String,
    pub name: String,
    pub typical_time: String, // "HH:MM" 24h format
    #[serde(default)]
    pub duration_minutes: u32,
    #[serde(default = "default_lock_minutes")]
    pub minimum_lock_minutes: u32,
    #[serde(default)]
    pub allowed_apps: Vec<String>,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
}

fn default_lock_minutes() -> u32 {
    10
}

// ============================================================================
// ACTIVITY MANAGEMENT
// ============================================================================

fn activities_path() -> Result<std::path::PathBuf, String> {
    let appdata = std::env::var("APPDATA").map_err(|_| "APPDATA not set")?;
    Ok(std::path::PathBuf::from(appdata)
        .join("prodblock")
        .join("activities.json"))
}

#[tauri::command]
fn get_activities() -> Result<Vec<Activity>, String> {
    let path = activities_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let activities: Vec<Activity> = serde_json::from_str(&data).map_err(|e| e.to_string())?;
    Ok(activities)
}

#[tauri::command]
fn save_activities(activities: Vec<Activity>) -> Result<(), String> {
    let path = activities_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let data = serde_json::to_string_pretty(&activities).map_err(|e| e.to_string())?;
    std::fs::write(&path, data).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn get_suggested_three() -> Result<Vec<Activity>, String> {
    let activities = get_activities()?;
    if activities.is_empty() {
        return Ok(Vec::new());
    }

    let now = chrono::Local::now();
    let now_mins = now.hour() * 60 + now.minute();

    let mut with_dist: Vec<_> = activities
        .into_iter()
        .map(|a| {
            let (h, m) = parse_time(&a.typical_time).unwrap_or((0, 0));
            let typical_mins = h * 60 + m;
            let mut dist = (typical_mins as i32 - now_mins as i32).abs();
            // Handle midnight wraparound
            if dist > 12 * 60 {
                dist = 24 * 60 - dist;
            }
            (dist, a)
        })
        .collect();

    with_dist.sort_by_key(|(d, _)| *d);
    Ok(with_dist.into_iter().take(3).map(|(_, a)| a).collect())
}

fn parse_time(s: &str) -> Option<(u32, u32)> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let h: u32 = parts[0].trim().parse().ok()?;
    let m: u32 = parts[1].trim().parse().ok()?;
    if h < 24 && m < 60 {
        Some((h, m))
    } else {
        None
    }
}

// ============================================================================
// FOCUS LOCK
// ============================================================================

#[tauri::command]
fn start_lock(
    app: tauri::AppHandle,
    _activity_id: String,
    whitelist: Vec<String>,
    allowed_domains: Vec<String>,
    minimum_lock_minutes: u32,
) -> Result<(), String> {
    use std::sync::atomic::Ordering;

    let end_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis() as u64
        + (minimum_lock_minutes as u64) * 60 * 1000;

    LOCK_END_MS.store(end_ms, Ordering::SeqCst);
    LOCK_ACTIVE.store(true, Ordering::SeqCst);

    // Maximize and focus prodblock window
    if let Some(main_win) = app.get_webview_window("main") {
        let _ = main_win.unminimize();
        let _ = main_win.maximize();
        let _ = main_win.set_focus();
    }

    #[cfg(windows)]
    {
        // Start foreground watcher thread
        let app_handle = app.clone();
        let whitelist_clone = whitelist.clone();
        std::thread::spawn(move || {
            run_foreground_watcher(app_handle, whitelist_clone);
        });

        // Always start WebSocket server for browser extension
        let domains_ws = allowed_domains.clone();
        std::thread::spawn(move || run_extension_ws_server(domains_ws));

        // Start proxy if allowed_domains is non-empty
        if !allowed_domains.is_empty() {
            let proxy_addr = format!("127.0.0.1:{}", PROXY_PORT);
            set_windows_proxy(&proxy_addr)?;
            let domains = allowed_domains.clone();
            std::thread::spawn(move || run_proxy(domains));
        }
    }

    Ok(())
}

#[tauri::command]
fn end_lock() -> Result<(), String> {
    LOCK_ACTIVE.store(false, Ordering::SeqCst);
    LOCK_END_MS.store(0, Ordering::SeqCst);

    #[cfg(windows)]
    let _ = restore_windows_proxy();

    Ok(())
}

#[derive(Serialize)]
struct LockStatus {
    remaining_ms: u64,
    can_finish: bool,
}

#[tauri::command]
fn get_lock_status() -> Result<LockStatus, String> {
    let end_ms = LOCK_END_MS.load(Ordering::SeqCst);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis() as u64;
    let remaining_ms = if end_ms > now_ms { end_ms - now_ms } else { 0 };
    Ok(LockStatus {
        remaining_ms,
        can_finish: remaining_ms == 0,
    })
}

// ============================================================================
// WINDOWS FOREGROUND WATCHER
// ============================================================================

#[cfg(windows)]
fn run_foreground_watcher(app: tauri::AppHandle, whitelist: Vec<String>) {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, ShowWindow, SW_MINIMIZE};

    let our_pid = unsafe { GetCurrentProcessId() };
    let whitelist_lower: Vec<String> = whitelist.iter().map(|s| s.to_lowercase()).collect();

    while LOCK_ACTIVE.load(Ordering::SeqCst) {
        if let Some(main_win) = app.get_webview_window("main") {
            let fg_hwnd = unsafe { GetForegroundWindow() };
            if !fg_hwnd.0.is_null() {
                let fg_pid = get_window_process_id(fg_hwnd);
                if fg_pid != 0 && fg_pid != our_pid {
                    if let Some(exe_path) = get_process_exe_name(fg_pid) {
                        let exe_name = exe_path.to_lowercase();
                        
                        // If whitelist is empty, block ALL apps (except prodblock)
                        // If whitelist has items, allow those apps
                        let allowed = if whitelist_lower.is_empty() {
                            false // Block everything
                        } else {
                            whitelist_lower.iter().any(|w| {
                                exe_name.ends_with(w)
                                    || exe_name.contains(&format!("\\{}", w))
                                    || exe_name == *w
                            })
                        };

                        if !allowed {
                            let _ = unsafe { ShowWindow(fg_hwnd, SW_MINIMIZE) };
                            let _ = main_win.set_focus();
                        }
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
}

#[cfg(windows)]
fn get_window_process_id(hwnd: windows::Win32::Foundation::HWND) -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    let mut pid: u32 = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    pid
}

#[cfg(windows)]
fn get_process_exe_name(pid: u32) -> Option<String> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()? };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    if unsafe { Process32FirstW(snapshot, &mut entry).is_ok() } {
        loop {
            if entry.th32ProcessID == pid {
                let name = String::from_utf16_lossy(
                    &entry.szExeFile[..entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(260)],
                );
                let _ = unsafe { windows::Win32::Foundation::CloseHandle(snapshot) };
                return Some(name);
            }
            if unsafe { Process32NextW(snapshot, &mut entry).is_err() } {
                break;
            }
        }
    }
    let _ = unsafe { windows::Win32::Foundation::CloseHandle(snapshot) };
    None
}

// ============================================================================
// HTTP PROXY FOR WEBSITE BLOCKING
// ============================================================================

fn domain_allowed(host: &str, allowed: &[String]) -> bool {
    let host = host.to_lowercase();
    let host = host.split(':').next().unwrap_or(&host).trim();
    if host.is_empty() {
        return false;
    }
    for d in allowed {
        let d = d.to_lowercase();
        let d = d.trim();
        if d.is_empty() {
            continue;
        }
        if host == d || host.ends_with(&format!(".{}", d)) {
            return true;
        }
    }
    false
}

fn run_proxy(allowed_domains: Vec<String>) {
    use std::net::TcpListener;

    let Ok(listener) = TcpListener::bind(("127.0.0.1", PROXY_PORT)) else {
        return;
    };
    let _ = listener.set_nonblocking(true);

    while LOCK_ACTIVE.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let allowed = allowed_domains.clone();
                std::thread::spawn(move || handle_proxy_connection(stream, allowed));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            _ => break,
        }
    }
}

fn handle_proxy_connection(mut client: std::net::TcpStream, allowed_domains: Vec<String>) {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let mut buf = [0u8; 4096];
    let n = match client.read(&mut buf) {
        Ok(0) => return,
        Ok(n) => n,
        Err(_) => return,
    };

    let head = match std::str::from_utf8(&buf[..n]) {
        Ok(h) => h,
        Err(_) => return,
    };

    let first_line = head.lines().next().unwrap_or("");
    let host = if first_line.starts_with("CONNECT ") {
        first_line
            .strip_prefix("CONNECT ")
            .and_then(|s| s.split_whitespace().next())
            .unwrap_or("")
    } else {
        head.lines()
            .find(|l| l.to_lowercase().starts_with("host:"))
            .and_then(|l| l.split(':').nth(1))
            .map(str::trim)
            .unwrap_or("")
    };
    let host = host.split(':').next().unwrap_or(host).trim();

    if host.is_empty() {
        let _ = client.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n");
        return;
    }

    if !domain_allowed(host, &allowed_domains) {
        let body = b"<html><body style='background:#0d0d0d;color:#fff;font-family:system-ui;display:flex;align-items:center;justify-content:center;height:100vh;margin:0'><div style='text-align:center'><h1>Blocked by Prodblock</h1><p>This site is not in your activity's allowed list.</p></div></body></html>";
        let _ = client.write_all(
            format!(
                "HTTP/1.1 403 Forbidden\r\nConnection: close\r\nContent-Length: {}\r\nContent-Type: text/html\r\n\r\n",
                body.len()
            )
            .as_bytes(),
        );
        let _ = client.write_all(body);
        return;
    }

    // Handle CONNECT (HTTPS tunneling)
    if first_line.starts_with("CONNECT ") {
        let host_port = first_line
            .strip_prefix("CONNECT ")
            .and_then(|s| s.split_whitespace().next())
            .unwrap_or("");
        let mut parts = host_port.split(':');
        let host = parts.next().unwrap_or("").trim();
        let port: u16 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(443);
        
        let upstream = match TcpStream::connect((host, port)) {
            Ok(s) => s,
            Err(_) => {
                let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\n\r\n");
                return;
            }
        };
        let _ = client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n");

        let mut client_read = match client.try_clone() { Ok(s) => s, Err(_) => return };
        let mut client_write = match client.try_clone() { Ok(s) => s, Err(_) => return };
        let mut up_read = match upstream.try_clone() { Ok(s) => s, Err(_) => return };
        let mut up_write = match upstream.try_clone() { Ok(s) => s, Err(_) => return };

        std::thread::spawn(move || {
            let _ = std::io::copy(&mut client_read, &mut up_write);
        });
        let _ = std::io::copy(&mut up_read, &mut client_write);
    } else {
        // Handle plain HTTP
        let host_header = head
            .lines()
            .find(|l| l.to_lowercase().starts_with("host:"))
            .and_then(|l| l.split_once(':'))
            .map(|(_, v)| v.trim())
            .unwrap_or("");
        let port: u16 = host_header.split(':').nth(1).and_then(|p| p.parse().ok()).unwrap_or(80);
        let host = host_header.split(':').next().unwrap_or(host_header).trim();
        
        let mut upstream = match TcpStream::connect((host, port)) {
            Ok(s) => s,
            Err(_) => {
                let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\n\r\n");
                return;
            }
        };
        let _ = upstream.write_all(&buf[..n]);
        let _ = std::io::copy(&mut upstream, &mut client);
    }
}

// ============================================================================
// WEBSOCKET SERVER FOR BROWSER EXTENSION
// ============================================================================

fn run_extension_ws_server(allowed_domains: Vec<String>) {
    use std::io::ErrorKind;
    use std::net::TcpListener;
    use tungstenite::Message;

    let Ok(listener) = TcpListener::bind(("127.0.0.1", EXTENSION_WS_PORT)) else {
        return;
    };
    let _ = listener.set_nonblocking(true);

    while LOCK_ACTIVE.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let domains = allowed_domains.clone();
                std::thread::spawn(move || {
                    let mut ws = match tungstenite::accept(stream) {
                        Ok(w) => w,
                        Err(_) => return,
                    };
                    while LOCK_ACTIVE.load(Ordering::SeqCst) {
                        let msg = serde_json::json!({
                            "lockActive": true,
                            "allowedDomains": domains
                        });
                        if ws.send(Message::Text(msg.to_string())).is_err() {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_secs(1));
                    }
                    let _ = ws.send(Message::Text(r#"{"lockActive":false}"#.to_string()));
                });
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {}
            _ => {}
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

// ============================================================================
// WINDOWS PROXY SETTINGS
// ============================================================================

#[cfg(windows)]
fn set_windows_proxy(host_port: &str) -> Result<(), String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
    use winreg::RegKey;

    let settings = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
            KEY_READ | KEY_SET_VALUE,
        )
        .map_err(|e| e.to_string())?;

    let prev_enable: u32 = settings.get_value("ProxyEnable").unwrap_or(0);
    let prev_server: String = settings.get_value("ProxyServer").unwrap_or_default();
    *SAVED_PROXY.lock().map_err(|e| e.to_string())? = Some((prev_enable, prev_server));

    settings.set_value("ProxyEnable", &1u32).map_err(|e| e.to_string())?;
    settings.set_value("ProxyServer", &host_port.to_string()).map_err(|e| e.to_string())?;

    refresh_wininet_proxy();
    Ok(())
}

#[cfg(windows)]
fn restore_windows_proxy() -> Result<(), String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::RegKey;

    let saved = SAVED_PROXY.lock().map_err(|e| e.to_string())?.take();
    let Some((prev_enable, prev_server)) = saved else {
        return Ok(());
    };

    let settings = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
            KEY_SET_VALUE,
        )
        .map_err(|e| e.to_string())?;

    settings.set_value("ProxyEnable", &prev_enable).map_err(|e| e.to_string())?;
    settings.set_value("ProxyServer", &prev_server).map_err(|e| e.to_string())?;

    refresh_wininet_proxy();
    Ok(())
}

#[cfg(windows)]
fn refresh_wininet_proxy() {
    use windows::Win32::Networking::WinInet::{
        InternetSetOptionW, INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED,
    };
    unsafe {
        let _ = InternetSetOptionW(None, INTERNET_OPTION_SETTINGS_CHANGED, None, 0);
        let _ = InternetSetOptionW(None, INTERNET_OPTION_REFRESH, None, 0);
    }
}

// ============================================================================
// RUN AT STARTUP
// ============================================================================

#[tauri::command]
fn set_run_at_startup(enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;

        let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe_path_str = exe_path.to_string_lossy();
        let run = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(
                "Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                winreg::enums::KEY_SET_VALUE,
            )
            .map_err(|e| e.to_string())?;

        if enabled {
            run.set_value("prodblock", &exe_path_str.to_string())
                .map_err(|e| e.to_string())?;
        } else {
            let _ = run.delete_value("prodblock");
        }
    }
    #[cfg(not(windows))]
    let _ = enabled;
    Ok(())
}

#[tauri::command]
fn get_run_at_startup() -> Result<bool, String> {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;

        let run = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(
                "Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                winreg::enums::KEY_READ,
            )
            .map_err(|e| e.to_string())?;
        return Ok(run.get_value::<String, _>("prodblock").is_ok());
    }
    #[cfg(not(windows))]
    Ok(false)
}

// ============================================================================
// TAURI ENTRY POINT
// ============================================================================

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_activities,
            save_activities,
            get_suggested_three,
            start_lock,
            end_lock,
            get_lock_status,
            set_run_at_startup,
            get_run_at_startup,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
