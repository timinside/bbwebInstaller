#![windows_subsystem = "windows"]

use std::collections::HashSet;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::iter::once;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::ptr::null_mut;
use std::thread;

use serde::Deserialize;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, DEFAULT_CHARSET, FW_NORMAL, HBRUSH, HFONT, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
    CLEARTYPE_QUALITY, DEFAULT_PITCH, FF_DONTCARE, COLOR_WINDOW,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Shell::{Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, NIIF_INFO};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW, FindWindowW,
    GetCursorPos, GetDlgItem, GetMessageW, InsertMenuW, IsIconic, KillTimer, LoadCursorW, LoadIconW,
    MessageBoxW, OpenIcon, PostQuitMessage, RegisterClassW, SendMessageW, SetForegroundWindow,
    SetTimer, SetWindowTextW, ShowWindow, TrackPopupMenu, TranslateMessage, CB_ADDSTRING, CB_GETCURSEL,
    CB_RESETCONTENT, CB_SETCURSEL, CBS_DROPDOWNLIST, ES_AUTOVSCROLL, ES_MULTILINE, ES_READONLY,
    HMENU, IDC_ARROW, IDI_APPLICATION, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MF_STRING, MSG,
    SW_HIDE, SW_RESTORE, SW_SHOW, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_LBUTTONDBLCLK, WM_NULL, WM_RBUTTONUP,
    WM_SETFONT, WM_SETTEXT, WM_TIMER, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_MINIMIZEBOX, WS_OVERLAPPED,
    WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

const APP_NAME: &str = "bbweb Installer";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const MANIFEST_URL: &str = "https://raw.githubusercontent.com/timinside/bbwebInstaller/refs/heads/pages/manifest.json";
const CREATE_NO_WINDOW: u32 = 0x08000000;

const ID_PROJECT: i32 = 1001;
const ID_INSTALL: i32 = 1003;
const ID_STATUS: i32 = 1004;
const ID_UPDATE_ALL: i32 = 1007;
const ID_UPDATE_MANIFEST: i32 = 1008;
const ID_TRAY_UPDATE: i32 = 2001;
const ID_TRAY_SHOW: i32 = 2002;
const ID_TRAY_EXIT: i32 = 2003;
const WM_TRAYICON: u32 = 0x8000 + 1;

#[derive(Debug, Deserialize, Clone)]
struct Manifest {
    installer: InstallerInfo,
    projects: Vec<Project>,
}

#[derive(Debug, Deserialize, Clone)]
struct InstallerInfo {
    version: String,
    download_url: String,
}

#[derive(Debug, Deserialize, Clone)]
struct Project {
    id: String,
    name: String,
    version: Option<String>,
    url: String,
    file: String,
    #[serde(default)]
    args: Vec<String>,
    installed_version_path: Option<String>,
    latest_version_url: Option<String>,
}

static mut PROJECTS: Vec<Project> = Vec::new();
static mut PROJECT_LIST: Vec<String> = Vec::new();
static mut INSTALLER_INFO: Option<InstallerInfo> = None;
static mut UPDATE_AVAILABLE: bool = false;
static mut STARTUP_LOG: String = String::new();
static mut PROGRAM_UPDATES: Vec<Project> = Vec::new();
static mut LAUNCH_ARGS: Vec<String> = Vec::new();
static mut SERVICE_MODE: bool = false;
static mut NO_CHECK: bool = false;
static mut LOG_PATH: Option<PathBuf> = None;
static mut MAIN_HWND: HWND = HWND(std::ptr::null_mut());
static mut SERVICE_BALLOON_ACTIVE: bool = false;

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(once(0)).collect()
}

fn pcw(s: &Vec<u16>) -> PCWSTR {
    PCWSTR(s.as_ptr())
}

fn show_error(text: &str) {
    let t = to_wide(text);
    let c = to_wide(APP_NAME);
    unsafe { let _ = MessageBoxW(HWND(std::ptr::null_mut()), pcw(&t), pcw(&c), MB_OK | MB_ICONERROR); }
}

fn show_info(text: &str) {
    let t = to_wide(text);
    let c = to_wide(APP_NAME);
    unsafe { let _ = MessageBoxW(HWND(std::ptr::null_mut()), pcw(&t), pcw(&c), MB_OK | MB_ICONINFORMATION); }
}

fn app_dir() -> Result<PathBuf, String> {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| env::var_os("TEMP").map(PathBuf::from))
        .ok_or_else(|| "Could not determine app directory".to_string())?;
    let dir = base.join("bbwebInstaller");
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create {}: {e}", dir.display()))?;
    Ok(dir)
}

fn downloads_dir() -> Result<PathBuf, String> {
    let dir = app_dir()?.join("downloads");
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create {}: {e}", dir.display()))?;
    Ok(dir)
}

fn read_text_file(path: &std::path::Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))
}

fn fetch_text(url: &str, dest_name: &str) -> Result<String, String> {
    let dir = app_dir()?;
    let dest = dir.join(dest_name);
    let status = Command::new("curl.exe")
        .args(["-L", "--fail", "--silent", "--show-error", "-o"])
        .arg(&dest)
        .arg(url)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("Failed to run curl.exe: {e}"))?;
    if !status.success() {
        return Err(format!("Failed to fetch: {url}"));
    }
    read_text_file(&dest)
}

