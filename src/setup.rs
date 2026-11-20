#![windows_subsystem = "windows"]
//! Instalador de Centro LED: descarga la última versión desde GitHub y la instala para el
//! usuario actual (sin permisos de administrador), con accesos directos y desinstalador.

#[allow(dead_code)]
#[path = "update.rs"]
mod update;

use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::Com::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const UNINSTALL_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\CentroLED";
const TITLE: &str = "Centro LED";

// Valores de TaskDialog
const TDF_USE_HICON_MAIN: i32 = 0x0002;
const TDF_ALLOW_DIALOG_CANCELLATION: i32 = 0x0008;
const TDF_VERIFICATION_FLAG_CHECKED: i32 = 0x0100;
const TDF_SHOW_MARQUEE_PROGRESS_BAR: i32 = 0x0400;
const TDF_CALLBACK_TIMER: i32 = 0x0800;
const TDCBF_CANCEL: i32 = 0x0008;
const TDCBF_CLOSE: i32 = 0x0020;
const TDN_CREATED: u32 = 0;
const TDN_TIMER: u32 = 4;
const TDM_CLICK_BUTTON: u32 = WM_USER + 102;
const TDM_SET_MARQUEE: u32 = WM_USER + 107;
const ID_INSTALL: i32 = 100;
const ID_OPEN: i32 = 101;

static RESULT: Mutex<Option<Result<PathBuf, String>>> = Mutex::new(None);
static DESKTOP: AtomicBool = AtomicBool::new(true);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn install_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| ".".into());
    base.join("Programs").join("CentroLED")
}

fn desktop_dir() -> Option<PathBuf> {
    unsafe {
        let mut p: windows_sys::core::PWSTR = null_mut();
        if SHGetKnownFolderPath(&FOLDERID_Desktop, 0, null_mut(), &mut p) != 0 {
            return None;
        }
        let mut n = 0;
        while *p.add(n) != 0 {
            n += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
        CoTaskMemFree(p as _);
        Some(PathBuf::from(s))
    }
}

/// Crea un acceso directo (.lnk) con IShellLink (llamadas COM por la tabla de métodos).
fn shortcut(lnk: &Path, target: &Path, workdir: &Path) -> Result<(), String> {
    type SetStr = unsafe extern "system" fn(*mut core::ffi::c_void, *const u16) -> i32;
    type Qi = unsafe extern "system" fn(*mut core::ffi::c_void, *const GUID, *mut *mut core::ffi::c_void) -> i32;
    type Save = unsafe extern "system" fn(*mut core::ffi::c_void, *const u16, BOOL) -> i32;
    type Release = unsafe extern "system" fn(*mut core::ffi::c_void) -> u32;
    unsafe {
        CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32);
        let clsid = GUID::from_u128(0x00021401_0000_0000_c000_000000000046); // ShellLink
        let iid_link = GUID::from_u128(0x000214F9_0000_0000_c000_000000000046); // IShellLinkW
        let iid_file = GUID::from_u128(0x0000010b_0000_0000_c000_000000000046); // IPersistFile
        let mut link: *mut core::ffi::c_void = null_mut();
        if CoCreateInstance(&clsid, null_mut(), CLSCTX_INPROC_SERVER, &iid_link, &mut link) != 0 {
            return Err("No se pudo crear el acceso directo".into());
        }
        let vt = *(link as *const *const usize);
        let m = |i: usize| *vt.add(i);
        std::mem::transmute::<usize, SetStr>(m(20))(link, wide(&target.display().to_string()).as_ptr()); // SetPath
        std::mem::transmute::<usize, SetStr>(m(9))(link, wide(&workdir.display().to_string()).as_ptr()); // SetWorkingDirectory
        std::mem::transmute::<usize, SetStr>(m(7))(link, wide("Luces y batería de tus periféricos").as_ptr()); // SetDescription
        let mut file: *mut core::ffi::c_void = null_mut();
        let hr = std::mem::transmute::<usize, Qi>(m(0))(link, &iid_file, &mut file);
        let mut ok = false;
        if hr == 0 {
            let vf = *(file as *const *const usize);
            ok = std::mem::transmute::<usize, Save>(*vf.add(6))(file, wide(&lnk.display().to_string()).as_ptr(), 1) == 0;
            std::mem::transmute::<usize, Release>(*vf.add(2))(file);
        }
        std::mem::transmute::<usize, Release>(m(2))(link);
        if ok { Ok(()) } else { Err("No se pudo guardar el acceso directo".into()) }
    }
}

