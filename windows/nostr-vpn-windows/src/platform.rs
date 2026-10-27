// Windows platform integration ported from the WPF shell:
// single-instance mutex + deep-link pipe (SingleInstanceService),
// Run-key startup and nvpn:// registration (StartupService),
// registry preferences (AppViewModel.State), clipboard, shell open,
// and the roster e2e trace hook.

use std::path::{Path, PathBuf};

use winreg::{enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE}, RegKey};
use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_WRITE, GetLastError, INVALID_HANDLE_VALUE, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FlushFileBuffers, OPEN_EXISTING, PIPE_ACCESS_INBOUND, ReadFile, WriteFile,
    FILE_SHARE_READ,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_ACCEPT_REMOTE_CLIENTS,
    PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES,
};
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use windows_sys::Win32::UI::Shell::ShellExecuteW;

pub const MUTEX_NAME: &str = "Local\\NostrVpn.Windows";
pub const PIPE_NAME: &str = "\\\\.\\pipe\\NostrVpn.Windows.DeepLinks";

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const PROTOCOL_KEY: &str = "Software\\Classes\\nvpn";
const PREFS_KEY: &str = "Software\\Nostr VPN";
const APP_NAME: &str = "Nostr VPN";

pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("NVPN_APP_DATA_DIR")
        .map(PathBuf::from)
        .filter(|dir| !dir.as_os_str().is_empty())
    {
        return dir;
    }
    let appdata = std::env::var("APPDATA")
        .unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into_owned());
    Path::new(&appdata).join(APP_NAME)
}

pub fn trace_roster_e2e(message: &str) {
    let Some(path) = std::env::var("NVPN_ROSTER_E2E_TRACE_PATH")
        .ok()
        .filter(|value| !value.trim().is_empty())
    else {
        return;
    };
    use std::io::Write;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0);
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{secs} {message}");
    }
}

/// Claims the single-instance mutex, or forwards the launch args to the
/// running instance and reports that another instance won.
pub fn claim_or_signal(args: &[String]) -> Result<SingleInstance, ()> {
    let wide = to_wide(MUTEX_NAME);
    let handle = unsafe { CreateMutexW(std::ptr::null(), 1, wide.as_ptr()) };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(());
    }
    // WAIT_OBJECT_0 and WAIT_ABANDONED both leave us owning the mutex;
    // WAIT_TIMEOUT means another instance already holds it.
    if unsafe { WaitForSingleObject(handle, 0) } == WAIT_TIMEOUT {
        unsafe { CloseHandle(handle) };
        signal_primary(args);
        return Err(());
    }
    Ok(SingleInstance { mutex: handle })
}

pub struct SingleInstance {
    mutex: *mut std::ffi::c_void,
}

// Only the raw HANDLE crosses threads; the listening loop is the sole owner
// after the hand-off.
unsafe impl Send for SingleInstance {}

impl SingleInstance {
    /// Blocking listener loop; run on its own thread. Each received payload is
    /// a JSON array of launch args, matching the WPF pipe protocol.
    pub fn listen<F: Fn(Vec<String>) + Send + 'static>(&self, on_launch_args: F) {
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: 0,
        };
        loop {
            let name = to_wide(PIPE_NAME);
            let server = unsafe {
                CreateNamedPipeW(
                    name.as_ptr(),
                    PIPE_ACCESS_INBOUND,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_ACCEPT_REMOTE_CLIENTS,
                    PIPE_UNLIMITED_INSTANCES,
                    4096,
                    4096,
                    0,
                    &sa,
                )
            };
            if server.is_null() || server == INVALID_HANDLE_VALUE {
                std::thread::sleep(std::time::Duration::from_millis(200));
                continue;
            }
            let connected = unsafe { ConnectNamedPipe(server, std::ptr::null_mut()) };
            // ERROR_PIPE_CONNECTED (532): a client connected between creation
            // and the ConnectNamedPipe call, which still counts as accepted.
            if connected == 0 && unsafe { GetLastError() } != 532 {
                unsafe { CloseHandle(server) };
                std::thread::sleep(std::time::Duration::from_millis(200));
                continue;
            }
            let payload = read_all(server);
            unsafe {
                FlushFileBuffers(server);
                DisconnectNamedPipe(server);
                CloseHandle(server);
            }
            if let Ok(args) = parse_args(&payload) {
                on_launch_args(args);
            }
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.mutex);
            CloseHandle(self.mutex);
        }
    }
}