fn load_manifest_source(source: Option<&str>) -> Result<Manifest, String> {
    let data = if let Some(src) = source {
        if src.starts_with("http://") || src.starts_with("https://") {
            fetch_text(src, "manifest_override.json")?
        } else {
            read_text_file(&PathBuf::from(src))?
        }
    } else if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            let local = dir.join("manifest.json");
            if local.exists() {
                read_text_file(&local)?
            } else {
                fetch_text(MANIFEST_URL, "manifest.json")?
            }
        } else {
            fetch_text(MANIFEST_URL, "manifest.json")?
        }
    } else {
        fetch_text(MANIFEST_URL, "manifest.json")?
    };
    serde_json::from_str(&data).map_err(|e| format!("Failed to parse manifest: {e}"))
}

fn normalize_ver(s: &str) -> String {
    s.trim().trim_start_matches('v').trim_start_matches('V').to_string()
}

fn resolve_latest_version(p: &Project) -> Option<String> {
    let Some(ver) = &p.version else { return None; };
    let v = ver.trim();
    if v.starts_with("http://") || v.starts_with("https://") {
        fetch_text(v, &format!("ver_{}.txt", p.id.replace(|c: char| !c.is_alphanumeric(), "_")))
            .ok()
            .map(|s| normalize_ver(&s))
            .filter(|s| !s.is_empty())
    } else {
        let n = normalize_ver(v);
        if n.is_empty() { None } else { Some(n) }
    }
}

#[derive(Clone)]
struct AppStatus {
    name: String,
    installed: String,
    latest: Option<String>,
    update: bool,
}

fn scan_programs(manifest: &Manifest) -> (Vec<AppStatus>, Vec<Project>) {
    let mut results = Vec::new();
    let mut update_projects = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for p in &manifest.projects {
        let key = p.installed_version_path.clone().unwrap_or_else(|| p.id.clone());
        if !seen.insert(key.clone()) {
            continue;
        }
        let installed = if let Some(path) = &p.installed_version_path {
            fs::read_to_string(path).ok().map(|s| normalize_ver(&s)).filter(|s| !s.is_empty())
        } else {
            None
        };
        let Some(installed) = installed else { continue; };
        let latest = resolve_latest_version(p);
        let update = latest.as_ref().map(|l| l != &installed).unwrap_or(false);
        if update {
            update_projects.push(p.clone());
        }
        results.push(AppStatus { name: p.name.clone(), installed, latest, update });
    }
    (results, update_projects)
}

fn format_scan_log(apps: &[AppStatus]) -> String {
    let mut log = String::from("Installed apps:");
    if apps.is_empty() {
        log.push_str("\r\n  (none found)");
    } else {
        for a in apps {
            if a.update {
                let latest = a.latest.clone().unwrap_or_else(|| "?".to_string());
                log.push_str(&format!("\r\n  - {}: {} -> {}", a.name, a.installed, latest));
            } else if let Some(latest) = &a.latest {
                log.push_str(&format!("\r\n  - {}: {} ({})", a.name, a.installed, latest));
            } else {
                log.push_str(&format!("\r\n  - {}: {}", a.name, a.installed));
            }
        }
    }
    log
}

fn project_display_name(p: &Project, apps: &[AppStatus]) -> String {
    let status = apps.iter().find(|a| {
        p.installed_version_path.as_ref().map(|path| a.name == p.name || p.installed_version_path.as_ref().map(|_| true).unwrap_or(false) && fs::read_to_string(path).ok().map(|s| normalize_ver(&s)).as_ref() == Some(&a.installed)).unwrap_or(false)
            || a.name == p.name
    });
    if let Some(a) = status {
        if a.update {
            let latest = a.latest.clone().unwrap_or_else(|| "?".to_string());
            return format!("{} ({} -> {})", p.name, a.installed, latest);
        } else if let Some(latest) = &a.latest {
            return format!("{} (installed: {}, {})", p.name, a.installed, latest);
        } else {
            return format!("{} (installed: {})", p.name, a.installed);
        }
    }
    if let Some(v) = resolve_latest_version(p) {
        format!("{} ({})", p.name, v)
    } else {
        p.name.clone()
    }
}

fn parse_args() -> (Option<String>, Option<String>, bool, bool, bool, bool, bool, Option<String>, Option<String>, bool) {
    let args: Vec<String> = env::args().collect();
    let mut manifest_url = None;
    let mut product_check = None;
    let mut installed = false;
    let mut service = false;
    let mut update_all = false;
    let mut logs = false;
    let mut nocheck = false;
    let mut check_update = None;
    let mut update_product = None;
    let mut help = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--manifest-url" => { i += 1; manifest_url = args.get(i).cloned(); }
            "--product-check" => { i += 1; product_check = args.get(i).cloned(); }
            "--installed" => installed = true,
            "--service" => service = true,
            "--update-all" => update_all = true,
            "--logs" => logs = true,
            "--nocheck" => nocheck = true,
            "--check-update" => { i += 1; check_update = args.get(i).cloned(); }
            "--update-product" => { i += 1; update_product = args.get(i).cloned(); }
            "--help" | "-h" | "/?" => help = true,
            _ => {}
        }
        i += 1;
    }
    (manifest_url, product_check, installed, service, update_all, logs, nocheck, check_update, update_product, help)
}

