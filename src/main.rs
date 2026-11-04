#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ui_win32;

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
use tokio::sync::Mutex;

const API_BASE: &str = "https://daily-cloudcode-pa.googleapis.com";
const AGY_CRED_TARGET: &str = "gemini:antigravity";
const CRED_TYPE_GENERIC: u32 = 1;
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub(crate) const GWL_STYLE: i32 = -16;
pub(crate) const GWL_EXSTYLE: i32 = -20;
pub(crate) const GWLP_HWNDPARENT: i32 = -8;
pub(crate) const WS_VISIBLE: i32 = 0x10000000;
pub(crate) const WS_POPUP: i32 = -2147483648; // 0x80000000
pub(crate) const WS_EX_TOPMOST: i32 = 0x00000008;
pub(crate) const WS_EX_LAYERED: i32 = 0x00080000;
pub(crate) const WS_EX_TOOLWINDOW: i32 = 0x00000080;
pub(crate) const WS_EX_NOACTIVATE: i32 = 0x08000000;
pub(crate) const LWA_ALPHA: u32 = 0x00000002;
pub(crate) const VK_LBUTTON: i32 = 0x01;
pub(crate) const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
pub(crate) const DWMWA_BORDER_COLOR: u32 = 34;
pub(crate) const DWMWCP_DONOTROUND: u32 = 1;
pub(crate) const DWMWCP_ROUND: u32 = 2;
pub(crate) const DWMWA_COLOR_NONE: u32 = 0xFFFFFFFE;
pub(crate) const DWMWA_COLOR_DEFAULT: u32 = 0xFFFFFFFF;
pub(crate) const HWND_TOPMOST: isize = -1;
pub(crate) const SWP_NOSIZE: u32 = 0x0001;
pub(crate) const SWP_NOMOVE: u32 = 0x0002;
pub(crate) const SWP_NOACTIVATE: u32 = 0x0010;
pub(crate) const SWP_FRAMECHANGED: u32 = 0x0020;
pub(crate) const SWP_SHOWWINDOW: u32 = 0x0040;
pub(crate) const MONITOR_DEFAULTTONEAREST: u32 = 0x00000002;