fn read_all(handle: *mut std::ffi::c_void) -> String {
    let mut buffer = [0u8; 8192];
    let mut out = Vec::new();
    loop {
        let mut read = 0u32;
        let ok = unsafe {
            ReadFile(
                handle,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut read,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 || read == 0 {
            break;
        }
        out.extend_from_slice(&buffer[..read as usize]);
        if out.len() > 1 << 20 {
            break;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn signal_primary(args: &[String]) {
    let payload = args_json(args).into_bytes();
    for _ in 0..8 {
        let client = unsafe {
            CreateFileW(
                to_wide(PIPE_NAME).as_ptr(),
                GENERIC_WRITE,
                FILE_SHARE_READ,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if client != INVALID_HANDLE_VALUE && !client.is_null() {
            let mut written = 0u32;
            unsafe {
                WriteFile(
                    client,
                    payload.as_ptr().cast(),
                    payload.len() as u32,
                    &mut written,
                    std::ptr::null_mut(),
                );
                FlushFileBuffers(client);
                CloseHandle(client);
            }
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn args_json(args: &[String]) -> String {
    let mut json = String::from("[");
    for (index, arg) in args.iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        json.push_str(&quote(arg));
    }
    json.push(']');
    json
}

fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Parses the pipe payload: a JSON array of strings (no serde_json dependency).
fn parse_args(payload: &str) -> Result<Vec<String>, ()> {
    let trimmed = payload.trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .ok_or(())?;
    let mut args = Vec::new();
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        match ch {
            ' ' | ',' | '\r' | '\n' => {}
            '"' => {
                let mut value = String::new();
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some('"') => value.push('"'),
                            Some('\\') => value.push('\\'),
                            Some('/') => value.push('/'),
                            Some('n') => value.push('\n'),
                            Some('r') => value.push('\r'),
                            Some('t') => value.push('\t'),
                            Some('u') => {
                                let hex: String = (0..4).filter_map(|_| chars.next()).collect();
                                let code = u32::from_str_radix(&hex, 16).map_err(|_| ())?;
                                value.push(char::from_u32(code).ok_or(())?);
                            }
                            _ => return Err(()),
                        },
                        Some(other) => value.push(other),
                        None => return Err(()),
                    }
                }
                args.push(value);
            }
            _ => return Err(()),
        }
    }
    Ok(args)
}

pub fn load_auto_install_updates() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(PREFS_KEY, KEY_READ)
        .and_then(|key| key.get_value::<u32, _>("AutoInstallUpdates"))
        .map(|value| value != 0)
        .unwrap_or(false)
}

pub fn save_auto_install_updates(enabled: bool) {
    if let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(PREFS_KEY, KEY_SET_VALUE) {
        let _ = key.set_value("AutoInstallUpdates", &(if enabled { 1u32 } else { 0u32 }));
    }
}

/// 0 follow the system, 1 light, 2 dark.
pub fn load_theme_pref() -> i32 {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(PREFS_KEY, KEY_READ)
        .and_then(|key| key.get_value::<u32, _>("Theme"))
        .map(|value| (value as i32).clamp(0, 2))
        .unwrap_or(0)
}

pub fn save_theme_pref(pref: i32) {
    if let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(PREFS_KEY, KEY_SET_VALUE) {
        let _ = key.set_value("Theme", &(pref.clamp(0, 2) as u32));
    }
}

pub fn set_launch_on_startup(enabled: bool) -> Result<(), String> {
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)
        .map_err(|error| format!("could not open the Run registry key: {error}"))?;
    if enabled {
        key.set_value(APP_NAME, &startup_command())
            .map_err(|error| format!("could not write the startup entry: {error}"))?;
    } else {
        let _ = key.delete_value(APP_NAME);
    }
    Ok(())
}

pub fn sync_launch_on_startup(state_supported: bool, state_enabled: bool) {
    if let Err(error) = set_launch_on_startup(state_supported && state_enabled) {
        eprintln!("startup registration failed: {error}");
    }
}

fn startup_command() -> String {
    format!("\"{}\" --hidden", current_exe().display())
}

pub fn register_deep_link_protocol() {
    let Ok((key, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(PROTOCOL_KEY) else {
        return;
    };
    let _ = key.set_value("", &"URL:Nostr VPN");
    let _ = key.set_value("URL Protocol", &"");
    if let Ok((command, _)) = key.create_subkey(r"shell\open\command") {
        let _ = command.set_value("", &format!("\"{}\" \"%1\"", current_exe().display()));
    }
}

pub fn current_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("nostr-vpn-gui.exe"))
}

pub fn shell_open(target: &str) {
    let target = to_wide(target);
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            std::ptr::null(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            5, // SW_SHOW
        );
    }
}

pub fn copy_to_clipboard(value: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(value.to_string()))
        .map_err(|error| error.to_string())
}

pub fn open_file_dialog(title: &str, filter: &[(&str, &[&str])]) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title(title);
    for &(name, extensions) in filter {
        dialog = dialog.add_filter(name, extensions);
    }
    dialog.pick_file()
}

fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