fn show_help() {
    let text = "\
bbweb Installer

Usage:
  bbwebinstaller.exe [options]

Options:
  --manifest-url <url|path>     Override manifest source
  --logs                        Write logs to C:\\Users\\<user>\\.bbweb\\logs\\bbwebinstaller.log
  --nocheck                     Suppress update notifications
  --installed                   Installed-apps context (scan/update/service flows)
  --service                     Service check mode (with --installed): balloon if updates, else exit
  --check-update <id|name>      Check update for a project (with --installed)
                                Exit codes: 100 update, 0 none, 3 not installed, 2 not found
  --update-product <id|name>    Install/update only that project (with --installed)
                                Exit codes: 0 success, 1 error, 2 not found
  --update-all                  Update all available projects (with --installed)
  --product-check <id|name>     Show installed/latest info dialog (with --installed)
  --help, -h, /?                Show this help";
    show_info(text);
}

fn init_logs(enabled: bool) {
    if !enabled { return; }
    if let Some(home) = env::var_os("USERPROFILE") {
        let dir = PathBuf::from(home).join(".bbweb").join("logs");
        if fs::create_dir_all(&dir).is_ok() {
            let path = dir.join("bbwebinstaller.log");
            unsafe { LOG_PATH = Some(path.clone()); }
            write_log(&format!("--- bbweb Installer v{} started ---", CURRENT_VERSION));
        }
    }
}

fn write_log(msg: &str) {
    unsafe {
        if let Some(path) = &LOG_PATH {
            use std::io::Write;
            if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(path) {
                let _ = writeln!(f, "{}", msg.replace("\r\n", "\n"));
            }
        }
    }
}

fn relaunch_args() -> Vec<String> {
    unsafe { LAUNCH_ARGS.clone() }
}

fn set_status(hwnd: isize, text: &str) {
    let w = to_wide(text);
    unsafe {
        let _ = SendMessageW(HWND(hwnd as *mut _), WM_SETTEXT, WPARAM(0), LPARAM(w.as_ptr() as isize));
    }
}

fn format_progress(name: &str, chunk: &str) -> Option<String> {
    let s = chunk.replace('\r', " ").replace('\n', " ");
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() >= 4 && parts[0].parse::<f32>().is_ok() {
        let pct = parts[0];
        let total = parts[1];
        let recv = parts[3];
        if pct != "100" || !recv.is_empty() {
            return Some(format!("Downloading {}... {}% ({}/{})", name, pct, recv, total));
        }
    }
    if let Some(p) = s.split_whitespace().find(|t| t.ends_with('%')) {
        return Some(format!("Downloading {}... {}", name, p));
    }
    None
}

fn install_project(p: Project, status_hwnd: isize) -> Result<String, String> {
    let dir = downloads_dir()?;
    let dest = dir.join(&p.file);
    let mut log = String::from(format!("Downloading {}...", p.name));
    set_status(status_hwnd, &log);

    let mut child = Command::new("curl.exe")
        .args(["-L", "--fail", "--retry", "3", "--retry-delay", "2", "-o"])
        .arg(&dest)
        .arg(&p.url)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run curl.exe: {e}"))?;

    if let Some(mut stderr) = child.stderr.take() {
        let mut buf = [0u8; 256];
        let mut acc = String::new();
        while let Ok(n) = stderr.read(&mut buf) {
            if n == 0 { break; }
            let chunk = String::from_utf8_lossy(&buf[..n]);
            acc.push_str(&chunk);
            let part = acc.rsplit(|c| c == '\r' || c == '\n').next().unwrap_or("").to_string();
            if let Some(msg) = format_progress(&p.name, &part) {
                log = msg;
                if status_hwnd != 0 { set_status(status_hwnd, &log); }
            }
            if acc.len() > 1024 {
                acc = part.chars().take(512).collect();
            }
        }
    }

    let status = child.wait().map_err(|e| format!("curl wait failed: {e}"))?;
    if !status.success() {
        let _ = fs::remove_file(&dest);
        let err = format!("Download failed for {} (curl exit: {status})", p.name);
        if status_hwnd != 0 { set_status(status_hwnd, &err); }
        write_log(&err);
        return Err(err);
    }

    log = format!("Downloading {}... done\r\nRunning installer...", p.name);
    if status_hwnd != 0 { set_status(status_hwnd, &log); }
    let status = Command::new(&dest)
        .args(&p.args)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("Failed to start installer: {e}"))?;
    if !status.success() {
        let err = format!("Installer for {} exited with status: {status}", p.name);
        if status_hwnd != 0 { set_status(status_hwnd, &err); }
        write_log(&err);
        return Err(err);
    }
    log.push_str(&format!("\r\nInstalled {}", p.name));
    if status_hwnd != 0 { set_status(status_hwnd, &log); }
    Ok(format!("Installed {}", p.name))
}