pub(crate) const PILL_WIDTH_LOGICAL: f64 = 124.0;
pub(crate) const PILL_HEIGHT_LOGICAL: f64 = 38.0;
pub(crate) const FULL_WIDTH_LOGICAL: f64 = 368.0;
pub(crate) const FULL_HEIGHT_LOGICAL: f64 = 440.0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WinPoint {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WinRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct MonitorInfo {
    cb_size: u32,
    rc_monitor: WinRect,
    rc_work: WinRect,
    dw_flags: u32,
}

#[repr(C)]
struct FileTime {
    dw_low_date_time: u32,
    dw_high_date_time: u32,
}

#[repr(C)]
struct CredentialW {
    flags: u32,
    cred_type: u32,
    target_name: *mut u16,
    comment: *mut u16,
    last_written: FileTime,
    credential_blob_size: u32,
    credential_blob: *mut u8,
    persist: u32,
    attribute_count: u32,
    attributes: *mut std::ffi::c_void,
    target_alias: *mut u16,
    user_name: *mut u16,
}

#[link(name = "user32")]
extern "system" {
    pub(crate) fn GetWindowLongW(hwnd: isize, index: i32) -> i32;
    pub(crate) fn SetWindowLongW(hwnd: isize, index: i32, new_long: i32) -> i32;
    pub(crate) fn SetWindowLongPtrW(hwnd: isize, index: i32, new_long: isize) -> isize;
    pub(crate) fn SetLayeredWindowAttributes(hwnd: isize, cr_key: u32, alpha: u8, flags: u32) -> i32;
    pub(crate) fn GetWindowRect(hwnd: isize, lp_rect: *mut WinRect) -> i32;
    fn FindWindowW(lp_class_name: *const u16, lp_window_name: *const u16) -> isize;
    fn FindWindowExW(
        hwnd_parent: isize,
        hwnd_child_after: isize,
        lpsz_class: *const u16,
        lpsz_window: *const u16,
    ) -> isize;
    pub(crate) fn SetWindowPos(
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
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> isize;
    fn K32EmptyWorkingSet(h_process: isize) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    fn GetPixel(hdc: isize, x: i32, y: i32) -> u32;
}

#[link(name = "dwmapi")]
extern "system" {
    pub(crate) fn DwmSetWindowAttribute(
        hwnd: isize,
        attr: u32,
        pv_attr: *const u32,
        cb_attr: u32,
    ) -> i32;
}

#[link(name = "advapi32")]
extern "system" {
    fn CredReadW(
        target_name: *const u16,
        cred_type: u32,
        flags: u32,
        credential: *mut *mut CredentialW,
    ) -> i32;
    fn CredFree(buffer: *const std::ffi::c_void);
    fn RegOpenKeyExW(
        hkey: isize,
        lp_sub_key: *const u16,
        ul_options: u32,
        sam_desired: u32,
        phk_result: *mut isize,
    ) -> i32;
    fn RegQueryValueExW(
        hkey: isize,
        lp_value_name: *const u16,
        lp_reserved: *const u32,
        lp_type: *mut u32,
        lp_data: *mut u8,
        lpcb_data: *mut u32,
    ) -> i32;
    fn RegSetValueExW(
        hkey: isize,
        lp_value_name: *const u16,
        reserved: u32,
        dw_type: u32,
        lp_data: *const u8,
        cb_data: u32,
    ) -> i32;
    fn RegDeleteValueW(hkey: isize, lp_value_name: *const u16) -> i32;
    fn RegCloseKey(hkey: isize) -> i32;
}

pub(crate) fn trim_process_working_set() {
    unsafe {
        let _ = K32EmptyWorkingSet(GetCurrentProcess());
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
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
            &corner_pref,
            std::mem::size_of::<u32>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &border_col,
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

pub(crate) fn get_hwnd_scale(hwnd: isize) -> f64 {
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

pub(crate) fn get_work_area_for_point(x: i32, y: i32) -> WinRect {
    let hmon = unsafe { MonitorFromPoint(WinPoint { x, y }, MONITOR_DEFAULTTONEAREST) };
    get_work_area_from_hmonitor(hmon)
}

pub(crate) fn get_work_area_for_hwnd(hwnd: isize) -> WinRect {
    let hmon = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    get_work_area_from_hmonitor(hmon)
}

pub(crate) fn raise_hwnd_topmost(hwnd: isize) {
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

pub(crate) fn get_taskbar_rects(scale: f64, work: WinRect) -> (WinRect, Option<WinRect>) {
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

/// Positions the `pill` window as a top-level layered popup owned by `Shell_TrayWnd`
/// directly over the left "Widgets" area of the Windows 11 taskbar.
/// Returns `(rel_x, sampled_color)`.
pub(crate) fn embed_pill_into_taskbar(
    pill_hwnd: isize,
    desired_rel_x: i32,
    scale: f64,
    work: WinRect,
) -> (i32, Option<String>) {
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

    let rel_x = if desired_rel_x <= 0 {
        min_rel_x
    } else {
        desired_rel_x.clamp(min_rel_x, max_rel_x)
    };
    let rel_y = ((tb_h - pill_h) / 2).max(0);
    let screen_x = tb_rect.left + rel_x;
    let screen_y = tb_rect.top + rel_y;

    let sample_x = (screen_x + pill_w + (10.0 * scale).round() as i32).min(tb_rect.right - 20);
    let sampled_col = sample_taskbar_color(sample_x, tb_rect.top, tb_h);

    apply_hwnd_taskbar_style(pill_hwnd, true);

    unsafe {
        let tray_hwnd = find_taskbar_hwnd();
        let style = GetWindowLongW(pill_hwnd, GWL_STYLE);
        if (style & WS_POPUP) == 0 || (style & WS_VISIBLE) == 0 {
            SetWindowLongW(pill_hwnd, GWL_STYLE, WS_POPUP | WS_VISIBLE);
        }
        let ex = GetWindowLongW(pill_hwnd, GWL_EXSTYLE);
        let target_ex =
            ex | WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
        if ex != target_ex {
            SetWindowLongW(pill_hwnd, GWL_EXSTYLE, target_ex);
        }
        if tray_hwnd != 0 {
            let _ = SetWindowLongPtrW(pill_hwnd, GWLP_HWNDPARENT, tray_hwnd);
        }
        let _ = SetLayeredWindowAttributes(pill_hwnd, 0, 255, LWA_ALPHA);
        let _ = SetWindowPos(
            pill_hwnd,
            HWND_TOPMOST,
            screen_x,
            screen_y,
            pill_w,
            pill_h,
            SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_FRAMECHANGED,
        );
    }

    (rel_x, sampled_col)
}

pub(crate) fn raise_pill_in_taskbar(pill_hwnd: isize) {
    unsafe {
        let tray_hwnd = find_taskbar_hwnd();
        if tray_hwnd != 0 {
            let _ = SetWindowLongPtrW(pill_hwnd, GWLP_HWNDPARENT, tray_hwnd);
        }
        let _ = SetWindowPos(
            pill_hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct SavedWindowState {
    #[serde(default)]
    pub anchor_x: i32,
    #[serde(default = "default_opacity")]
    pub opacity: u8,
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default)]
    pub show_remaining: Option<bool>,
}

fn default_opacity() -> u8 {
    95
}

const AUTOSTART_REG_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const AUTOSTART_REG_NAME: &str = "AntigravityQuotaMonitor";
const HKEY_CURRENT_USER: isize = -2147483647isize; // 0x80000001
const KEY_READ: u32 = 0x20019;
const KEY_SET_VALUE: u32 = 0x0002;
const REG_SZ: u32 = 1;

fn clean_windows_path(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        p
    }
}

fn installed_exe_path() -> Option<PathBuf> {
    std::env::var("LOCALAPPDATA").ok().map(|p| {
        PathBuf::from(p)
            .join("Programs")
            .join("AntigravityQuotaMonitor")
            .join("antigravity-quota-monitor.exe")
    })
}

fn start_menu_shortcut_path() -> Option<PathBuf> {
    std::env::var("APPDATA").ok().map(|p| {
        PathBuf::from(p)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Antigravity Quota Monitor.lnk")
    })
}

pub(crate) fn is_autostart_enabled_sync() -> bool {
    let subkey = to_wide(AUTOSTART_REG_SUBKEY);
    let val_name = to_wide(AUTOSTART_REG_NAME);
    unsafe {
        let mut hkey: isize = 0;
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_READ, &mut hkey) != 0 {
            return false;
        }
        let mut data_len: u32 = 0;
        let status = RegQueryValueExW(
            hkey,
            val_name.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut data_len,
        );
        let _ = RegCloseKey(hkey);
        status == 0 && data_len > 2
    }
}

fn ensure_installed_binary_sync() -> Option<PathBuf> {
    let cur = clean_windows_path(std::env::current_exe().ok()?);
    let target = installed_exe_path()?;
    if cur != target {
        if let Some(parent) = target.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if fs::copy(&cur, &target).is_ok() {
            return Some(target);
        }
    }
    if target.is_file() {
        return Some(target);
    }
    Some(cur)
}

fn create_start_menu_shortcut_sync(target_exe: &PathBuf) {
    let Some(lnk_path) = start_menu_shortcut_path() else {
        return;
    };
    let clean_target = clean_windows_path(target_exe.clone());
    let ps_cmd = format!(
        "$ws = New-Object -ComObject WScript.Shell; $s = $ws.CreateShortcut('{}'); $s.TargetPath = '{}'; $s.WorkingDirectory = '{}'; $s.Description = 'Antigravity Quota Monitor'; $s.Save()",
        lnk_path.to_string_lossy().replace('\'', "''"),
        clean_target.to_string_lossy().replace('\'', "''"),
        clean_target
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
            .replace('\'', "''")
    );
    let _ = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps_cmd])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
}

fn remove_start_menu_shortcut_sync() {
    if let Some(lnk_path) = start_menu_shortcut_path() {
        let _ = fs::remove_file(lnk_path);
    }
}

pub(crate) fn set_autostart_sync(enabled: bool) -> Result<bool, String> {
    let subkey = to_wide(AUTOSTART_REG_SUBKEY);
    let val_name = to_wide(AUTOSTART_REG_NAME);
    if enabled {
        let exe_path = ensure_installed_binary_sync()
            .or_else(|| std::env::current_exe().ok().map(clean_windows_path))
            .ok_or_else(|| "EXE_PATH_NOT_FOUND".to_string())?;
        create_start_menu_shortcut_sync(&exe_path);
        let cmd_str = format!("\"{}\"", exe_path.to_string_lossy());
        let wide_val = to_wide(&cmd_str);
        unsafe {
            let mut hkey: isize = 0;
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut hkey,
            ) != 0
            {
                return Err("REG_OPEN_FAILED".to_string());
            }
            let status = RegSetValueExW(
                hkey,
                val_name.as_ptr(),
                0,
                REG_SZ,
                wide_val.as_ptr() as *const u8,
                (wide_val.len() * 2) as u32,
            );
            let _ = RegCloseKey(hkey);
            if status != 0 {
                return Err(format!("REG_SET_FAILED_{}", status));
            }
        }
    } else {
        unsafe {
            let mut hkey: isize = 0;
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut hkey,
            ) == 0
            {
                let _ = RegDeleteValueW(hkey, val_name.as_ptr());
                let _ = RegCloseKey(hkey);
            }
        }
    }
    Ok(is_autostart_enabled_sync())
}

fn window_state_path() -> Option<PathBuf> {
    agy_dir().map(|d| d.join("window_state.json"))
}

fn load_saved_window_state() -> Option<SavedWindowState> {
    let path = window_state_path()?;
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str::<SavedWindowState>(&raw).ok()
}

pub(crate) fn save_window_state_sync(st: &SavedWindowState) {
    if let Some(path) = window_state_path() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json_str) = serde_json::to_string_pretty(st) {
            let _ = fs::write(path, json_str);
        }
    }
}

