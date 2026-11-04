#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{Manager, State};
use tokio::sync::Mutex;
use windows_sys::Win32::Security::Credentials::{
    CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC,
};

const API_BASE: &str = "https://daily-cloudcode-pa.googleapis.com";
const AGY_CRED_TARGET: &str = "gemini:antigravity";
const CREATE_NO_WINDOW: u32 = 0x08000000;
const GWL_STYLE: i32 = -16;
const GWL_EXSTYLE: i32 = -20;
const WS_CHILD: i32 = 0x40000000;
const WS_VISIBLE: i32 = 0x10000000;
const WS_CLIPSIBLINGS: i32 = 0x04000000;
const WS_POPUP: i32 = -2147483648; // 0x80000000
const WS_EX_LAYERED: i32 = 0x00080000;
const WS_EX_TOOLWINDOW: i32 = 0x00000080;
const WS_EX_NOACTIVATE: i32 = 0x08000000;
const LWA_ALPHA: u32 = 0x00000002;
const VK_LBUTTON: i32 = 0x01;
const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
const DWMWA_BORDER_COLOR: u32 = 34;
const DWMWCP_DONOTROUND: u32 = 1;
const DWMWCP_ROUND: u32 = 2;
const DWMWA_COLOR_NONE: u32 = 0xFFFFFFFE;
const DWMWA_COLOR_DEFAULT: u32 = 0xFFFFFFFF;
const HWND_TOP: isize = 0;
const HWND_TOPMOST: isize = -1;
const SWP_NOSIZE: u32 = 0x0001;
const SWP_NOMOVE: u32 = 0x0002;
const SWP_NOACTIVATE: u32 = 0x0010;
const SWP_FRAMECHANGED: u32 = 0x0020;
const SWP_SHOWWINDOW: u32 = 0x0040;
const MONITOR_DEFAULTTONEAREST: u32 = 0x00000002;
const PILL_WIDTH_LOGICAL: f64 = 124.0;
const PILL_HEIGHT_LOGICAL: f64 = 38.0;
const FULL_WIDTH_LOGICAL: f64 = 368.0;
const FULL_HEIGHT_LOGICAL: f64 = 440.0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct WinPoint {
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct WinRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct MonitorInfo {
    cb_size: u32,
    rc_monitor: WinRect,
    rc_work: WinRect,
    dw_flags: u32,
}

#[link(name = "user32")]
extern "system" {
    fn GetWindowLongW(hwnd: isize, index: i32) -> i32;
    fn SetWindowLongW(hwnd: isize, index: i32, new_long: i32) -> i32;
    fn SetLayeredWindowAttributes(hwnd: isize, cr_key: u32, alpha: u8, flags: u32) -> i32;
    fn GetAsyncKeyState(vkey: i32) -> i16;
    fn GetCursorPos(lp_point: *mut WinPoint) -> i32;
    fn GetWindowRect(hwnd: isize, lp_rect: *mut WinRect) -> i32;
    fn FindWindowW(lp_class_name: *const u16, lp_window_name: *const u16) -> isize;
    fn FindWindowExW(
        hwnd_parent: isize,
        hwnd_child_after: isize,
        lpsz_class: *const u16,
        lpsz_window: *const u16,
    ) -> isize;
    fn SetWindowPos(
        hwnd: isize,
        hwnd_insert_after: isize,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        u_flags: u32,
    ) -> i32;
    fn MonitorFromPoint(pt: WinPoint, dw_flags: u32) -> isize;
    fn MonitorFromWindow(hwnd: isize, dw_flags: u32) -> isize;
    fn GetMonitorInfoW(hmonitor: isize, lpmi: *mut MonitorInfo) -> i32;
    fn GetDpiForWindow(hwnd: isize) -> u32;
    fn GetDC(hwnd: isize) -> isize;
    fn ReleaseDC(hwnd: isize, hdc: isize) -> i32;
    fn SetParent(hwnd_child: isize, hwnd_new_parent: isize) -> isize;
    fn GetParent(hwnd: isize) -> isize;
}

#[link(name = "gdi32")]
extern "system" {
    fn GetPixel(hdc: isize, x: i32, y: i32) -> u32;
}

#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(hwnd: isize, attr: u32, pv_attr: *const u32, cb_attr: u32) -> i32;
}

fn apply_hwnd_opacity(hwnd: isize, percent: u8) {
    unsafe {
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
        if percent >= 100 {
            if (ex & WS_EX_LAYERED) != 0 {
                SetWindowLongW(hwnd, GWL_EXSTYLE, ex & !WS_EX_LAYERED);
            }
        } else {
            if (ex & WS_EX_LAYERED) == 0 {
                SetWindowLongW(hwnd, GWL_EXSTYLE, ex | WS_EX_LAYERED);
            }
            let alpha = ((percent as u32 * 255) / 100).clamp(90, 255) as u8;
            SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA);
        }
    }
}

fn apply_hwnd_taskbar_style(hwnd: isize, is_taskbar_pill: bool) {
    unsafe {
        let corner_pref = if is_taskbar_pill {
            DWMWCP_DONOTROUND
        } else {
            DWMWCP_ROUND
        };
        let border_col = if is_taskbar_pill {
            DWMWA_COLOR_NONE
        } else {
            DWMWA_COLOR_DEFAULT
        };
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner_pref as *const u32,
            std::mem::size_of::<u32>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &border_col as *const u32,
            std::mem::size_of::<u32>() as u32,
        );
    }
}

fn sample_taskbar_color(px: i32, py: i32, pill_h: i32) -> Option<String> {
    unsafe {
        let hdc = GetDC(0);
        if hdc == 0 {
            return None;
        }
        let sample_x = (px - 6).max(4);
        let sample_y = py + pill_h / 2;
        let cr = GetPixel(hdc, sample_x, sample_y);
        ReleaseDC(0, hdc);
        if cr == 0xFFFFFFFF {
            return None;
        }
        let r = (cr & 0xFF) as u8;
        let g = ((cr >> 8) & 0xFF) as u8;
        let b = ((cr >> 16) & 0xFF) as u8;
        Some(format!("rgb({}, {}, {})", r, g, b))
    }
}

fn get_hwnd_scale(hwnd: isize) -> f64 {
    unsafe {
        let dpi = GetDpiForWindow(hwnd);
        if dpi == 0 {
            1.0
        } else {
            (dpi as f64 / 96.0).clamp(1.0, 4.0)
        }
    }
}

fn get_work_area_from_hmonitor(hmon: isize) -> WinRect {
    unsafe {
        let mut mi = MonitorInfo {
            cb_size: std::mem::size_of::<MonitorInfo>() as u32,
            rc_monitor: WinRect::default(),
            rc_work: WinRect::default(),
            dw_flags: 0,
        };
        if hmon != 0 && GetMonitorInfoW(hmon, &mut mi) != 0 {
            mi.rc_work
        } else {
            WinRect {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1040,
            }
        }
    }
}

fn get_work_area_for_point(x: i32, y: i32) -> WinRect {
    let hmon = unsafe { MonitorFromPoint(WinPoint { x, y }, MONITOR_DEFAULTTONEAREST) };
    get_work_area_from_hmonitor(hmon)
}

fn get_work_area_for_hwnd(hwnd: isize) -> WinRect {
    let hmon = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    get_work_area_from_hmonitor(hmon)
}

fn set_hwnd_bounds(hwnd: isize, x: i32, y: i32, w: i32, h: i32) {
    unsafe {
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE);
    }
}

fn raise_hwnd_topmost(hwnd: isize) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