fn self_update(download_url: String, status_hwnd: isize, restart_args: Vec<String>) -> Result<String, String> {
    let current_exe = env::current_exe().map_err(|e| format!("Cannot locate current exe: {e}"))?;
    let dir = app_dir()?;
    let new_exe = dir.join("bbwebinstaller_update.exe");
    let mut log = String::from("Downloading bbweb Installer update...");
    if status_hwnd != 0 { set_status(status_hwnd, &log); }

    let mut child = Command::new("curl.exe")
        .args(["-L", "--fail", "--retry", "3", "--retry-delay", "2", "-o"])
        .arg(&new_exe)
        .arg(&download_url)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run curl.exe: {e}"))?;

    if let Some(mut stderr) = child.stderr.take() {
        let mut buf = [0u8; 256];
        let mut acc = String::new();
        while let Ok(n) = stderr.read(&mut buf) {
            if n == 0 { break; }
            let chunk = String::from_utf8_lossy(&buf[..n]);
            acc.push_str(&chunk);
            let part = acc.rsplit(|c| c == '\r' || c == '\n').next().unwrap_or("").to_string();
            if let Some(msg) = format_progress("bbweb Installer", &part) {
                log = msg;
                if status_hwnd != 0 { set_status(status_hwnd, &log); }
            }
            if acc.len() > 1024 {
                acc = part.chars().take(512).collect();
            }
        }
    }

    let status = child.wait().map_err(|e| format!("curl wait failed: {e}"))?;
    if !status.success() {
        let _ = fs::remove_file(&new_exe);
        let err = format!("Update download failed (curl exit: {status})");
        if status_hwnd != 0 { set_status(status_hwnd, &err); }
        write_log(&err);
        return Err(err);
    }

    if status_hwnd != 0 { set_status(status_hwnd, "Download complete. Replacing executable..."); }
    let old_exe = current_exe.with_extension("old.exe");
    let _ = fs::remove_file(&old_exe);
    fs::rename(&current_exe, &old_exe).map_err(|e| format!("Failed to replace current exe: {e}"))?;
    if let Err(e) = fs::copy(&new_exe, &current_exe) {
        let _ = fs::rename(&old_exe, &current_exe);
        return Err(format!("Failed to install new exe: {e}"));
    }
    let _ = fs::remove_file(&new_exe);

    Command::new(&current_exe)
        .args(&restart_args)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Updated but failed to restart: {e}"))?;
    Ok("Updated. Restarting...".to_string())
}

fn get_combo_sel(hwnd: HWND) -> i32 {
    unsafe { SendMessageW(hwnd, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 as i32 }
}

fn set_combo_sel(hwnd: HWND, idx: i32) {
    unsafe { SendMessageW(hwnd, CB_SETCURSEL, WPARAM(idx as usize), LPARAM(0)); }
}

fn add_combo_item(hwnd: HWND, text: &str) {
    let w = to_wide(text);
    unsafe { SendMessageW(hwnd, CB_ADDSTRING, WPARAM(0), LPARAM(w.as_ptr() as isize)); }
}

fn copy_wide(dst: &mut [u16], src: &str) {
    let w = to_wide(src);
    let n = std::cmp::min(w.len(), dst.len());
    dst[..n].copy_from_slice(&w[..n]);
}

fn tray_add(hwnd: HWND) {
    unsafe {
        let hinstance = HINSTANCE(GetModuleHandleW(None).unwrap_or_default().0);
        let icon = LoadIconW(hinstance, PCWSTR(1 as *const u16)).unwrap_or_else(|_| LoadIconW(None, IDI_APPLICATION).unwrap_or_default());
        let mut nid = NOTIFYICONDATAW::default();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_TRAYICON;
        nid.hIcon = icon;
        copy_wide(&mut nid.szTip, APP_NAME);
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &nid);
    }
}

fn tray_balloon(hwnd: HWND, title: &str, info: &str) {
    unsafe {
        let mut nid = NOTIFYICONDATAW::default();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_INFO;
        nid.dwInfoFlags = NIIF_INFO;
        copy_wide(&mut nid.szInfoTitle, title);
        copy_wide(&mut nid.szInfo, info);
        let _ = Shell_NotifyIconW(windows::Win32::UI::Shell::NIM_MODIFY, &nid);
    }
}