pub(crate) struct AppState {
    pub cached_token: Mutex<Option<(String, String, Instant)>>, // (access_token, email, expires_at)
    pub cached_project_id: Mutex<Option<String>>,
    pub cached_oauth_client: Mutex<Option<(String, String)>>, // Dynamically read from local CLI/IDE binary
    pub cached_user_agent: Mutex<Option<String>>,             // Dynamically detected via `agy.exe --version`
    pub http: reqwest::Client,
}

#[derive(Deserialize, Debug, Clone)]
struct StoredCred {
    token: Option<StoredOAuthToken>,
    id_token: Option<String>,
    id_token_email: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
struct StoredOAuthToken {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expiry: Option<String>,
}

#[derive(Deserialize, Debug)]
struct TokenExchangeResponse {
    access_token: String,
    id_token: Option<String>,
    expires_in: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct QuotaBucketView {
    pub group_key: String,
    pub window: String,
    pub remaining_fraction: f64,
    pub remaining_percent: f64,
    pub used_percent: f64,
    pub reset_seconds: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct ModelQuotaView {
    pub display_name: String,
    pub family_key: String,
    pub remaining_fraction: f64,
    pub remaining_percent: f64,
    pub used_percent: f64,
    pub reset_seconds: Option<i64>,
}

#[derive(Clone, Debug)]
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

pub(crate) fn to_wide(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_utf16().collect();
    v.push(0);
    v
}

fn read_windows_credential(target: &str) -> Result<String, String> {
    let wide_target = to_wide(target);
    unsafe {
        let mut cred_ptr: *mut CredentialW = std::ptr::null_mut();
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
            cred.credential_blob,
            cred.credential_blob_size as usize,
        );
        let text = String::from_utf8_lossy(slice).to_string();
        CredFree(cred_ptr as *const _);
        Ok(text)
    }
}

/// Locates installed Antigravity CLI (`agy.exe`) and IDE binaries on the user's machine.
fn find_antigravity_binaries() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = home_dir() {
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
    if let Some(home) = home_dir() {
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
        let pairs = {
            let Ok(bytes) = fs::read(&bin_path) else {
                continue;
            };
            extract_oauth_clients_from_binary(&bytes)
        };
        trim_process_working_set();
        for (cid, sec) in pairs {
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
            let latest_match = {
                let Some(bytes) = read_file_shared(&db_path) else {
                    continue;
                };
                let key_marker = b"antigravityUnifiedStateSync.oauthToken";
                let mut pos = 0;
                let mut found: Option<StoredCred> = None;
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
                                    b.is_ascii_alphanumeric()
                                        || b == b'+'
                                        || b == b'/'
                                        || b == b'='
                                })
                                .collect();
                            if let Ok(inner_bytes) = STANDARD.decode(&inner_b64) {
                                let text = String::from_utf8_lossy(&inner_bytes);
                                let access = extract_token_prefix(&text, "ya29.");
                                let refresh = extract_token_prefix(&text, "1//");
                                if refresh.is_some() || access.is_some() {
                                    found = Some(StoredCred {
                                        token: Some(StoredOAuthToken {
                                            access_token: access,
                                            refresh_token: refresh,
                                            expiry: None,
                                        }),
                                        id_token: None,
                                        id_token_email: None,
                                    });
                                }
                            }
                        }
                    }
                }
                found
            };
            trim_process_working_set();
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
    home_dir().map(|h| h.join(".gemini").join("antigravity-cli"))
}