fn set_str(key: HKEY, name: &str, value: &str) {
    let v = wide(value);
    unsafe { RegSetValueExW(key, wide(name).as_ptr(), 0, REG_SZ, v.as_ptr() as *const u8, (v.len() * 2) as u32) };
}

fn set_dword(key: HKEY, name: &str, value: u32) {
    unsafe { RegSetValueExW(key, wide(name).as_ptr(), 0, REG_DWORD, &value as *const u32 as *const u8, 4) };
}

/// Entrada en "Aplicaciones instaladas" con su desinstalador.
fn register_uninstall(exe: &Path, dir: &Path, version: &str, bytes: usize) {
    unsafe {
        let mut key: HKEY = null_mut();
        if RegCreateKeyExW(HKEY_CURRENT_USER, wide(UNINSTALL_KEY).as_ptr(), 0, null(), 0, KEY_SET_VALUE, null(), &mut key, null_mut()) != 0 {
            return;
        }
        let e = exe.display().to_string();
        set_str(key, "DisplayName", "Centro LED");
        set_str(key, "DisplayVersion", version);
        set_str(key, "Publisher", "Darwin2996");
        set_str(key, "DisplayIcon", &format!("{e},0"));
        set_str(key, "InstallLocation", &dir.display().to_string());
        set_str(key, "UninstallString", &format!("\"{e}\" --uninstall"));
        set_str(key, "URLInfoAbout", &format!("https://github.com/{}", update::REPO));
        set_dword(key, "NoModify", 1);
        set_dword(key, "NoRepair", 1);
        set_dword(key, "EstimatedSize", (bytes / 1024) as u32);
        RegCloseKey(key);
    }
}

/// Descarga la última versión y la instala en `dir`. `register` crea accesos directos y desinstalador.
fn install(dir: &Path, register: bool, desktop: bool) -> Result<PathBuf, String> {
    let rel = update::latest()?;
    let data = update::download(&rel.url)?;
    if data.len() < 50_000 || !data.starts_with(b"MZ") {
        return Err("El archivo descargado no es válido".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("No se pudo crear la carpeta: {e}"))?;
    let exe = dir.join("CentroLED.exe");
    // Si ya estaba instalado y abierto, Windows permite renombrar el ejecutable en uso.
    if exe.exists() && std::fs::remove_file(&exe).is_err() {
        let old = dir.join("CentroLED.old.exe");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&exe, &old).map_err(|e| format!("No se pudo reemplazar la versión instalada: {e}"))?;
    }
    std::fs::write(&exe, &data).map_err(|e| format!("No se pudo guardar el programa: {e}"))?;
    if register {
        let appdata = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default();
        shortcut(&appdata.join("Microsoft\\Windows\\Start Menu\\Programs\\Centro LED.lnk"), &exe, dir)?;
        if desktop {
            if let Some(d) = desktop_dir() {
                let _ = shortcut(&d.join("Centro LED.lnk"), &exe, dir);
            }
        }
        register_uninstall(&exe, dir, &rel.version, data.len());
    }
    Ok(exe)
}

unsafe extern "system" fn progress_cb(hwnd: HWND, msg: i32, _wp: WPARAM, _lp: LPARAM, _data: isize) -> i32 {
    match msg as u32 {
        TDN_CREATED => {
            SendMessageW(hwnd, TDM_SET_MARQUEE, 1, 30);
            let desktop = DESKTOP.load(Ordering::Relaxed);
            std::thread::spawn(move || {
                let r = install(&install_dir(), true, desktop);
                *RESULT.lock().unwrap() = Some(r);
            });
        }
        TDN_TIMER => {
            if RESULT.lock().unwrap().is_some() {
                SendMessageW(hwnd, TDM_CLICK_BUTTON, IDCANCEL as usize, 0);
            }
        }
        _ => {}
    }
    0
}

struct Dialog {
    instruction: String,
    content: String,
    buttons: Vec<(i32, String)>,
    common: i32,
    flags: i32,
    verification: Option<String>,
    callback: bool,
}