fn find_taskbar_hwnd() -> isize {
    let tray_class = to_wide("Shell_TrayWnd");
    unsafe { FindWindowW(tray_class.as_ptr(), std::ptr::null()) }
}

fn get_taskbar_rects(scale: f64, work: WinRect) -> (WinRect, Option<WinRect>) {
    let notify_class = to_wide("TrayNotifyWnd");
    unsafe {
        let tray_hwnd = find_taskbar_hwnd();
        if tray_hwnd != 0 {
            let mut tb_rect = WinRect::default();
            if GetWindowRect(tray_hwnd, &mut tb_rect) != 0 && tb_rect.right > tb_rect.left {
                let notify_hwnd =
                    FindWindowExW(tray_hwnd, 0, notify_class.as_ptr(), std::ptr::null());
                let notify_rect = if notify_hwnd != 0 {
                    let mut nr = WinRect::default();
                    if GetWindowRect(notify_hwnd, &mut nr) != 0 && nr.right > nr.left {
                        Some(nr)
                    } else {
                        None
                    }
                } else {
                    None
                };
                return (tb_rect, notify_rect);
            }
        }
    }
    let tb_h = (48.0 * scale).round() as i32;
    (
        WinRect {
            left: work.left,
            top: work.bottom,
            right: work.right,
            bottom: work.bottom + tb_h,
        },
        None,
    )
}