fn tray_remove(hwnd: HWND) {
    unsafe {
        let mut nid = NOTIFYICONDATAW::default();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

fn start_update_all(hwnd: HWND) {
    unsafe {
        if SERVICE_MODE {
            tray_remove(hwnd);
            let _ = KillTimer(hwnd, 1);
        }
        SERVICE_BALLOON_ACTIVE = false;
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }
    let status_hwnd = unsafe { GetDlgItem(hwnd, ID_STATUS).unwrap_or(HWND(std::ptr::null_mut())) };
    let program_updates = unsafe { PROGRAM_UPDATES.clone() };
    let installer_update = unsafe { UPDATE_AVAILABLE };
    if program_updates.is_empty() && !installer_update {
        unsafe { let _ = SetWindowTextW(status_hwnd, pcw(&to_wide("No updates available."))); }
        return;
    }
    let status_hwnd_raw = status_hwnd.0 as isize;
    let installer_url = unsafe { INSTALLER_INFO.as_ref().map(|i| i.download_url.clone()) };
    let args = relaunch_args();
    unsafe { let _ = SetWindowTextW(status_hwnd, pcw(&to_wide("Starting updates..."))); }
    write_log("Starting Update all.");
    thread::spawn(move || {
        for p in program_updates {
            write_log(&format!("Updating {}", p.name));
            let _ = install_project(p, status_hwnd_raw);
        }
        if installer_update {
            if let Some(url) = installer_url {
                let _ = self_update(url, status_hwnd_raw, args);
                unsafe { PostQuitMessage(0); }
            }
        } else {
            set_status(status_hwnd_raw, "All updates completed.");
            write_log("Update all completed.");
        }
    });
}

fn apply_font(hwnd: HWND, font: HFONT) {
    unsafe { SendMessageW(hwnd, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1)); }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            let hinstance = HINSTANCE(GetModuleHandleW(None).unwrap_or_default().0);
            let static_class = to_wide("STATIC");
            let combo_class = to_wide("COMBOBOX");
            let button_class = to_wide("BUTTON");
            let edit_class = to_wide("EDIT");
            let font = CreateFontW(20, 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32, pcw(&to_wide("Segoe UI")));

            let label = CreateWindowExW(WINDOW_EX_STYLE(0), pcw(&static_class), pcw(&to_wide("Project")), WS_CHILD | WS_VISIBLE, 24, 20, 120, 24, hwnd, HMENU(std::ptr::null_mut()), hinstance, None).unwrap_or(HWND(std::ptr::null_mut()));
            apply_font(label, font);

            let project = CreateWindowExW(WINDOW_EX_STYLE(0), pcw(&combo_class), PCWSTR::null(), WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWNLIST as u32), 24, 48, 440, 300, hwnd, HMENU(ID_PROJECT as *mut _), hinstance, None).unwrap_or(HWND(std::ptr::null_mut()));
            for name in PROJECT_LIST.iter() { add_combo_item(project, name); }
            if !PROJECT_LIST.is_empty() { set_combo_sel(project, 0); }
            apply_font(project, font);

            let install = CreateWindowExW(WINDOW_EX_STYLE(0), pcw(&button_class), pcw(&to_wide("Install")), WS_CHILD | WS_VISIBLE | WS_TABSTOP, 24, 96, 140, 36, hwnd, HMENU(ID_INSTALL as *mut _), hinstance, None).unwrap_or(HWND(std::ptr::null_mut()));
            apply_font(install, font);

            let any_updates = UPDATE_AVAILABLE || !PROGRAM_UPDATES.is_empty();
            let update_all = CreateWindowExW(WINDOW_EX_STYLE(0), pcw(&button_class), pcw(&to_wide(if any_updates { "Update all" } else { "No updates" })), WS_CHILD | WS_VISIBLE | WS_TABSTOP, 176, 96, 140, 36, hwnd, HMENU(ID_UPDATE_ALL as *mut _), hinstance, None).unwrap_or(HWND(std::ptr::null_mut()));
            apply_font(update_all, font);

            let upd_manifest = CreateWindowExW(WINDOW_EX_STYLE(0), pcw(&button_class), pcw(&to_wide("Update manifest")), WS_CHILD | WS_VISIBLE | WS_TABSTOP, 324, 96, 140, 36, hwnd, HMENU(ID_UPDATE_MANIFEST as *mut _), hinstance, None).unwrap_or(HWND(std::ptr::null_mut()));
            apply_font(upd_manifest, font);

            let initial = if STARTUP_LOG.is_empty() {
                "Select a project and click Install.".to_string()
            } else {
                format!("{}\r\n\r\nSelect a project and click Install.", STARTUP_LOG)
            };
            let status = CreateWindowExW(
                WINDOW_EX_STYLE(0x00000200),
                pcw(&edit_class),
                pcw(&to_wide(&initial)),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32 | ES_READONLY as u32),
                24,
                148,
                440,
                170,
                hwnd,
                HMENU(ID_STATUS as *mut _),
                hinstance,
                None,
            ).unwrap_or(HWND(std::ptr::null_mut()));
            apply_font(status, font);
            let updates: Vec<String> = unsafe { PROGRAM_UPDATES.iter().map(|p| p.name.clone()).collect() };
            let any_updates = unsafe { UPDATE_AVAILABLE || !PROGRAM_UPDATES.is_empty() };
            let no_check = unsafe { NO_CHECK };
            if SERVICE_MODE {
                let _ = ShowWindow(hwnd, SW_HIDE);
                if !no_check && any_updates {
                    tray_add(hwnd);
                    unsafe { SERVICE_BALLOON_ACTIVE = true; }
                    let title = if updates.is_empty() {
                        "bbweb Installer update available".to_string()
                    } else if updates.len() == 1 {
                        format!("{} has new update available", updates[0])
                    } else if updates.len() == 2 {
                        format!("{} and {} have new updates available", updates[0], updates[1])
                    } else {
                        format!("{}, {} and more have new updates available", updates[0], updates[1])
                    };
                    tray_balloon(hwnd, &title, "Ready to download. Click to update all.");
                    write_log(&format!("Notification: {}", title));
                } else {
                    PostQuitMessage(0);
                }
            } else if !no_check && any_updates {
                tray_add(hwnd);
                let title = if updates.is_empty() {
                    "bbweb Installer update available".to_string()
                } else if updates.len() == 1 {
                    format!("{} has new update available", updates[0])
                } else if updates.len() == 2 {
                    format!("{} and {} have new updates available", updates[0], updates[1])
                } else {
                    format!("{}, {} and more have new updates available", updates[0], updates[1])
                };
                tray_balloon(hwnd, &title, "Ready to download. Click to update all.");
                write_log(&format!("Notification: {}", title));
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            if SERVICE_MODE { let _ = KillTimer(hwnd, 1); }
            let id = (wparam.0 & 0xffff) as i32;
            let status_hwnd = GetDlgItem(hwnd, ID_STATUS).unwrap_or(HWND(std::ptr::null_mut()));
            if id == ID_INSTALL {
                let project_hwnd = GetDlgItem(hwnd, ID_PROJECT).unwrap_or(HWND(std::ptr::null_mut()));
                let psel = get_combo_sel(project_hwnd);
                if psel < 0 || (psel as usize) >= PROJECTS.len() { return LRESULT(0); }
                let project = PROJECTS[psel as usize].clone();
                let status_hwnd_raw = status_hwnd.0 as isize;
                let _ = SetWindowTextW(status_hwnd, pcw(&to_wide("Starting...")));
                thread::spawn(move || {
                    let result = install_project(project, status_hwnd_raw);
                    match result {
                        Ok(msg) => {
                            let t = to_wide(&msg);
                            let c = to_wide(APP_NAME);
                            let _ = MessageBoxW(HWND(std::ptr::null_mut()), pcw(&t), pcw(&c), MB_OK | MB_ICONINFORMATION);
                        }
                        Err(e) => {
                            let t = to_wide(&e);
                            let c = to_wide(APP_NAME);
                            let _ = MessageBoxW(HWND(std::ptr::null_mut()), pcw(&t), pcw(&c), MB_OK | MB_ICONERROR);
                        }
                    }
                });
            } else if id == ID_UPDATE_ALL || id == ID_TRAY_UPDATE {
                start_update_all(hwnd);
            } else if id == ID_TRAY_SHOW {
                if SERVICE_MODE { let _ = KillTimer(hwnd, 1); }
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = SetForegroundWindow(hwnd);
            } else if id == ID_TRAY_EXIT {
                PostQuitMessage(0);
            } else if id == ID_UPDATE_MANIFEST {
                let _ = SetWindowTextW(status_hwnd, pcw(&to_wide("Updating manifest...")));
                let manifest_url = unsafe { LAUNCH_ARGS.windows(2).find(|w| w[0] == "--manifest-url").map(|w| w[1].clone()) };
                match load_manifest_source(manifest_url.as_deref()) {
                    Ok(manifest) => {
                        let installer_update = manifest.installer.version != CURRENT_VERSION;
                        let (apps, program_updates) = scan_programs(&manifest);
                        let mut startup_log = if installer_update {
                            format!("bbweb Installer v{} -> v{}", CURRENT_VERSION, manifest.installer.version)
                        } else {
                            format!("bbweb Installer v{}", CURRENT_VERSION)
                        };
                        let scan_log = format_scan_log(&apps);
                        if !scan_log.is_empty() {
                            startup_log.push_str("\r\n\r\n");
                            startup_log.push_str(&scan_log);
                        }
                        let project_list: Vec<String> = manifest.projects.iter().map(|p| project_display_name(p, &apps)).collect();
                        unsafe {
                            UPDATE_AVAILABLE = installer_update;
                            INSTALLER_INFO = Some(manifest.installer.clone());
                            PROJECTS = manifest.projects;
                            PROJECT_LIST = project_list;
                            STARTUP_LOG = startup_log.clone();
                            PROGRAM_UPDATES = program_updates;
                        }
                        let project_hwnd = GetDlgItem(hwnd, ID_PROJECT).unwrap_or(HWND(std::ptr::null_mut()));
                        unsafe { SendMessageW(project_hwnd, windows::Win32::UI::WindowsAndMessaging::CB_RESETCONTENT, WPARAM(0), LPARAM(0)); }
                        for name in unsafe { PROJECT_LIST.iter() } { add_combo_item(project_hwnd, name); }
                        if !unsafe { PROJECT_LIST.is_empty() } { set_combo_sel(project_hwnd, 0); }
                        let any_updates = unsafe { UPDATE_AVAILABLE || !PROGRAM_UPDATES.is_empty() };
                        let update_all_hwnd = GetDlgItem(hwnd, ID_UPDATE_ALL).unwrap_or(HWND(std::ptr::null_mut()));
                        let _ = SetWindowTextW(update_all_hwnd, pcw(&to_wide(if any_updates { "Update all" } else { "No updates" })));
                        let _ = SetWindowTextW(status_hwnd, pcw(&to_wide(&format!("{}\r\n\r\nManifest updated. Select a project and click Install.", startup_log))));
                    }
                    Err(_) => {
                        let _ = SetWindowTextW(status_hwnd, pcw(&to_wide("Failed to update manifest.")));
                    }
                }
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == 1 {
                let _ = KillTimer(hwnd, 1);
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        msg if msg == WM_TRAYICON => {
            let l = (lparam.0 & 0xffff) as u32;
            if l == 0x0405 { // NIN_BALLOONUSERCLICK
                unsafe { SERVICE_BALLOON_ACTIVE = false; }
                start_update_all(hwnd);
            } else if l == 0x0404 || l == 0x0407 { // NIN_BALLOONTIMEOUT / NIN_BALLOONHIDE
                if unsafe { SERVICE_MODE && SERVICE_BALLOON_ACTIVE } {
                    unsafe { SERVICE_BALLOON_ACTIVE = false; }
                    tray_remove(hwnd);
                    PostQuitMessage(0);
                }
            } else if l == WM_LBUTTONDBLCLK || l == 0x0203 {
                unsafe { SERVICE_BALLOON_ACTIVE = false; }
                start_update_all(hwnd);
            } else if l == 0x0202 || l == 0x0201 {
                unsafe { SERVICE_BALLOON_ACTIVE = false; }
                if SERVICE_MODE { let _ = KillTimer(hwnd, 1); }
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = SetForegroundWindow(hwnd);
            } else if l == WM_RBUTTONUP || l == 0x0205 {
                unsafe { SERVICE_BALLOON_ACTIVE = false; }
                if SERVICE_MODE { let _ = KillTimer(hwnd, 1); }
                let mut pt = windows::Win32::Foundation::POINT::default();
                let _ = GetCursorPos(&mut pt);
                let menu = CreatePopupMenu().unwrap_or(HMENU(std::ptr::null_mut()));
                let _ = InsertMenuW(menu, 0, MF_STRING, ID_TRAY_UPDATE as usize, pcw(&to_wide("Update all")));
                let _ = InsertMenuW(menu, 1, MF_STRING, ID_TRAY_SHOW as usize, pcw(&to_wide("Show")));
                let _ = InsertMenuW(menu, 2, MF_STRING, ID_TRAY_EXIT as usize, pcw(&to_wide("Exit")));
                let _ = SetForegroundWindow(hwnd);
                let cmd = TrackPopupMenu(menu, TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RETURNCMD, pt.x, pt.y, 0, hwnd, None).0 as i32;
                let _ = SendMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
                let _ = DestroyMenu(menu);
                if cmd == ID_TRAY_UPDATE { start_update_all(hwnd); }
                else if cmd == ID_TRAY_SHOW { let _ = ShowWindow(hwnd, SW_SHOW); let _ = SetForegroundWindow(hwnd); }
                else if cmd == ID_TRAY_EXIT { PostQuitMessage(0); }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            tray_remove(hwnd);
            if SERVICE_MODE { let _ = KillTimer(hwnd, 1); }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn run_app() {
    unsafe {
        let hinstance = HINSTANCE(GetModuleHandleW(None).unwrap_or_default().0);
        let class_name = to_wide("bbwebInstallerWin32");
        let icon = LoadIconW(hinstance, PCWSTR(1 as *const u16)).unwrap_or_else(|_| LoadIconW(None, IDI_APPLICATION).unwrap_or_default());

        let wc = WNDCLASSW {
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hInstance: hinstance,
            hIcon: icon,
            lpszClassName: pcw(&class_name),
            lpfnWndProc: Some(wnd_proc),
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as *mut _),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let show = !SERVICE_MODE;
        let mut style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        if show { style |= WS_VISIBLE; }
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            pcw(&class_name),
            pcw(&to_wide(APP_NAME)),
            style,
            240,
            240,
            510,
            380,
            None,
            None,
            hinstance,
            None,
        )
        .unwrap_or(HWND(std::ptr::null_mut()));

        let _ = ShowWindow(hwnd, if show { SW_SHOW } else { SW_HIDE });

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn main() {
    let args_vec: Vec<String> = env::args().skip(1).collect();
    let (manifest_url, product_check, installed, service, update_all, logs, nocheck, check_update, update_product, help) = parse_args();
    if help {
        show_help();
        return;
    }
    init_logs(logs);
    unsafe { NO_CHECK = nocheck; }

    write_log(&format!("args: {}", args_vec.join(" ")));

    let interactive_gui = !(installed && (service || update_all || check_update.is_some() || update_product.is_some() || product_check.is_some()));
    if interactive_gui {
        unsafe {
            let mutex_name = to_wide("Local\\bbwebInstaller.single");
            let _mutex = CreateMutexW(None, true, pcw(&mutex_name));
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let class_name = to_wide("bbwebInstallerWin32");
                let hwnd = FindWindowW(pcw(&class_name), PCWSTR::null()).unwrap_or(HWND(std::ptr::null_mut()));
                if hwnd.0 != std::ptr::null_mut() {
                    if IsIconic(hwnd).as_bool() { let _ = OpenIcon(hwnd); let _ = ShowWindow(hwnd, SW_RESTORE); }
                    else { let _ = ShowWindow(hwnd, SW_SHOW); }
                    let _ = SetForegroundWindow(hwnd);
                }
                return;
            }
        }
    }
    let manifest = match load_manifest_source(manifest_url.as_deref()) {
        Ok(m) => { write_log("Manifest loaded."); m }
        Err(e) => {
            write_log(&format!("Manifest load failed: {}", e));
            show_error("Launch error: no internet connection or failed to load manifest.");
            return;
        }
    };

    let installer_update = manifest.installer.version != CURRENT_VERSION;

    if installed {
        let (apps, program_updates) = scan_programs(&manifest);
        let scan_log = format_scan_log(&apps);
        write_log(&scan_log);
        if service {
            if installer_update {
                write_log(&format!("Service: self-update {} -> {}", CURRENT_VERSION, manifest.installer.version));
                let _ = self_update(manifest.installer.download_url.clone(), 0, args_vec.clone());
                return;
            }
            if nocheck || program_updates.is_empty() {
                write_log("Service: no updates, exiting.");
                return;
            }
            write_log("Service: updates available, showing balloon.");
            let project_list: Vec<String> = manifest.projects.iter().map(|p| project_display_name(p, &apps)).collect();
            unsafe {
                UPDATE_AVAILABLE = false;
                INSTALLER_INFO = Some(manifest.installer.clone());
                PROJECTS = manifest.projects;
                PROJECT_LIST = project_list;
                STARTUP_LOG = scan_log;
                PROGRAM_UPDATES = program_updates;
                LAUNCH_ARGS = args_vec;
                SERVICE_MODE = true;
            }
            run_app();
            return;
        }
        if update_all {
            write_log("Update all: started.");
            for p in &program_updates {
                write_log(&format!("Update all: updating {}", p.name));
                let _ = install_project(p.clone(), 0);
            }
            if installer_update {
                write_log(&format!("Update all: updating bbweb Installer {} -> {}", CURRENT_VERSION, manifest.installer.version));
                let _ = self_update(manifest.installer.download_url.clone(), 0, args_vec);
                return;
            }
            write_log("Update all: completed.");
            show_info(&format!("{}\r\n\r\nUpdate all completed.", scan_log));
            return;
        }
        if let Some(product) = check_update {
            let needle = product.to_lowercase();
            let app = apps.iter().find(|a| a.name.to_lowercase() == needle || a.name.to_lowercase().contains(&needle));
            if let Some(a) = app {
                if a.update {
                    let latest = a.latest.clone().unwrap_or_else(|| "?".to_string());
                    write_log(&format!("check-update '{}': update available {} -> {}", product, a.installed, latest));
                    std::process::exit(100);
                } else {
                    write_log(&format!("check-update '{}': no update (installed {})", product, a.installed));
                    std::process::exit(0);
                }
            } else {
                // maybe project exists in manifest but not installed; check by manifest entry
                let proj = manifest.projects.iter().find(|p| p.id.to_lowercase() == needle || p.name.to_lowercase() == needle || p.name.to_lowercase().contains(&needle));
                if let Some(p) = proj {
                    let latest = resolve_latest_version(p).unwrap_or_else(|| "?".to_string());
                    write_log(&format!("check-update '{}': not installed, latest {}", product, latest));
                    std::process::exit(3);
                }
                write_log(&format!("check-update '{}': not found", product));
                std::process::exit(2);
            }
        }
        if let Some(product) = update_product {
            let needle = product.to_lowercase();
            let proj = manifest.projects.iter().find(|p| p.id.to_lowercase() == needle || p.name.to_lowercase() == needle || p.name.to_lowercase().contains(&needle)).cloned();
            if let Some(p) = proj {
                write_log(&format!("update-product '{}': starting", product));
                match install_project(p, 0) {
                    Ok(msg) => { write_log(&format!("update-product '{}': {}", product, msg)); std::process::exit(0); }
                    Err(e) => { write_log(&format!("update-product '{}': {}", product, e)); std::process::exit(1); }
                }
            } else {
                write_log(&format!("update-product '{}': not found", product));
                std::process::exit(2);
            }
        }
        if let Some(product) = product_check {
            let needle = product.to_lowercase();
            let filtered: Vec<String> = apps.iter()
                .filter(|a| a.name.to_lowercase().contains(&needle))
                .map(|a| {
                    if a.update {
                        format!("{}: {} -> {}", a.name, a.installed, a.latest.clone().unwrap_or_else(|| "?".to_string()))
                    } else if let Some(latest) = &a.latest {
                        format!("{}: {} ({})", a.name, a.installed, latest)
                    } else {
                        format!("{}: {}", a.name, a.installed)
                    }
                }).collect();
            let msg = if filtered.is_empty() {
                format!("No installed app info found for '{}'.", product)
            } else {
                filtered.join("\r\n")
            };
            write_log(&format!("product-check '{}': {}", product, msg));
            show_info(&msg);
            return;
        }
    }

    let (apps, program_updates) = scan_programs(&manifest);
    let mut startup_log = if installer_update {
        format!("bbweb Installer v{} -> v{}", CURRENT_VERSION, manifest.installer.version)
    } else {
        format!("bbweb Installer v{}", CURRENT_VERSION)
    };
    let scan_log = format_scan_log(&apps);
    if !scan_log.is_empty() {
        startup_log.push_str("\r\n\r\n");
        startup_log.push_str(&scan_log);
    }
    write_log(&startup_log);
    let project_list: Vec<String> = manifest.projects.iter().map(|p| project_display_name(p, &apps)).collect();

    unsafe {
        UPDATE_AVAILABLE = installer_update;
        INSTALLER_INFO = Some(manifest.installer.clone());
        PROJECTS = manifest.projects;
        PROJECT_LIST = project_list;
        STARTUP_LOG = startup_log;
        PROGRAM_UPDATES = program_updates;
        LAUNCH_ARGS = args_vec;
    }
    run_app();
}