/// Muestra un TaskDialog; devuelve (botón pulsado, casilla marcada).
fn show(d: &Dialog) -> (i32, bool) {
    unsafe {
        let title = wide(TITLE);
        let instr = wide(&d.instruction);
        let content = wide(&d.content);
        let footer = wide(&format!("<a href=\"x\">github.com/{}</a>", update::REPO));
        let texts: Vec<Vec<u16>> = d.buttons.iter().map(|b| wide(&b.1)).collect();
        let btns: Vec<TASKDIALOG_BUTTON> = d.buttons.iter().zip(&texts).map(|(b, t)| TASKDIALOG_BUTTON { nButtonID: b.0, pszButtonText: t.as_ptr() }).collect();
        let verif = d.verification.as_ref().map(|v| wide(v));
        let mut cfg: TASKDIALOGCONFIG = std::mem::zeroed();
        cfg.cbSize = std::mem::size_of::<TASKDIALOGCONFIG>() as u32;
        cfg.hInstance = GetModuleHandleW(null());
        cfg.dwFlags = (d.flags | TDF_USE_HICON_MAIN) as _;
        cfg.dwCommonButtons = d.common as _;
        cfg.pszWindowTitle = title.as_ptr();
        cfg.Anonymous1.hMainIcon = LoadIconW(cfg.hInstance, 1 as _);
        cfg.pszMainInstruction = instr.as_ptr();
        cfg.pszContent = content.as_ptr();
        cfg.cButtons = btns.len() as u32;
        cfg.pButtons = if btns.is_empty() { null() } else { btns.as_ptr() };
        cfg.nDefaultButton = d.buttons.first().map(|b| b.0).unwrap_or(0);
        if let Some(v) = &verif {
            cfg.pszVerificationText = v.as_ptr();
        }
        cfg.pszFooter = footer.as_ptr();
        if d.callback {
            cfg.pfCallback = Some(progress_cb);
        }
        let mut button = 0;
        let mut checked: BOOL = 0;
        TaskDialogIndirect(&cfg, &mut button, null_mut(), &mut checked);
        (button, checked != 0)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Modo de prueba sin ventanas: CentroLED-Setup.exe --silent <carpeta> <salida.txt>
    if args.get(1).map(|s| s.as_str()) == Some("--silent") && args.len() >= 4 {
        let r = install(Path::new(&args[2]), false, false);
        let _ = std::fs::write(&args[3], match r {
            Ok(p) => format!("OK {}", p.display()),
            Err(e) => format!("ERROR {e}"),
        });
        return;
    }

    let dir = install_dir();
    let (button, desktop) = show(&Dialog {
        instruction: "Instalar Centro LED".into(),
        content: format!(
            "Luces y batería de tus periféricos en una app muy ligera.\n\nSe descargará la última versión desde GitHub y se instalará para tu usuario en:\n{}\n\nNo necesita permisos de administrador y se actualiza sola.",
            dir.display()
        ),
        buttons: vec![(ID_INSTALL, "Instalar".into())],
        common: TDCBF_CANCEL,
        flags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_VERIFICATION_FLAG_CHECKED,
        verification: Some("Crear acceso directo en el escritorio".into()),
        callback: false,
    });
    if button != ID_INSTALL {
        return;
    }
    DESKTOP.store(desktop, Ordering::Relaxed);
    // Si Centro LED está abierto, cerrarlo para poder sustituirlo.
    unsafe {
        let h = FindWindowW(wide("CentroLEDWindow").as_ptr(), null());
        if !h.is_null() {
            PostMessageW(h, WM_APP + 9, 0, 0);
            std::thread::sleep(std::time::Duration::from_millis(1200));
        }
    }
    show(&Dialog {
        instruction: "Instalando Centro LED…".into(),
        content: "Descargando la última versión desde GitHub.".into(),
        buttons: vec![],
        common: TDCBF_CANCEL,
        flags: TDF_SHOW_MARQUEE_PROGRESS_BAR | TDF_CALLBACK_TIMER,
        verification: None,
        callback: true,
    });
    let result = RESULT.lock().unwrap().take();
    match result {
        Some(Ok(exe)) => {
            let (b, _) = show(&Dialog {
                instruction: "Centro LED está instalado".into(),
                content: "Lo encontrarás en el menú Inicio. Para desinstalarlo, usa Configuración › Aplicaciones › Aplicaciones instaladas.".into(),
                buttons: vec![(ID_OPEN, "Abrir Centro LED".into())],
                common: TDCBF_CLOSE,
                flags: TDF_ALLOW_DIALOG_CANCELLATION,
                verification: None,
                callback: false,
            });
            if b == ID_OPEN {
                let _ = std::process::Command::new(&exe).current_dir(exe.parent().unwrap()).spawn();
            }
        }
        Some(Err(e)) => {
            show(&Dialog {
                instruction: "No se pudo instalar Centro LED".into(),
                content: format!("{e}\n\nComprueba tu conexión a Internet y vuelve a intentarlo."),
                buttons: vec![],
                common: TDCBF_CLOSE,
                flags: TDF_ALLOW_DIALOG_CANCELLATION,
                verification: None,
                callback: false,
            });
        }
        None => {} // cancelado
    }
}