fn agy_ide_dir() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".gemini").join("antigravity"))
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

    if let Some(home) = home_dir() {
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
                                refresh_token: Some(rt.to_string()),
                                expiry: None,
                            }),
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

pub(crate) async fn fetch_usage_internal(state: &AppState) -> Result<UsageSnapshot, String> {
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
                    let window = b
                        .get("window")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let rem = b
                        .get("remainingFraction")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1.0)
                        .clamp(0.0, 1.0);
                    let reset_time = b.get("resetTime").and_then(|v| v.as_str());
                    let reset_seconds = compute_reset_seconds(reset_time);
                    buckets.push(QuotaBucketView {
                        group_key: group_key.clone(),
                        window,
                        remaining_fraction: rem,
                        remaining_percent: (rem * 1000.0).round() / 10.0,
                        used_percent: ((1.0 - rem) * 1000.0).round() / 10.0,
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
            let reset_time = q_info.get("resetTime").and_then(|v| v.as_str());
            let reset_seconds = compute_reset_seconds(reset_time);

            models.push(ModelQuotaView {
                display_name,
                family_key,
                remaining_fraction: rem,
                remaining_percent: (rem * 1000.0).round() / 10.0,
                used_percent: ((1.0 - rem) * 1000.0).round() / 10.0,
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

pub(crate) async fn launch_antigravity_action(state: &AppState) {
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
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--install") {
        let _ = set_autostart_sync(true);
        if let Some(installed) = installed_exe_path().filter(|p| p.is_file()) {
            let _ = Command::new(installed).spawn();
        }
        std::process::exit(0);
    }
    if args.iter().any(|a| a == "--uninstall") {
        let _ = set_autostart_sync(false);
        remove_start_menu_shortcut_sync();
        std::process::exit(0);
    }

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .expect("HTTP_CLIENT_INIT_FAILED");

    let saved = load_saved_window_state();
    let init_opacity = saved
        .as_ref()
        .map(|s| s.opacity.clamp(45, 100))
        .unwrap_or(95);
    let init_locale = saved
        .as_ref()
        .and_then(|s| s.locale.clone())
        .unwrap_or_else(|| "et".to_string());
    let init_show_rem = saved
        .as_ref()
        .and_then(|s| s.show_remaining)
        .unwrap_or(true);
    let init_rel_x = saved.as_ref().map(|s| s.anchor_x).unwrap_or(0);

    let app_state = Arc::new(AppState {
        cached_token: Mutex::new(None),
        cached_project_id: Mutex::new(None),
        cached_oauth_client: Mutex::new(None),
        cached_user_agent: Mutex::new(None),
        http,
    });

    // Single-threaded background Tokio runtime for HTTP requests (minimal memory footprint)
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("TOKIO_RUNTIME_INIT_FAILED");
    let rt_handle = rt.handle().clone();

    std::thread::Builder::new()
        .name("agy-net".to_string())
        .spawn(move || {
            rt.block_on(async {
                loop {
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                }
            });
        })
        .expect("NET_THREAD_SPAWN_FAILED");

    ui_win32::run_native_win32_ui(
        app_state,
        rt_handle,
        init_rel_x,
        init_opacity,
        init_locale,
        init_show_rem,
    );
}