/// Embeds the `pill` window directly inside `Shell_TrayWnd` via Win32 `SetParent` on the left "Widgets" area.
/// Returns `(rel_x, screen_x, screen_y, pill_w, pill_h, sampled_color)`.
fn embed_pill_into_taskbar(
    pill_hwnd: isize,
    desired_rel_x: i32,
    scale: f64,
    work: WinRect,
) -> (i32, i32, i32, i32, i32, Option<String>) {
    let pill_w = (PILL_WIDTH_LOGICAL * scale).round() as i32;
    let pill_h = (PILL_HEIGHT_LOGICAL * scale).round() as i32;
    let (tb_rect, notify_opt) = get_taskbar_rects(scale, work);
    let tb_w = (tb_rect.right - tb_rect.left).max(pill_w + 16);
    let tb_h = (tb_rect.bottom - tb_rect.top).max(pill_h);

    let min_rel_x = (8.0 * scale).round() as i32;
    let max_rel_x = notify_opt
        .map(|nr| (nr.left - tb_rect.left) - pill_w - (6.0 * scale).round() as i32)
        .unwrap_or(tb_w - pill_w - min_rel_x)
        .max(min_rel_x);

    // Default to the left "Widgets" slot on the Windows 11 taskbar
    let rel_x = if desired_rel_x <= 0 {
        min_rel_x
    } else {
        desired_rel_x.clamp(min_rel_x, max_rel_x)
    };
    let rel_y = ((tb_h - pill_h) / 2).max(0);
    let screen_x = tb_rect.left + rel_x;
    let screen_y = tb_rect.top + rel_y;

    // Sample taskbar color slightly to the right of the pill inside the taskbar
    let sample_x = (screen_x + pill_w + (10.0 * scale).round() as i32).min(tb_rect.right - 20);
    let sampled_col = sample_taskbar_color(sample_x, tb_rect.top, tb_h);

    apply_hwnd_taskbar_style(pill_hwnd, true);
    apply_hwnd_opacity(pill_hwnd, 100);

    unsafe {
        let tray_hwnd = find_taskbar_hwnd();
        if tray_hwnd != 0 {
            if GetParent(pill_hwnd) != tray_hwnd {
                let style = GetWindowLongW(pill_hwnd, GWL_STYLE);
                SetWindowLongW(
                    pill_hwnd,
                    GWL_STYLE,
                    (style & !WS_POPUP) | WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                );
                let ex = GetWindowLongW(pill_hwnd, GWL_EXSTYLE);
                SetWindowLongW(
                    pill_hwnd,
                    GWL_EXSTYLE,
                    (ex & !WS_EX_LAYERED) | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                );
                let _ = SetParent(pill_hwnd, tray_hwnd);
            }
            let _ = SetWindowPos(
                pill_hwnd,
                HWND_TOP,
                rel_x,
                rel_y,
                pill_w,
                pill_h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_FRAMECHANGED,
            );
        } else {
            let _ = SetWindowPos(
                pill_hwnd,
                HWND_TOPMOST,
                screen_x,
                screen_y,
                pill_w,
                pill_h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }

    (rel_x, screen_x, screen_y, pill_w, pill_h, sampled_col)
}

fn raise_pill_in_taskbar(pill_hwnd: isize, rel_x: i32, scale: f64, work: WinRect) {
    let tray_hwnd = find_taskbar_hwnd();
    if tray_hwnd != 0 {
        unsafe {
            if GetParent(pill_hwnd) != tray_hwnd {
                let _ = embed_pill_into_taskbar(pill_hwnd, rel_x, scale, work);
            } else {
                let _ = SetWindowPos(
                    pill_hwnd,
                    HWND_TOP,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct SavedWindowState {
    anchor_x: i32,
    anchor_y: i32,
    dock_edge: String,
    opacity: u8,
    always_on_top: bool,
    #[serde(default)]
    is_pinned: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct DockStateView {
    is_collapsed: bool,
    dock_edge: String,
    is_pinned: bool,
    opacity: u8,
    taskbar_color: Option<String>,
}

struct DockRuntime {
    anchor_x: i32,
    anchor_y: i32,
    dock_edge: String,
    is_dragging: bool,
    is_pinned: bool,
    taskbar_color: Option<String>,
}

fn window_state_path() -> Option<PathBuf> {
    agy_dir().map(|d| d.join("window_state.json"))
}

fn load_saved_window_state() -> Option<SavedWindowState> {
    let path = window_state_path()?;
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str::<SavedWindowState>(&raw).ok()
}

fn save_window_state_sync(st: &SavedWindowState) {
    if let Some(path) = window_state_path() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json_str) = serde_json::to_string_pretty(st) {
            let _ = fs::write(path, json_str);
        }
    }
}

struct AppState {
    cached_token: Mutex<Option<(String, String, Instant)>>, // (access_token, email, expires_at)
    cached_project_id: Mutex<Option<String>>,
    cached_oauth_client: Mutex<Option<(String, String)>>, // Dynamically read from local CLI/IDE binary
    cached_user_agent: Mutex<Option<String>>,             // Dynamically detected via `agy.exe --version`
    window_opacity: Mutex<u8>,
    dock: Mutex<DockRuntime>,
    last_snapshot: Mutex<Option<UsageSnapshot>>,
    show_remaining: Mutex<bool>,
    http: reqwest::Client,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct StoredCred {
    token: Option<StoredOAuthToken>,
    auth_method: Option<String>,
    id_token: Option<String>,
    id_token_email: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct StoredOAuthToken {
    access_token: Option<String>,
    token_type: Option<String>,
    refresh_token: Option<String>,
    expiry: Option<String>,
}

#[derive(Deserialize, Debug)]
struct TokenExchangeResponse {
    access_token: String,
    id_token: Option<String>,
    expires_in: Option<u64>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct QuotaBucketView {
    pub id: String,
    pub group_key: String,
    pub group_name: String,
    pub group_description: String,
    pub display_name: String,
    pub window: String,
    pub remaining_fraction: f64,
    pub remaining_percent: f64,
    pub used_percent: f64,
    pub reset_time: Option<String>,
    pub reset_seconds: Option<i64>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModelQuotaView {
    pub id: String,
    pub display_name: String,
    pub family_key: String,
    pub remaining_fraction: f64,
    pub remaining_percent: f64,
    pub used_percent: f64,
    pub reset_time: Option<String>,
    pub reset_seconds: Option<i64>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub installed: bool,
    pub authenticated: bool,
    pub auth_source: String,
    pub email: String,
    pub tier_name: Option<String>,
    pub paid_tier_name: Option<String>,
    pub project_id: String,
    pub active_sessions: usize,
    pub buckets: Vec<QuotaBucketView>,
    pub models: Vec<ModelQuotaView>,
    pub updated_at: String,
}

fn to_wide(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_utf16().collect();
    v.push(0);
    v
}

fn read_windows_credential(target: &str) -> Result<String, String> {
    let wide_target = to_wide(target);
    unsafe {
        let mut cred_ptr: *mut CREDENTIALW = std::ptr::null_mut();
        let ok = CredReadW(
            wide_target.as_ptr(),
            CRED_TYPE_GENERIC,
            0,
            &mut cred_ptr,
        );
        if ok == 0 || cred_ptr.is_null() {
            return Err("ERR_CRED_NOT_FOUND".to_string());
        }

        let cred = &*cred_ptr;
        let slice = std::slice::from_raw_parts(
            cred.CredentialBlob,
            cred.CredentialBlobSize as usize,
        );
        let text = String::from_utf8_lossy(slice).to_string();
        CredFree(cred_ptr as *const _);
        Ok(text)
    }
}

/// Locates installed Antigravity CLI (`agy.exe`) and IDE binaries on the user's machine.
fn find_antigravity_binaries() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = dirs::home_dir() {
        let gemini_bin = home.join(".gemini").join("bin").join("agy.exe");
        if gemini_bin.is_file() {
            paths.push(gemini_bin);
        }
    }
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let base = PathBuf::from(&local_app_data);
        let cli_bin = base.join("agy").join("bin").join("agy.exe");
        if cli_bin.is_file() && !paths.contains(&cli_bin) {
            paths.push(cli_bin);
        }
        for folder in ["Antigravity IDE", "Antigravity-IDE", "antigravity"] {
            let ide_ls = base
                .join("Programs")
                .join(folder)
                .join("resources")
                .join("app")
                .join("extensions")
                .join("antigravity")
                .join("bin")
                .join("language_server_windows_x64.exe");
            if ide_ls.is_file() && !paths.contains(&ide_ls) {
                paths.push(ide_ls);
            }
        }
    }
    paths
}

fn is_antigravity_installed() -> bool {
    if !find_antigravity_binaries().is_empty() {
        return true;
    }
    if let Some(dir) = agy_ide_dir() {
        if dir.is_dir() {
            return true;
        }
    }
    if let Some(dir) = agy_dir() {
        if dir.is_dir() {
            return true;
        }
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        let ide_dir = PathBuf::from(appdata).join("Antigravity IDE");
        if ide_dir.is_dir() {
            return true;
        }
    }
    false
}

/// Dynamically detects the installed Antigravity CLI version via `agy.exe --version` (or IDE package.json)
/// and constructs the User-Agent string without hardcoding version numbers or changelists.
async fn resolve_user_agent(state: &AppState) -> String {
    {
        let guard = state.cached_user_agent.lock().await;
        if let Some(ref ua) = *guard {
            return ua.clone();
        }
    }

    let mut version = String::new();
    let mut candidate_bins = Vec::new();
    if let Some(home) = dirs::home_dir() {
        candidate_bins.push(home.join(".gemini").join("bin").join("agy.exe"));
    }
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let base = PathBuf::from(&local_app_data);
        candidate_bins.push(base.join("agy").join("bin").join("agy.exe"));
    }

    for cli_bin in candidate_bins {
        if cli_bin.is_file() {
            if let Ok(out) = Command::new(&cli_bin)
                .arg("--version")
                .creation_flags(CREATE_NO_WINDOW)
                .output()
            {
                let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !v.is_empty()
                    && v.chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
                {
                    version = v;
                    break;
                }
            }
        }
    }

    if version.is_empty() {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            let base = PathBuf::from(&local_app_data);
            for folder in ["Antigravity IDE", "Antigravity-IDE", "antigravity"] {
                let pkg_path = base
                    .join("Programs")
                    .join(folder)
                    .join("resources")
                    .join("app")
                    .join("package.json");
                if let Ok(raw) = fs::read_to_string(pkg_path) {
                    if let Ok(val) = serde_json::from_str::<Value>(&raw) {
                        if let Some(v) = val.get("version").and_then(|v| v.as_str()) {
                            version = v.trim().to_string();
                            if !version.is_empty() {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    };
    let os_type = std::env::consts::OS;
    let ver_segment = if version.is_empty() {
        String::new()
    } else {
        format!("/{}", version)
    };

    let ua = format!(
        "antigravity/cli{} (aidev_client; os_type={}; arch={}; auth_method=consumer)",
        ver_segment, os_type, arch
    );

    let mut guard = state.cached_user_agent.lock().await;
    *guard = Some(ua.clone());
    ua
}

/// Dynamically extracts OAuth Client IDs and Secrets from the locally installed Antigravity CLI/IDE binary
/// so no Google credentials or prefixes are ever hardcoded in this application's source code or binary.
fn extract_oauth_clients_from_binary(bytes: &[u8]) -> Vec<(String, String)> {
    let suffix = b".apps.googleusercontent.com";
    let mut client_ids = Vec::new();

    let mut pos = 0;
    while pos + suffix.len() <= bytes.len() {
        if let Some(rel) = bytes[pos..].windows(suffix.len()).position(|w| w == suffix) {
            let match_idx = pos + rel;
            let mut start = match_idx;
            while start > 0 {
                let b = bytes[start - 1];
                if b.is_ascii_alphanumeric() || b == b'-' {
                    start -= 1;
                } else {
                    break;
                }
            }
            let candidate = &bytes[start..match_idx + suffix.len()];
            if let Ok(s) = std::str::from_utf8(candidate) {
                if let Some((num_part, rest)) = s.split_once('-') {
                    // Strip any preceding Go `.rodata` string characters before the numeric project ID
                    let clean_num: String = num_part
                        .chars()
                        .rev()
                        .take_while(|c| c.is_ascii_digit())
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    if clean_num.len() >= 10 && rest.len() > suffix.len() + 20 {
                        let cid = format!("{}-{}", clean_num, rest);
                        if !client_ids.contains(&cid) {
                            client_ids.push(cid);
                        }
                    }
                }
            }
            pos = match_idx + suffix.len();
        } else {
            break;
        }
    }

    let secret_prefix = b"GOCSPX-";
    let mut secrets = Vec::new();
    let mut spos = 0;
    while spos + secret_prefix.len() + 28 <= bytes.len() {
        if let Some(rel) = bytes[spos..]
            .windows(secret_prefix.len())
            .position(|w| w == secret_prefix)
        {
            let match_idx = spos + rel;
            let end = (match_idx + 35).min(bytes.len());
            let slice = &bytes[match_idx..end];
            if slice.len() == 35
                && slice[7..]
                    .iter()
                    .all(|&b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                if let Ok(s) = std::str::from_utf8(slice) {
                    let sec = s.to_string();
                    if !secrets.contains(&sec) {
                        secrets.push(sec);
                    }
                }
            }
            spos = match_idx + secret_prefix.len();
        } else {
            break;
        }
    }

    let mut pairs = Vec::new();
    for cid in &client_ids {
        for sec in &secrets {
            pairs.push((cid.clone(), sec.clone()));
        }
    }
    pairs
}

async fn refresh_with_local_oauth_client(
    state: &AppState,
    refresh_token: &str,
) -> Result<TokenExchangeResponse, String> {
    let cached_pair = {
        let guard = state.cached_oauth_client.lock().await;
        guard.clone()
    };

    if let Some((ref cid, ref sec)) = cached_pair {
        if let Ok(resp) = state
            .http
            .post("https://oauth2.googleapis.com/token")
            .form(&[
                ("client_id", cid.as_str()),
                ("client_secret", sec.as_str()),
                ("refresh_token", refresh_token),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(parsed) = resp.json::<TokenExchangeResponse>().await {
                    return Ok(parsed);
                }
            }
        }
    }

    for bin_path in find_antigravity_binaries() {
        let Ok(bytes) = fs::read(&bin_path) else {
            continue;
        };
        for (cid, sec) in extract_oauth_clients_from_binary(&bytes) {
            let resp = state
                .http
                .post("https://oauth2.googleapis.com/token")
                .form(&[
                    ("client_id", cid.as_str()),
                    ("client_secret", sec.as_str()),
                    ("refresh_token", refresh_token),
                    ("grant_type", "refresh_token"),
                ])
                .send()
                .await
                .map_err(|_| "ERR_TOKEN_REFRESH".to_string())?;

            if resp.status().is_success() {
                let parsed: TokenExchangeResponse = resp
                    .json()
                    .await
                    .map_err(|_| "ERR_INVALID_OAUTH".to_string())?;
                let mut guard = state.cached_oauth_client.lock().await;
                *guard = Some((cid, sec));
                return Ok(parsed);
            }
        }
    }

    Err("NOT_AUTHENTICATED".to_string())
}

fn read_file_shared(path: &PathBuf) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut file = fs::OpenOptions::new()
        .read(true)
        .share_mode(7)
        .open(path)
        .ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// Attempts to read an OAuth token from Antigravity IDE's `state.vscdb` (or `.backup`).
fn load_ide_vscdb_credential() -> Option<StoredCred> {
    let appdata = std::env::var("APPDATA").ok()?;
    let base = PathBuf::from(appdata);

    for ide_folder in ["Antigravity IDE", "Antigravity"] {
        let storage_dir = base.join(ide_folder).join("User").join("globalStorage");
        for filename in ["state.vscdb", "state.vscdb.backup"] {
            let db_path = storage_dir.join(filename);
            let Some(bytes) = read_file_shared(&db_path) else {
                continue;
            };
            let key_marker = b"antigravityUnifiedStateSync.oauthToken";
            let mut pos = 0;
            let mut latest_match: Option<StoredCred> = None;
            while pos + key_marker.len() < bytes.len() {
                let Some(rel) = bytes[pos..]
                    .windows(key_marker.len())
                    .position(|w| w == key_marker)
                else {
                    break;
                };
                let start = pos + rel + key_marker.len();
                pos = start;

                let b64_slice: Vec<u8> = bytes[start..]
                    .iter()
                    .copied()
                    .take_while(|&b| {
                        b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'='
                    })
                    .collect();

                if b64_slice.len() < 64 {
                    continue;
                }
                let Ok(outer_decoded) = STANDARD.decode(&b64_slice) else {
                    continue;
                };

                let sentinel = b"oauthTokenInfoSentinelKey";
                if let Some(s_rel) = outer_decoded
                    .windows(sentinel.len())
                    .position(|w| w == sentinel)
                {
                    let after_sentinel = &outer_decoded[s_rel + sentinel.len()..];
                    if let Some(b64_start) = after_sentinel
                        .iter()
                        .position(|&b| b == b'C' || b == b'E')
                    {
                        let inner_b64: Vec<u8> = after_sentinel[b64_start..]
                            .iter()
                            .copied()
                            .take_while(|&b| {
                                b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'='
                            })
                            .collect();
                        if let Ok(inner_bytes) = STANDARD.decode(&inner_b64) {
                            let text = String::from_utf8_lossy(&inner_bytes);
                            let access = extract_token_prefix(&text, "ya29.");
                            let refresh = extract_token_prefix(&text, "1//");
                            if refresh.is_some() || access.is_some() {
                                latest_match = Some(StoredCred {
                                    token: Some(StoredOAuthToken {
                                        access_token: access,
                                        token_type: Some("Bearer".to_string()),
                                        refresh_token: refresh,
                                        expiry: None,
                                    }),
                                    auth_method: Some("consumer".to_string()),
                                    id_token: None,
                                    id_token_email: None,
                                });
                            }
                        }
                    }
                }
            }
            if latest_match.is_some() {
                return latest_match;
            }
        }
    }
    None
}

fn extract_token_prefix(haystack: &str, prefix: &str) -> Option<String> {
    let idx = haystack.find(prefix)?;
    let token: String = haystack[idx..]
        .chars()
        .take_while(|&c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' || c == '/')
        .collect();
    if token.len() > prefix.len() + 10 {
        Some(token)
    } else {
        None
    }
}

fn agy_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".gemini").join("antigravity-cli"))
}

fn agy_ide_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".gemini").join("antigravity"))
}

/// Checks if a file is currently locked by an active OS process (exclusive share_mode(0) open fails).
fn is_file_os_locked(path: &PathBuf) -> bool {
    if !path.is_file() {
        return false;
    }
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(path)
        .is_err()
}

fn count_locked_presence_files(base_dir: &PathBuf) -> usize {
    let presence = base_dir.join("presence");
    let Ok(entries) = fs::read_dir(presence) else {
        return 0;
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|x| x.to_str())
                .map(|x| x == "lock")
                .unwrap_or(false)
                && is_file_os_locked(p)
        })
        .count()
}

/// Returns `(ide_sessions, cli_sessions)` based on live OS file locks in
/// `~/.gemini/antigravity` (IDE) and `~/.gemini/antigravity-cli` (CLI).
fn detect_active_sessions() -> (usize, usize) {
    let mut ide_sessions = 0;
    if let Some(ide_dir) = agy_ide_dir() {
        let locks = count_locked_presence_files(&ide_dir);
        if locks > 0 {
            ide_sessions = locks;
        } else if is_file_os_locked(&ide_dir.join("cli.log"))
            || is_file_os_locked(&ide_dir.join("conversation_summaries.db-shm"))
        {
            ide_sessions = 1;
        }
    }

    let mut cli_sessions = 0;
    if let Some(cli_dir) = agy_dir() {
        let locks = count_locked_presence_files(&cli_dir);
        if locks > 0 {
            cli_sessions = locks;
        }
    }

    (ide_sessions, cli_sessions)
}

fn resolve_active_source_label() -> String {
    let (ide_sessions, cli_sessions) = detect_active_sessions();
    if ide_sessions > 0 && cli_sessions > 0 {
        "Antigravity IDE & CLI".to_string()
    } else if ide_sessions > 0 {
        "Antigravity IDE".to_string()
    } else if cli_sessions > 0 {
        "Antigravity CLI".to_string()
    } else if agy_ide_dir().map(|d| d.is_dir()).unwrap_or(false) {
        "Antigravity IDE".to_string()
    } else {
        "Antigravity CLI".to_string()
    }
}

/// Loads all candidate sessions from official Antigravity IDE, CLI, and ACP stores.
fn load_all_stored_credentials() -> Vec<(StoredCred, String)> {
    let mut list = Vec::new();
    let active_label = resolve_active_source_label();
    let (ide_sessions, _) = detect_active_sessions();

    // If IDE is actively running, check IDE's state.vscdb first as well as Windows Credential Manager
    if ide_sessions > 0 {
        if let Some(cred) = load_ide_vscdb_credential() {
            list.push((cred, active_label.clone()));
        }
    }

    if let Ok(raw) = read_windows_credential(AGY_CRED_TARGET) {
        if let Ok(cred) = serde_json::from_str::<StoredCred>(&raw) {
            if cred
                .token
                .as_ref()
                .and_then(|t| t.refresh_token.as_ref().or(t.access_token.as_ref()))
                .is_some()
            {
                list.push((cred, active_label.clone()));
            }
        }
    }

    if ide_sessions == 0 {
        if let Some(cred) = load_ide_vscdb_credential() {
            list.push((cred, "Antigravity IDE".to_string()));
        }
    }

    if let Some(home) = dirs::home_dir() {
        let acp_path = home
            .join(".gemini")
            .join("antigravity-acp")
            .join("acp_token.json");
        if let Ok(raw) = fs::read_to_string(acp_path) {
            if let Ok(val) = serde_json::from_str::<Value>(&raw) {
                if let Some(rt) = val.get("refresh_token").and_then(|v| v.as_str()) {
                    list.push((
                        StoredCred {
                            token: Some(StoredOAuthToken {
                                access_token: None,
                                token_type: Some("Bearer".to_string()),
                                refresh_token: Some(rt.to_string()),
                                expiry: None,
                            }),
                            auth_method: Some("consumer".to_string()),
                            id_token: None,
                            id_token_email: None,
                        },
                        "Antigravity ACP".to_string(),
                    ));
                }
            }
        }
    }

    list
}

fn load_stored_credential() -> Result<(StoredCred, String), String> {
    load_all_stored_credentials()
        .into_iter()
        .next()
        .ok_or_else(|| "NOT_AUTHENTICATED".to_string())
}

fn decode_jwt_email(id_token: &str) -> Option<String> {
    let parts: Vec<&str> = id_token.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let bytes = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    let val: Value = serde_json::from_slice(&bytes).ok()?;
    val.get("email")?.as_str().map(|s| s.to_string())
}

async fn fetch_userinfo_email(http: &reqwest::Client, access_token: &str) -> Option<String> {
    let resp = http
        .get("https://www.googleapis.com/oauth2/v2/userinfo")
        .header("Authorization", format!("Bearer {}", access_token))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let val: Value = resp.json().await.ok()?;
    val.get("email")?.as_str().map(|s| s.to_string())
}

fn read_local_project_id() -> Option<String> {
    for dir_opt in [agy_ide_dir(), agy_dir()] {
        if let Some(dir) = dir_opt {
            let p = dir.join("cache").join("default_project_id.txt");
            if let Ok(s) = fs::read_to_string(p) {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }
    None
}

fn count_active_sessions() -> usize {
    let (ide_sessions, cli_sessions) = detect_active_sessions();
    ide_sessions + cli_sessions
}

fn compute_reset_seconds(reset_iso: Option<&str>) -> Option<i64> {
    let iso = reset_iso?;
    let dt = DateTime::parse_from_rfc3339(iso).ok()?;
    let diff = dt.with_timezone(&Utc) - Utc::now();
    Some(diff.num_seconds().max(0))
}

async fn get_valid_access_token(
    state: &AppState,
    force_refresh: bool,
) -> Result<(String, String, String), String> {
    let candidates = load_all_stored_credentials();
    if candidates.is_empty() {
        return Err("NOT_AUTHENTICATED".to_string());
    }

    if !force_refresh {
        let guard = state.cached_token.lock().await;
        if let Some((ref tok, ref cached_email, expires_at)) = *guard {
            if Instant::now() < expires_at {
                return Ok((
                    tok.clone(),
                    cached_email.clone(),
                    resolve_active_source_label(),
                ));
            }
        }
    }

    for (cred, auth_source) in candidates {
        let mut email = cred
            .id_token_email
            .clone()
            .or_else(|| cred.id_token.as_deref().and_then(decode_jwt_email))
            .unwrap_or_default();

        let Some(token_obj) = cred.token else {
            continue;
        };

        if !force_refresh {
            if let (Some(ref access), Some(ref exp_str)) =
                (&token_obj.access_token, &token_obj.expiry)
            {
                if let Ok(exp_dt) = DateTime::parse_from_rfc3339(exp_str) {
                    let remaining = exp_dt.with_timezone(&Utc) - Utc::now();
                    if remaining.num_seconds() > 120 {
                        if email.is_empty() {
                            email = fetch_userinfo_email(&state.http, access)
                                .await
                                .unwrap_or_else(|| auth_source.clone());
                        }
                        let dur = Duration::from_secs((remaining.num_seconds() - 60) as u64);
                        let mut guard = state.cached_token.lock().await;
                        *guard = Some((access.clone(), email.clone(), Instant::now() + dur));
                        return Ok((access.clone(), email, auth_source));
                    }
                }
            }
        }

        if let Some(ref refresh_token) = token_obj.refresh_token {
            if let Ok(refreshed) = refresh_with_local_oauth_client(state, refresh_token).await {
                if let Some(ref id_tok) = refreshed.id_token {
                    if let Some(em) = decode_jwt_email(id_tok) {
                        email = em;
                    }
                }
                if email.is_empty() {
                    email = fetch_userinfo_email(&state.http, &refreshed.access_token)
                        .await
                        .unwrap_or_else(|| auth_source.clone());
                }

                let ttl = refreshed.expires_in.unwrap_or(3500).saturating_sub(120);
                let mut guard = state.cached_token.lock().await;
                *guard = Some((
                    refreshed.access_token.clone(),
                    email.clone(),
                    Instant::now() + Duration::from_secs(ttl),
                ));

                return Ok((refreshed.access_token, email, auth_source));
            }
        }
    }

    Err("NOT_AUTHENTICATED".to_string())
}

async fn fetch_usage_internal(state: &AppState) -> Result<UsageSnapshot, String> {
    let installed = is_antigravity_installed();
    let token_res = get_valid_access_token(state, false).await;
    let (mut token, email, auth_source) = match token_res {
        Ok(triple) => triple,
        Err(e) if e == "NOT_AUTHENTICATED" => {
            return Ok(UsageSnapshot {
                installed,
                authenticated: false,
                auth_source: String::new(),
                email: String::new(),
                tier_name: None,
                paid_tier_name: None,
                project_id: String::new(),
                active_sessions: count_active_sessions(),
                buckets: Vec::new(),
                models: Vec::new(),
                updated_at: chrono::Local::now().format("%H:%M:%S").to_string(),
            });
        }
        Err(e) => return Err(e),
    };

    let assist_url = format!("{}/v1internal:loadCodeAssist", API_BASE);
    let quota_url = format!("{}/v1internal:retrieveUserQuotaSummary", API_BASE);
    let models_url = format!("{}/v1internal:fetchAvailableModels", API_BASE);

    let mut project_id = {
        let guard = state.cached_project_id.lock().await;
        guard.clone()
    }
    .or_else(read_local_project_id)
    .unwrap_or_default();

    let assist_body = if project_id.is_empty() {
        json!({ "metadata": { "ideType": "ANTIGRAVITY" } })
    } else {
        json!({
            "cloudaicompanionProject": project_id,
            "metadata": { "ideType": "ANTIGRAVITY" }
        })
    };

    let user_agent = resolve_user_agent(state).await;

    let mut assist_resp = state
        .http
        .post(&assist_url)
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", &user_agent)
        .json(&assist_body)
        .send()
        .await
        .map_err(|_| "ERR_NETWORK".to_string())?;

    if assist_resp.status().as_u16() == 401 {
        let (new_tok, _, _) = get_valid_access_token(state, true).await?;
        token = new_tok;
        assist_resp = state
            .http
            .post(&assist_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", &user_agent)
            .json(&assist_body)
            .send()
            .await
            .map_err(|_| "ERR_NETWORK".to_string())?;
    }

    let assist_val: Value = assist_resp.json().await.unwrap_or(json!({}));

    if project_id.is_empty() {
        project_id = assist_val
            .get("cloudaicompanionProject")
            .and_then(|v| v.as_str())
            .unwrap_or("aicode-consumers")
            .to_string();
    }
    {
        let mut guard = state.cached_project_id.lock().await;
        *guard = Some(project_id.clone());
    }

    let (quota_res, models_res) = tokio::join!(
        state
            .http
            .post(&quota_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", &user_agent)
            .json(&json!({ "project": project_id }))
            .send(),
        state
            .http
            .post(&models_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", &user_agent)
            .json(&json!({ "project": project_id }))
            .send()
    );

    let quota_val: Value = match quota_res {
        Ok(r) => r.json().await.unwrap_or(json!({})),
        Err(_) => json!({}),
    };

    let models_val: Value = match models_res {
        Ok(r) => r.json().await.unwrap_or(json!({})),
        Err(_) => json!({}),
    };

    let tier_name = assist_val
        .pointer("/currentTier/name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let paid_tier_name = assist_val
        .pointer("/paidTier/name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // 1. Parse Quota Summary Groups & Buckets
    let mut buckets = Vec::new();
    if let Some(groups) = quota_val.get("groups").and_then(|g| g.as_array()) {
        for grp in groups {
            let raw_grp_name = grp
                .get("displayName")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let group_key = match raw_grp_name {
                "Gemini Models" => "gemini".to_string(),
                "Claude and GPT models" => "claude_gpt".to_string(),
                _ => "other".to_string(),
            };
            let grp_desc = grp
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if let Some(b_arr) = grp.get("buckets").and_then(|b| b.as_array()) {
                let mut sorted_b: Vec<&Value> = b_arr.iter().collect();
                sorted_b.sort_by_key(|b| {
                    match b.get("window").and_then(|w| w.as_str()).unwrap_or("") {
                        "5h" => 0,
                        "weekly" => 1,
                        _ => 2,
                    }
                });

                for b in sorted_b {
                    let id = b
                        .get("bucketId")
                        .and_then(|v| v.as_str())
                        .unwrap_or("bucket")
                        .to_string();
                    let window = b
                        .get("window")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let disp = b
                        .get("displayName")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&id)
                        .to_string();
                    let rem = b
                        .get("remainingFraction")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1.0)
                        .clamp(0.0, 1.0);
                    let reset_time = b
                        .get("resetTime")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let reset_seconds = compute_reset_seconds(reset_time.as_deref());
                    buckets.push(QuotaBucketView {
                        id,
                        group_key: group_key.clone(),
                        group_name: raw_grp_name.to_string(),
                        group_description: grp_desc.clone(),
                        display_name: disp,
                        window,
                        remaining_fraction: rem,
                        remaining_percent: (rem * 1000.0).round() / 10.0,
                        used_percent: ((1.0 - rem) * 1000.0).round() / 10.0,
                        reset_time,
                        reset_seconds,
                    });
                }
            }
        }
    }

    // 2. Parse active recommended models from agentModelSorts
    let mut allowed_ids: Vec<String> = Vec::new();
    let sorts_groups = models_val
        .pointer("/agentModelSorts/0/groups")
        .and_then(|g| g.as_array())
        .or_else(|| {
            models_val
                .pointer("/agentModelSorts/groups")
                .and_then(|g| g.as_array())
        });
    if let Some(groups) = sorts_groups {
        for g in groups {
            if let Some(ids) = g.get("modelIds").and_then(|m| m.as_array()) {
                for id_val in ids {
                    if let Some(id_str) = id_val.as_str() {
                        if !id_str.starts_with("gpt-oss") {
                            allowed_ids.push(id_str.to_string());
                        }
                    }
                }
            }
        }
    }

    let allowed_set: HashSet<String> = allowed_ids.iter().cloned().collect();
    let mut models = Vec::new();

    if let Some(m_obj) = models_val.get("models").and_then(|m| m.as_object()) {
        let keys_to_iter: Vec<String> = if !allowed_ids.is_empty() {
            allowed_ids
        } else {
            m_obj.keys().cloned().collect()
        };

        for model_id in keys_to_iter {
            let Some(m_val) = m_obj.get(&model_id) else {
                continue;
            };
            if !allowed_set.is_empty() && !allowed_set.contains(&model_id) {
                continue;
            }
            let family_key = if model_id.starts_with("gemini-4-argon") {
                "gemini_4_argon".to_string()
            } else if model_id.starts_with("gemini-3.8-flash") {
                "gemini_3_8_flash".to_string()
            } else if model_id.starts_with("claude-opus") {
                "claude_opus".to_string()
            } else if model_id.starts_with("claude-sonnet") {
                "claude_sonnet".to_string()
            } else {
                continue;
            };

            let Some(display_name) = m_val
                .get("displayName")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
            else {
                continue;
            };

            let Some(q_info) = m_val.get("quotaInfo") else {
                continue;
            };
            let rem = q_info
                .get("remainingFraction")
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0)
                .clamp(0.0, 1.0);
            let reset_time = q_info
                .get("resetTime")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let reset_seconds = compute_reset_seconds(reset_time.as_deref());

            models.push(ModelQuotaView {
                id: model_id,
                display_name,
                family_key,
                remaining_fraction: rem,
                remaining_percent: (rem * 1000.0).round() / 10.0,
                used_percent: ((1.0 - rem) * 1000.0).round() / 10.0,
                reset_time,
                reset_seconds,
            });
        }
    }

    let now_local = chrono::Local::now().format("%H:%M:%S").to_string();

    Ok(UsageSnapshot {
        installed,
        authenticated: true,
        auth_source,
        email,
        tier_name,
        paid_tier_name,
        project_id,
        active_sessions: count_active_sessions(),
        buckets,
        models,
        updated_at: now_local,
    })
}

async fn sync_pill_window(app: &tauri::AppHandle, state: &AppState, flyout_open: Option<bool>) {
    let Some(pill_win) = app.get_webview_window("pill") else {
        return;
    };
    let snap_opt = state.last_snapshot.lock().await.clone();
    let show_rem = *state.show_remaining.lock().await;
    let tb_col = state.dock.lock().await.taskbar_color.clone();
    let snap_json = serde_json::to_string(&snap_opt).unwrap_or_else(|_| "null".to_string());
    let tb_json = serde_json::to_string(&tb_col).unwrap_or_else(|_| "null".to_string());
    let open_json = match flyout_open {
        Some(b) => if b { "true" } else { "false" }.to_string(),
        None => "undefined".to_string(),
    };
    let _ = pill_win.eval(&format!(
        "window.__onPillSync && window.__onPillSync({}, {}, {}, {});",
        snap_json,
        if show_rem { "true" } else { "false" },
        tb_json,
        open_json
    ));
}

#[tauri::command]
async fn get_usage(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<UsageSnapshot, String> {
    let snap = fetch_usage_internal(&state).await?;
    {
        let mut guard = state.last_snapshot.lock().await;
        *guard = Some(snap.clone());
    }
    sync_pill_window(&app, &state, None).await;
    Ok(snap)
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

async fn persist_current_window_state(state: &AppState) {
    let opacity = *state.window_opacity.lock().await;
    let dock = state.dock.lock().await;
    let saved = SavedWindowState {
        anchor_x: dock.anchor_x,
        anchor_y: dock.anchor_y,
        dock_edge: dock.dock_edge.clone(),
        opacity,
        always_on_top: true,
        is_pinned: dock.is_pinned,
    };
    drop(dock);
    save_window_state_sync(&saved);
}

#[tauri::command]
async fn get_dock_state(state: State<'_, Arc<AppState>>) -> Result<DockStateView, String> {
    let opacity = *state.window_opacity.lock().await;
    let dock = state.dock.lock().await;
    Ok(DockStateView {
        is_collapsed: false,
        dock_edge: dock.dock_edge.clone(),
        is_pinned: dock.is_pinned,
        opacity,
        taskbar_color: dock.taskbar_color.clone(),
    })
}

#[tauri::command]
async fn set_display_mode(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    show_remaining: Option<bool>,
) -> Result<(), String> {
    if let Some(rem) = show_remaining {
        let mut guard = state.show_remaining.lock().await;
        *guard = rem;
    }
    sync_pill_window(&app, &state, None).await;
    Ok(())
}

#[tauri::command]
async fn set_flyout_pinned(
    state: State<'_, Arc<AppState>>,
    pinned: bool,
) -> Result<(), String> {
    {
        let mut dock = state.dock.lock().await;
        dock.is_pinned = pinned;
    }
    persist_current_window_state(&state).await;
    Ok(())
}

#[tauri::command]
async fn collapse_to_edge(window: tauri::WebviewWindow) -> Result<(), String> {
    let _ = window.hide();
    Ok(())
}

#[tauri::command]
async fn set_window_opacity(
    window: tauri::WebviewWindow,
    state: State<'_, Arc<AppState>>,
    percent: u8,
) -> Result<(), String> {
    let clamped = percent.clamp(45, 100);
    {
        let mut guard = state.window_opacity.lock().await;
        *guard = clamped;
    }
    if let Ok(hwnd) = window.hwnd() {
        apply_hwnd_opacity(hwnd.0 as isize, clamped);
    }
    persist_current_window_state(&state).await;
    Ok(())
}

#[tauri::command]
async fn start_window_drag(
    window: tauri::WebviewWindow,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    {
        let mut dock = state.dock.lock().await;
        dock.is_dragging = true;
    }
    let _ = window.start_dragging();
    let state_clone = Arc::clone(&state);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(25)).await;
            let down = unsafe { (GetAsyncKeyState(VK_LBUTTON) as u16 & 0x8000) != 0 };
            if !down {
                break;
            }
        }
        let mut dock = state_clone.dock.lock().await;
        dock.is_dragging = false;
    });
    Ok(())
}

#[tauri::command]
async fn start_pill_drag(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let Some(pill_win) = app.get_webview_window("pill") else {
        return Ok(());
    };
    let Ok(hwnd) = pill_win.hwnd() else {
        return Ok(());
    };
    let pill_h = hwnd.0 as isize;

    let mut start_cursor = WinPoint::default();
    let mut pill_rect = WinRect::default();
    unsafe {
        let _ = GetCursorPos(&mut start_cursor);
        let _ = GetWindowRect(pill_h, &mut pill_rect);
    }
    let grab_offset_x = (start_cursor.x - pill_rect.left).max(0);

    {
        let mut dock = state.dock.lock().await;
        dock.is_dragging = true;
    }

    let state_clone = Arc::clone(&state);
    let app_clone = app.clone();
    tokio::spawn(async move {
        let scale = get_hwnd_scale(pill_h);
        let work = get_work_area_for_hwnd(pill_h);
        let (tb_rect, _) = get_taskbar_rects(scale, work);
        let mut final_rel_x: Option<i32> = None;

        loop {
            tokio::time::sleep(Duration::from_millis(16)).await;
            let down = unsafe { (GetAsyncKeyState(VK_LBUTTON) as u16 & 0x8000) != 0 };
            if !down {
                break;
            }
            let mut cur = WinPoint::default();
            unsafe {
                let _ = GetCursorPos(&mut cur);
            }
            if (cur.x - start_cursor.x).abs() >= 4 {
                let desired_rel_x = (cur.x - grab_offset_x - tb_rect.left).max(4);
                let (rx, _, _, _, _, _) =
                    embed_pill_into_taskbar(pill_h, desired_rel_x, scale, work);
                final_rel_x = Some(rx);
            }
        }

        if let Some(rx) = final_rel_x {
            let (_, _, _, _, _, sampled_col) = embed_pill_into_taskbar(pill_h, rx, scale, work);
            {
                let mut dock = state_clone.dock.lock().await;
                dock.anchor_x = rx;
                dock.is_dragging = false;
                if sampled_col.is_some() {
                    dock.taskbar_color = sampled_col;
                }
            }
            persist_current_window_state(&state_clone).await;
            sync_pill_window(&app_clone, &state_clone, None).await;
        } else {
            let mut dock = state_clone.dock.lock().await;
            dock.is_dragging = false;
        }
    });

    Ok(())
}

#[tauri::command]
async fn launch_antigravity(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<UsageSnapshot, String> {
    {
        let mut guard = state.cached_token.lock().await;
        *guard = None;
    }

    if load_stored_credential().is_err() {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            let base = PathBuf::from(&local_app_data);
            let ide_exe_1 = base
                .join("Programs")
                .join("Antigravity IDE")
                .join("Antigravity IDE.exe");
            let ide_exe_2 = base
                .join("Programs")
                .join("Antigravity-IDE")
                .join("Antigravity IDE.exe");
            let cli_exe = base.join("agy").join("bin").join("agy.exe");

            if ide_exe_1.is_file() {
                let _ = Command::new(&ide_exe_1).spawn();
            } else if ide_exe_2.is_file() {
                let _ = Command::new(&ide_exe_2).spawn();
            } else if cli_exe.is_file() {
                let _ = Command::new("cmd.exe")
                    .args(["/c", "start", "", &cli_exe.to_string_lossy()])
                    .spawn();
            }
        }
    }

    let snap = fetch_usage_internal(&state).await?;
    {
        let mut guard = state.last_snapshot.lock().await;
        *guard = Some(snap.clone());
    }
    sync_pill_window(&app, &state, None).await;
    Ok(snap)
}

fn show_flyout_above_x(w: &tauri::WebviewWindow, pill_left_x: i32, pill_top_y: i32, opacity: u8) {
    let Ok(hwnd) = w.hwnd() else { return };
    let h = hwnd.0 as isize;
    let scale = get_hwnd_scale(h);
    let work = get_work_area_for_point(pill_left_x, pill_top_y);
    let (tb_rect, _) = get_taskbar_rects(scale, work);

    let full_w = (FULL_WIDTH_LOGICAL * scale).round() as i32;
    let full_h = (FULL_HEIGHT_LOGICAL * scale).round() as i32;
    let margin = (8.0 * scale).round() as i32;

    let min_x = work.left + margin;
    let max_x = (work.right - full_w - margin).max(min_x);
    let fx = pill_left_x.clamp(min_x, max_x);
    let fy = (tb_rect.top - full_h - margin).max(work.top + margin);

    apply_hwnd_taskbar_style(h, false);
    apply_hwnd_opacity(h, opacity);
    set_hwnd_bounds(h, fx, fy, full_w, full_h);
    let _ = w.show();
    raise_hwnd_topmost(h);
}

fn main() {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .expect("HTTP_CLIENT_INIT_FAILED");

    let saved_initial = load_saved_window_state();
    let init_opacity = saved_initial
        .as_ref()
        .map(|s| s.opacity.clamp(45, 100))
        .unwrap_or(95);
    let init_pinned = saved_initial.as_ref().map(|s| s.is_pinned).unwrap_or(false);
    // Default to left "Widgets" slot (`anchor_x = 12`) if previous saved state was on the far right (`> 800`)
    let init_rel_x = saved_initial
        .as_ref()
        .filter(|s| s.dock_edge == "taskbar_widgets")
        .map(|s| s.anchor_x)
        .unwrap_or(0);

    let app_state = Arc::new(AppState {
        cached_token: Mutex::new(None),
        cached_project_id: Mutex::new(None),
        cached_oauth_client: Mutex::new(None),
        cached_user_agent: Mutex::new(None),
        window_opacity: Mutex::new(init_opacity),
        dock: Mutex::new(DockRuntime {
            anchor_x: init_rel_x,
            anchor_y: 0,
            dock_edge: "taskbar_widgets".to_string(),
            is_dragging: false,
            is_pinned: init_pinned,
            taskbar_color: None,
        }),
        last_snapshot: Mutex::new(None),
        show_remaining: Mutex::new(true),
        http,
    });

    let state_for_setup = Arc::clone(&app_state);

    tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_usage,
            launch_antigravity,
            quit_app,
            set_display_mode,
            set_window_opacity,
            start_window_drag,
            start_pill_drag,
            get_dock_state,
            set_flyout_pinned,
            collapse_to_edge
        ])
        .setup(move |app| {
            let main_opt = app.get_webview_window("main");
            let pill_opt = app.get_webview_window("pill");

            if let Some(ref w) = main_opt {
                let _ = w.set_always_on_top(true);
                let _ = w.hide();
                if let Ok(hwnd) = w.hwnd() {
                    let h = hwnd.0 as isize;
                    apply_hwnd_taskbar_style(h, false);
                    apply_hwnd_opacity(h, init_opacity);
                }
            }

            if let (Some(main_win), Some(pill_win)) = (main_opt, pill_opt) {
                if let (Ok(main_hwnd), Ok(pill_hwnd)) = (main_win.hwnd(), pill_win.hwnd()) {
                    let main_h = main_hwnd.0 as isize;
                    let pill_h = pill_hwnd.0 as isize;
                    let scale = get_hwnd_scale(pill_h);
                    let work = get_work_area_for_hwnd(pill_h);

                    // Embed the `pill` window directly inside `Shell_TrayWnd` on the left "Widgets" slot
                    let (rel_x, _, _, _, _, sampled_tb) =
                        embed_pill_into_taskbar(pill_h, init_rel_x, scale, work);

                    if let Ok(mut guard) = state_for_setup.dock.try_lock() {
                        guard.anchor_x = rel_x;
                        guard.dock_edge = "taskbar_widgets".to_string();
                        guard.taskbar_color = sampled_tb;
                        save_window_state_sync(&SavedWindowState {
                            anchor_x: rel_x,
                            anchor_y: 0,
                            dock_edge: "taskbar_widgets".to_string(),
                            opacity: init_opacity,
                            always_on_top: true,
                            is_pinned: init_pinned,
                        });
                    }

                    // Hover watcher: monitors cursor over embedded taskbar `pill` -> opens/closes `main` Flyout above taskbar
                    let app_handle_hover = app.handle().clone();
                    let state_for_hover = Arc::clone(&state_for_setup);
                    tauri::async_runtime::spawn(async move {
                        let mut enter_ticks: u32 = 0;
                        let mut leave_ticks: u32 = 0;
                        let mut z_ticks: u32 = 0;

                        loop {
                            tokio::time::sleep(Duration::from_millis(45)).await;

                            let lbutton_down =
                                unsafe { (GetAsyncKeyState(VK_LBUTTON) as u16 & 0x8000) != 0 };
                            let mut cursor = WinPoint::default();
                            let mut pill_rect = WinRect::default();
                            let mut main_rect = WinRect::default();
                            unsafe {
                                let _ = GetCursorPos(&mut cursor);
                                let _ = GetWindowRect(pill_h, &mut pill_rect);
                                let _ = GetWindowRect(main_h, &mut main_rect);
                            }

                            let scale = get_hwnd_scale(pill_h);
                            let pad = (14.0 * scale).round() as i32;
                            let work = get_work_area_for_hwnd(pill_h);

                            // Keep embedded pill at top of Shell_TrayWnd's child Z-order
                            z_ticks += 1;
                            if z_ticks >= 20 {
                                z_ticks = 0;
                                let rx = state_for_hover.dock.lock().await.anchor_x;
                                raise_pill_in_taskbar(pill_h, rx, scale, work);
                            }

                            let inside_pill = cursor.x >= pill_rect.left
                                && cursor.x <= pill_rect.right
                                && cursor.y >= pill_rect.top
                                && cursor.y <= pill_rect.bottom;

                            let flyout_vis = main_win.is_visible().unwrap_or(false);
                            let (is_dragging, is_pinned) = {
                                let dock = state_for_hover.dock.lock().await;
                                (dock.is_dragging, dock.is_pinned)
                            };

                            if is_dragging {
                                enter_ticks = 0;
                                leave_ticks = 0;
                                continue;
                            }

                            if !flyout_vis {
                                leave_ticks = 0;
                                if inside_pill && !lbutton_down {
                                    enter_ticks += 1;
                                    if enter_ticks >= 3 {
                                        enter_ticks = 0;
                                        let op = *state_for_hover.window_opacity.lock().await;
                                        show_flyout_above_x(
                                            &main_win,
                                            pill_rect.left,
                                            pill_rect.top,
                                            op,
                                        );
                                        sync_pill_window(
                                            &app_handle_hover,
                                            &state_for_hover,
                                            Some(true),
                                        )
                                        .await;
                                    }
                                } else {
                                    enter_ticks = 0;
                                }
                            } else {
                                enter_ticks = 0;
                                let (tb_rect, _) = get_taskbar_rects(scale, work);
                                let inside_flyout_or_bridge = cursor.x >= main_rect.left - pad
                                    && cursor.x <= main_rect.right + pad
                                    && cursor.y >= main_rect.top - pad
                                    && cursor.y <= tb_rect.bottom + pad;

                                if inside_pill
                                    || inside_flyout_or_bridge
                                    || lbutton_down
                                    || is_pinned
                                {
                                    leave_ticks = 0;
                                } else {
                                    leave_ticks += 1;
                                    if leave_ticks >= 6 {
                                        leave_ticks = 0;
                                        let _ = main_win.hide();
                                        sync_pill_window(
                                            &app_handle_hover,
                                            &state_for_hover,
                                            Some(false),
                                        )
                                        .await;
                                    }
                                }
                            }
                        }
                    });
                }
            }

            // Background task: initial usage fetch
            let app_handle_bg = app.handle().clone();
            let state_for_bg = Arc::clone(&state_for_setup);
            tauri::async_runtime::spawn(async move {
                if let Ok(snap) = fetch_usage_internal(&state_for_bg).await {
                    {
                        let mut guard = state_for_bg.last_snapshot.lock().await;
                        *guard = Some(snap);
                    }
                    sync_pill_window(&app_handle_bg, &state_for_bg, None).await;
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("TAURI_RUNTIME_ERROR");
}
