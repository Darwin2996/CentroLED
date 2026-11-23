#![windows_subsystem = "windows"]
//! Centro LED: luces y batería de tus periféricos en una ventana nativa muy ligera.
//! La interfaz se dibuja a mano con GDI (sin controles estándar ni motores web).

mod devices;
mod diag;
mod history;
mod state;
mod generic;
mod headsets;
mod nintendo;
mod openrgb;
mod hid;
mod kb_layout;
mod kb_template;
mod update;

use devices::{is_app_animated, Device, Extra, KbExtra, LedSetting, PadExtra, KB_LEDS, MODE_CUSTOM, MODE_CYCLE, MODE_OFF, MODE_SW_BREATH, MODE_SW_RAINBOW, RIM_MODES};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::collections::{HashMap, HashSet};
use std::ptr::{null, null_mut};
use std::sync::Mutex;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::System::Power::{RegisterPowerSettingNotification, POWERBROADCAST_SETTING};
use windows_sys::Win32::System::RemoteDesktop::WTSRegisterSessionNotification;
use windows_sys::Win32::System::Threading::{CreateMutexW, GetCurrentProcess, SetPriorityClass, SetProcessWorkingSetSize, NORMAL_PRIORITY_CLASS};
use windows_sys::Win32::UI::Controls::Dialogs::*;
use windows_sys::Win32::UI::Controls::TTTOOLINFOW;
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// Sólo un hilo habla con los dispositivos a la vez.
static HID_LOCK: Mutex<()> = Mutex::new(());

const WM_TRAY: u32 = WM_APP + 1;
const WM_SCANNED: u32 = WM_APP + 2;
const WM_APPLIED: u32 = WM_APP + 3;
const MENU_OPEN: usize = 1;
const MENU_SCAN: usize = 2;
const MENU_EXIT: usize = 3;
const MENU_TOGGLE: usize = 4;
const TIMER_BATTERY: usize = 1;
const TIMER_DEVCHANGE: usize = 2;
const TIMER_ANIM: usize = 3;
const TIMER_PROMOTE: usize = 4;
const TIMER_UPDATE: usize = 5;
const TIMER_WIDGET: usize = 6;
/// Tras volver de la suspensión, esperar a que los dispositivos se reconecten.
const TIMER_RESUME: usize = 7;
/// Reintentar el icono de la bandeja si el explorador aún no estaba listo (arranque rápido).
const TIMER_TRAY: usize = 8;
/// Búsquedas seguidas al arrancar y tras un fallo al aplicar luces.
const TIMER_RETRY: usize = 9;
const WM_UPD_INFO: u32 = WM_APP + 4;
const WM_UPD_DONE: u32 = WM_APP + 5;
/// Petición de cierre real (la usa el desinstalador).
const WM_QUIT_APP: u32 = WM_APP + 9;
/// Un dispositivo no aceptó las luces al aplicarlas solas (se reintenta en la siguiente búsqueda).
const WM_APPLY_FAILED: u32 = WM_APP + 10;
const UNINSTALL_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\CentroLED";

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const APP_NAME: &str = "CentroLED";
const WIN_W: i32 = 1100;
const WIN_H: i32 = 720;
const SIDEBAR_W: i32 = 284;
const WIN_STYLE: u32 = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
/// Avisa de que la pantalla se apaga o se enciende.
const GUID_CONSOLE_DISPLAY_STATE: windows_sys::core::GUID = windows_sys::core::GUID::from_u128(0x6fe69556_704a_47a0_8f24_c28d936fda47);

// ------------------------------------------------------------------ tema

const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}
/// Colores de la interfaz: dependen del tema, el color de acento y las esquinas de Ajustes.
#[derive(Clone, Copy)]
struct Theme {
    bg: u32,
    sidebar: u32,
    card: u32,
    card_hot: u32,
    line: u32,
    /// Contorno fino de tarjetas y botones.
    border: u32,
    text: u32,
    sub: u32,
    accent: u32,
    /// Acento para resaltar y para texto (más claro en los temas oscuros, más oscuro en el claro).
    accent_hot: u32,
    /// Texto sobre el acento (blanco, o casi negro si el acento es muy claro).
    on_accent: u32,
    green: u32,
    amber: u32,
    red: u32,
    light: bool,
    /// Esquinas rectas en vez de redondeadas.
    square: bool,
}

/// Tema base (nombres en THEME_NAMES): fondo, barra lateral, tarjeta, tarjeta resaltada, línea, contorno, texto y texto secundario.
struct Base {
    c: [u32; 8],
    light: bool,
}

const BASES: [Base; 5] = [
    Base { c: [rgb(0x12, 0x12, 0x18), rgb(0x19, 0x19, 0x22), rgb(0x22, 0x22, 0x2D), rgb(0x2C, 0x2C, 0x3A), rgb(0x34, 0x34, 0x44), rgb(0x2B, 0x2B, 0x38), rgb(0xF2, 0xF2, 0xF6), rgb(0x9C, 0x9C, 0xB0)], light: false },
    Base { c: [rgb(0x00, 0x00, 0x00), rgb(0x0B, 0x0B, 0x0E), rgb(0x16, 0x16, 0x1A), rgb(0x22, 0x22, 0x28), rgb(0x2E, 0x2E, 0x35), rgb(0x22, 0x22, 0x28), rgb(0xF2, 0xF2, 0xF6), rgb(0x96, 0x96, 0xA4)], light: false },
    Base { c: [rgb(0x1E, 0x1E, 0x1E), rgb(0x27, 0x27, 0x27), rgb(0x2D, 0x2D, 0x2D), rgb(0x38, 0x38, 0x38), rgb(0x46, 0x46, 0x46), rgb(0x37, 0x37, 0x37), rgb(0xFF, 0xFF, 0xFF), rgb(0xAA, 0xAA, 0xAA)], light: false },
    Base { c: [rgb(0x0B, 0x12, 0x20), rgb(0x10, 0x19, 0x2B), rgb(0x17, 0x22, 0x38), rgb(0x20, 0x2E, 0x4A), rgb(0x2A, 0x3A, 0x58), rgb(0x20, 0x2D, 0x46), rgb(0xEC, 0xF2, 0xFA), rgb(0x8F, 0xA2, 0xBF)], light: false },
    Base { c: [rgb(0xF3, 0xF3, 0xF6), rgb(0xE9, 0xE9, 0xEF), rgb(0xFF, 0xFF, 0xFF), rgb(0xEE, 0xEE, 0xF3), rgb(0xDC, 0xDC, 0xE4), rgb(0xE0, 0xE0, 0xE8), rgb(0x1A, 0x1A, 0x22), rgb(0x62, 0x62, 0x74)], light: true },
];

/// Nombres de los temas en Ajustes (el último sigue el modo claro u oscuro de Windows).
const THEME_NAMES: [&str; 6] = ["Oscuro", "Negro", "Grafito", "Océano", "Claro", "Sistema"];
const THEME_SYSTEM: u8 = 5;

/// Colores de acento (el último es el de Windows).
const ACCENTS: [(&str, u32); 8] = [
    ("Violeta", rgb(0x8B, 0x5C, 0xF6)),
    ("Azul", rgb(0x3B, 0x82, 0xF6)),
    ("Cian", rgb(0x08, 0x91, 0xB2)),
    ("Verde", rgb(0x16, 0xA3, 0x4A)),
    ("Rosa", rgb(0xDB, 0x27, 0x77)),
    ("Naranja", rgb(0xEA, 0x58, 0x0C)),
    ("Rojo", rgb(0xDC, 0x26, 0x26)),
    ("Windows", 0),
];
const ACCENT_WINDOWS: u8 = 7;

/// Teclas de la vista previa del teclado: el teclado es oscuro con cualquier tema.
const KEYCAP: u32 = rgb(0x24, 0x24, 0x2E);
const KB_TEXT: u32 = rgb(0xF2, 0xF2, 0xF6);
const KB_SUB: u32 = rgb(0x9C, 0x9C, 0xB0);
const KB_LINE: u32 = rgb(0x34, 0x34, 0x44);

static mut THEME: Theme = Theme {
    bg: BASES[0].c[0],
    sidebar: BASES[0].c[1],
    card: BASES[0].c[2],
    card_hot: BASES[0].c[3],
    line: BASES[0].c[4],
    border: BASES[0].c[5],
    text: BASES[0].c[6],
    sub: BASES[0].c[7],
    accent: rgb(0x8B, 0x5C, 0xF6),
    accent_hot: rgb(0xA0, 0x7A, 0xFF),
    on_accent: rgb(0xFF, 0xFF, 0xFF),
    green: rgb(0x3D, 0xDC, 0x84),
    amber: rgb(0xF5, 0xA6, 0x23),
    red: rgb(0xFF, 0x5A, 0x5F),
    light: false,
    square: false,
};

/// Colores actuales de la interfaz.
fn th() -> &'static Theme {
    unsafe { &*std::ptr::addr_of!(THEME) }
}

const PALETTE: [[u8; 3]; 12] = [
    [0xFF, 0x00, 0x00],
    [0xFF, 0x6A, 0x00],
    [0xFF, 0xD5, 0x00],
    [0x7C, 0xFF, 0x00],
    [0x00, 0xFF, 0x40],
    [0x00, 0xFF, 0xD5],
    [0x00, 0xA2, 0xFF],
    [0x00, 0x33, 0xFF],
    [0x69, 0x00, 0xFF],
    [0xB0, 0x00, 0xFF],
    [0xFF, 0x00, 0xB4],
    [0xFF, 0xFF, 0xFF],
];

fn to_ref(c: [u8; 3]) -> u32 {
    rgb(c[0], c[1], c[2])
}

fn mix(a: u32, b: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |s: u32| -> (f32, f32) { (((a >> s) & 0xFF) as f32, ((b >> s) & 0xFF) as f32) };
    let mut out = 0;
    for s in [0, 8, 16] {
        let (x, y) = ch(s);
        out |= ((x + (y - x) * t).round() as u32) << s;
    }
    out
}

fn hue(h: f32) -> [u8; 3] {
    let h = (h.rem_euclid(1.0)) * 6.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    let (r, g, b) = match h as i32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8]
}

/// Luminosidad aproximada (0 = negro, 1 = blanco).
fn luma(c: u32) -> f32 {
    ((c & 0xFF) as f32 * 0.299 + ((c >> 8) & 0xFF) as f32 * 0.587 + ((c >> 16) & 0xFF) as f32 * 0.114) / 255.0
}

/// Valor DWORD del registro del usuario.
fn reg_dword(path: &str, name: &str) -> Option<u32> {
    unsafe {
        let mut key: HKEY = null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, wide(path).as_ptr(), 0, KEY_QUERY_VALUE, &mut key) != 0 {
            return None;
        }
        let mut v: u32 = 0;
        let mut size = 4u32;
        let ok = RegQueryValueExW(key, wide(name).as_ptr(), null(), null_mut(), &mut v as *mut u32 as *mut u8, &mut size) == 0;
        RegCloseKey(key);
        ok.then_some(v)
    }
}

/// ¿Las aplicaciones de Windows usan el modo claro?
fn system_light() -> bool {
    reg_dword("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize", "AppsUseLightTheme") == Some(1)
}

/// Color de énfasis de Windows (Configuración › Personalización › Colores).
fn windows_accent() -> u32 {
    reg_dword("Software\\Microsoft\\Windows\\DWM", "AccentColor").map(|v| v & 0x00FF_FFFF).unwrap_or(ACCENTS[0].1)
}

fn make_theme(theme: u8, accent: u8, square: bool) -> Theme {
    let b = match theme {
        THEME_SYSTEM => &BASES[if system_light() { 4 } else { 0 }],
        t => &BASES[(t as usize).min(BASES.len() - 1)],
    };
    let acc = if accent == ACCENT_WINDOWS { windows_accent() } else { ACCENTS[(accent as usize).min(ACCENTS.len() - 2)].1 };
    let white = rgb(0xFF, 0xFF, 0xFF);
    Theme {
        bg: b.c[0],
        sidebar: b.c[1],
        card: b.c[2],
        card_hot: b.c[3],
        line: b.c[4],
        border: b.c[5],
        text: b.c[6],
        sub: b.c[7],
        accent: acc,
        accent_hot: if b.light { mix(acc, 0, 0.18) } else { mix(acc, white, 0.2) },
        on_accent: if luma(acc) > 0.62 { rgb(0x14, 0x14, 0x18) } else { white },
        green: if b.light { rgb(0x16, 0x9A, 0x50) } else { rgb(0x3D, 0xDC, 0x84) },
        amber: if b.light { rgb(0xB0, 0x68, 0x00) } else { rgb(0xF5, 0xA6, 0x23) },
        red: if b.light { rgb(0xD0, 0x2C, 0x33) } else { rgb(0xFF, 0x5A, 0x5F) },
        light: b.light,
        square,
    }
}

/// Aplica el tema guardado: colores, barra de título y menús.
fn apply_theme() {
    let a = app();
    unsafe {
        THEME = make_theme(a.config.theme, a.config.accent, a.config.square);
        if !a.hwnd.is_null() {
            style_window(a.hwnd);
        }
        menu_mode(th().light);
    }
    redraw();
}

/// Barra de título con los colores del tema (en Windows 10 sólo cambia entre clara y oscura).
unsafe fn style_window(h: HWND) {
    let t = th();
    let dark: i32 = (!t.light) as i32;
    DwmSetWindowAttribute(h, 20, &dark as *const i32 as _, 4);
    DwmSetWindowAttribute(h, 35, &t.bg as *const u32 as _, 4); // DWMWA_CAPTION_COLOR
    DwmSetWindowAttribute(h, 36, &t.text as *const u32 as _, 4); // DWMWA_TEXT_COLOR
}

/// Menús emergentes claros u oscuros como el tema (uxtheme: SetPreferredAppMode y FlushMenuThemes).
unsafe fn menu_mode(light: bool) {
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    let ux = LoadLibraryW(wide("uxtheme.dll").as_ptr());
    if ux.is_null() {
        return;
    }
    if let Some(f) = GetProcAddress(ux, 135 as *const u8) {
        let set: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(f);
        set(if light { 3 } else { 2 }); // ForceLight / ForceDark
    }
    if let Some(f) = GetProcAddress(ux, 136 as *const u8) {
        let flush: unsafe extern "system" fn() = std::mem::transmute(f);
        flush();
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ------------------------------------------------------------------ estado

#[derive(Clone, Copy, PartialEq, Debug)]
enum Act {
    Device(usize),
    Tab(u8),
    Mode(u8),
    Color([u8; 3]),
    MoreColors,
    Multi,
    Slider(u8),
    Key(usize),
    FillAll,
    ClearAll,
    CycleRemove(usize),
    PlayerLed(u8),
    PlayerBright(u8),
    Mic(u8),
    Apply,
    Scan,
    Autostart,
    TrayBattery,
    Update,
    Home,
    Settings,
    Widget,
    LowBattery(u8),
    OpenUrl,
    Profile(usize),
    ProfileSave,
    ProfileMore(usize),
    AllColor([u8; 3]),
    AllToggle,
    DevProfiles,
    HistToggle,
    HistRange(u8),
    AutoOpt(u8),
    AutoDev(usize),
    HkRecord(usize),
    HkClear(usize),
    Diagnostics,
    FastStart,
    Theme(u8),
    Accent(u8),
    Corners(bool),
}

struct Fonts {
    title: HFONT,
    h2: HFONT,
    body: HFONT,
    bold: HFONT,
    small: HFONT,
    key: HFONT,
    key_small: HFONT,
    icon: HFONT,
    icon_big: HFONT,
    small_icon: HFONT,
}

#[derive(Default)]
struct Config {
    led: HashMap<String, LedSetting>,
    keys: HashMap<String, Box<[[u8; 3]; KB_LEDS]>>,
    cycles: HashMap<String, Vec<[u8; 3]>>,
    hide_tray_battery: bool,
    /// No mostrar el widget de batería en las barras de tareas secundarias.
    hide_widget: bool,
    /// Distancia del widget al borde derecho de la barra (px lógicos).
    widget_offset: i32,
    /// Umbral del aviso de batería baja (0 = desactivado).
    low_battery: u8,
    pads: HashMap<String, PadExtra>,
    /// Perfiles globales: el estado de luces de varios dispositivos.
    profiles: Vec<state::Profile>,
    /// Perfiles propios de cada dispositivo.
    dprofiles: HashMap<String, Vec<(String, state::DevState)>>,
    /// Estado de antes de "Apagar todo" (vacío si las luces no están apagadas así).
    off_backup: Vec<(String, state::DevState)>,
    active_profile: String,
    /// Apagado automático: al bloquear, al apagarse la pantalla, al suspender, al apagar el equipo.
    auto: [bool; 4],
    /// Dispositivos que no se apagan solos.
    auto_exclude: HashSet<String>,
    hotkeys: Vec<state::Hotkey>,
    /// Inicio con una tarea programada (sin el retraso de Windows y con más prioridad).
    fast_start: bool,
    /// Ya se crearon los perfiles de ejemplo.
    seeded: bool,
    /// Se apagaron las luces al apagar el equipo: devolverlas al arrancar.
    pending_restore: bool,
    /// Aspecto: tema (THEME_NAMES), color de acento (ACCENTS) y esquinas rectas.
    theme: u8,
    accent: u8,
    square: bool,
}

/// Hilo que funde suavemente los colores del ciclo en un mando.
struct Cycler {
    stop: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
}

const MAX_CYCLE: usize = 8;

struct App {
    hwnd: HWND,
    scale: f32,
    fonts: Fonts,
    icon: HICON,
    devices: Vec<Device>,
    sel: Option<String>,
    tab: u8,
    cur: LedSetting,
    rim: LedSetting,
    keys: Box<[[u8; 3]; KB_LEDS]>,
    cycle: Vec<[u8; 3]>,
    cyclers: HashMap<String, Cycler>,
    /// Ajustes por zona (cascos con varias luces); vacío si el dispositivo tiene una sola.
    zones: Vec<LedSetting>,
    /// LEDs de jugador y micrófono del DualSense.
    pad: PadExtra,
    dirty: bool,
    config: Config,
    known: HashSet<String>,
    scanning: bool,
    applying: bool,
    status: String,
    hits: Vec<(RECT, Act)>,
    hot: Option<Act>,
    drag: Option<Act>,
    tracking: bool,
    custom_colors: [u32; 16],
    taskbar_created: u32,
    autostart: bool,
    t0: std::time::Instant,
    /// Zona de la vista previa animada (sólo se repinta esa zona en cada fotograma).
    anim_rect: RECT,
    /// Dónde se dibujó la vista previa (teclado o mando) para redibujar sólo eso al animar.
    preview: Option<(bool, i32, i32, i32)>,
    /// Pintado parcial de la animación: no registrar zonas de clic.
    partial: bool,
    /// Última batería leída de cada dispositivo (para mostrarla cuando duerme).
    last_battery: HashMap<String, u8>,
    /// Dispositivos de los que ya se avisó por batería baja.
    low_warned: HashSet<String>,
    /// Iconos de batería en la barra de tareas, uno por dispositivo.
    tray_batt: HashMap<String, TrayBatt>,
    next_tray_id: u32,
    upd: Upd,
    mutex: HANDLE,
    /// 0 = inicio (resumen), 1 = dispositivo, 2 = ajustes
    page: u8,
    list_scroll: i32,
    list_max: i32,
    home_scroll: i32,
    home_max: i32,
    widgets: Vec<Widget>,
    widget_drag: Option<(i32, i32, bool)>,
    /// Motivos activos del apagado automático (bloqueo, pantalla, suspensión).
    auto_off: u8,
    /// Dispositivos con luces cambiadas mientras estaban desconectados.
    pending: HashSet<String>,
    /// Acción cuyo atajo se está grabando.
    recording: Option<String>,
    hk_ids: Vec<i32>,
    hk_failed: HashSet<String>,
    set_scroll: i32,
    set_max: i32,
    /// Las zonas de clic se recortan a esta zona (páginas con desplazamiento).
    clip: Option<RECT>,
    hist: HashMap<String, Vec<history::Rec>>,
    hist_days: u64,
    /// Página del dispositivo: mostrar el historial de batería en vez de las luces.
    show_hist: bool,
    first_scan: bool,
    /// Búsquedas rápidas que quedan (cada 15 s): al arrancar, para los dispositivos que tardan en
    /// estar listos (OpenRGB, Bluetooth, cascos), y tras un fallo al aplicar.
    quick_scans: u8,
}

/// Widget de batería colocado sobre la barra de tareas de una pantalla secundaria.
struct Widget {
    hwnd: HWND,
    taskbar: HWND,
    tip: HWND,
    sig: String,
    dpi: u32,
}

/// Estado del actualizador.
enum Upd {
    Idle,
    Checking,
    UpToDate,
    Available(update::Release),
    Installing,
    Error(String),
}

/// Icono de batería de un dispositivo en el área de notificación.
struct TrayBatt {
    id: u32,
    state: (u8, bool, bool),
    icon: HICON,
}

static mut APP: *mut App = null_mut();

fn app() -> &'static mut App {
    unsafe { &mut *APP }
}

fn s(v: i32) -> i32 {
    (v as f32 * app().scale).round() as i32
}

// ------------------------------------------------------------------ configuración

/// Versión portable: el ejecutable se llama "...portable..." o hay un "portable.txt" a su lado.
/// En ese caso la configuración se guarda junto al programa (sirve para llevarlo en un USB).
fn portable_dir() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.to_path_buf();
    let name = exe.file_name()?.to_string_lossy().to_lowercase();
    (name.contains("portable") || dir.join("portable.txt").exists()).then_some(dir)
}

fn config_path() -> std::path::PathBuf {
    if let Some(dir) = portable_dir() {
        return dir.join("CentroLED-datos").join("config.txt");
    }
    let base = std::env::var_os("APPDATA").map(std::path::PathBuf::from).unwrap_or_else(|| ".".into());
    base.join(APP_NAME).join("config.txt")
}

fn parse_rgb(h: &str) -> Option<[u8; 3]> {
    let v = u32::from_str_radix(h, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

fn load_config() -> Config {
    let mut cfg = Config { widget_offset: 110, low_battery: 15, ..Config::default() };
    let Ok(text) = std::fs::read_to_string(config_path()) else { return cfg };
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() == 3 && f[0] == "#keys" && f[2].len() == KB_LEDS * 6 {
            let mut keys = Box::new([[0u8; 3]; KB_LEDS]);
            for (i, k) in keys.iter_mut().enumerate() {
                *k = parse_rgb(&f[2][i * 6..i * 6 + 6]).unwrap_or([0; 3]);
            }
            cfg.keys.insert(f[1].to_string(), keys);
        } else if f.len() == 5 && f[0] == "#pad" {
            let n = |s: &str| s.parse::<u8>().unwrap_or(0);
            cfg.pads.insert(f[1].to_string(), PadExtra { player: n(f[2]), player_bright: n(f[3]), mic: n(f[4]) });
        } else if f.len() == 3 && f[0] == "#opt" {
            match f[1] {
                "hide_tray_battery" => cfg.hide_tray_battery = f[2] == "1",
                "hide_widget" => cfg.hide_widget = f[2] == "1",
                "widget_offset" => cfg.widget_offset = f[2].parse().unwrap_or(110),
                "low_battery" => cfg.low_battery = f[2].parse().unwrap_or(15),
                "auto_lock" => cfg.auto[0] = f[2] == "1",
                "auto_display" => cfg.auto[1] = f[2] == "1",
                "auto_sleep" => cfg.auto[2] = f[2] == "1",
                "auto_shutdown" => cfg.auto[3] = f[2] == "1",
                "fast_start" => cfg.fast_start = f[2] == "1",
                "profile" => cfg.active_profile = f[2].to_string(),
                "seeded" => cfg.seeded = f[2] == "1",
                "pending_restore" => cfg.pending_restore = f[2] == "1",
                "theme" => cfg.theme = f[2].parse().unwrap_or(0),
                "accent" => cfg.accent = f[2].parse().unwrap_or(0),
                "corners" => cfg.square = f[2] == "rectas",
                _ => {}
            }
        } else if f.len() == 3 && f[0] == "#cycle" {
            let list: Vec<[u8; 3]> = (0..f[2].len() / 6).filter_map(|i| parse_rgb(&f[2][i * 6..i * 6 + 6])).collect();
            if list.len() >= 2 {
                cfg.cycles.insert(f[1].to_string(), list);
            }
        } else if f.len() >= 2 && f[0] == "#prof" {
            let name = f[1].to_string();
            let i = match cfg.profiles.iter().position(|p| p.name == name) {
                Some(i) => i,
                None => {
                    cfg.profiles.push(state::Profile { name, devs: Vec::new() });
                    cfg.profiles.len() - 1
                }
            };
            if f.len() == 4 {
                cfg.profiles[i].devs.push((f[2].to_string(), state::DevState::from_text(f[3])));
            }
        } else if f.len() == 4 && f[0] == "#dprof" {
            cfg.dprofiles.entry(f[1].to_string()).or_default().push((f[2].to_string(), state::DevState::from_text(f[3])));
        } else if f.len() == 3 && f[0] == "#off" {
            cfg.off_backup.push((f[1].to_string(), state::DevState::from_text(f[2])));
        } else if f.len() == 2 && f[0] == "#noauto" {
            cfg.auto_exclude.insert(f[1].to_string());
        } else if f.len() == 4 && f[0] == "#hk" {
            if let (Ok(mods), Ok(vk)) = (f[2].parse(), f[3].parse()) {
                cfg.hotkeys.push(state::Hotkey { action: f[1].to_string(), mods, vk });
            }
        } else if f.len() == 6 {
            let n = |s: &str| s.parse::<u8>().ok();
            if let (Some(mode), Some(c), Some(bright), Some(speed), Some(multi)) = (n(f[1]), parse_rgb(f[2]), n(f[3]), n(f[4]), n(f[5])) {
                cfg.led.insert(f[0].to_string(), LedSetting { mode, rgb: (c[0], c[1], c[2]), bright, speed, multi: multi != 0 });
            }
        }
    }
    cfg
}

fn save_config(cfg: &Config) {
    let path = config_path();
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let mut text = String::new();
    for (k, s) in &cfg.led {
        text += &format!("{}\t{}\t{:02x}{:02x}{:02x}\t{}\t{}\t{}\n", k, s.mode, s.rgb.0, s.rgb.1, s.rgb.2, s.bright, s.speed, s.multi as u8);
    }
    for (k, keys) in &cfg.keys {
        let hex: String = keys.iter().map(|c| format!("{:02x}{:02x}{:02x}", c[0], c[1], c[2])).collect();
        text += &format!("#keys\t{}\t{}\n", k, hex);
    }
    for (k, list) in &cfg.cycles {
        let hex: String = list.iter().map(|c| format!("{:02x}{:02x}{:02x}", c[0], c[1], c[2])).collect();
        text += &format!("#cycle\t{}\t{}\n", k, hex);
    }
    for (k, x) in &cfg.pads {
        text += &format!("#pad\t{}\t{}\t{}\t{}\n", k, x.player, x.player_bright, x.mic);
    }
    text += &format!("#opt\thide_tray_battery\t{}\n", cfg.hide_tray_battery as u8);
    text += &format!("#opt\thide_widget\t{}\n", cfg.hide_widget as u8);
    text += &format!("#opt\twidget_offset\t{}\n", cfg.widget_offset);
    text += &format!("#opt\tlow_battery\t{}\n", cfg.low_battery);
    for (i, n) in ["auto_lock", "auto_display", "auto_sleep", "auto_shutdown"].iter().enumerate() {
        text += &format!("#opt\t{}\t{}\n", n, cfg.auto[i] as u8);
    }
    text += &format!("#opt\tfast_start\t{}\n", cfg.fast_start as u8);
    text += &format!("#opt\tprofile\t{}\n", cfg.active_profile);
    text += &format!("#opt\tseeded\t{}\n", cfg.seeded as u8);
    text += &format!("#opt\tpending_restore\t{}\n", cfg.pending_restore as u8);
    text += &format!("#opt\ttheme\t{}\n", cfg.theme);
    text += &format!("#opt\taccent\t{}\n", cfg.accent);
    text += &format!("#opt\tcorners\t{}\n", if cfg.square { "rectas" } else { "redondeadas" });
    for p in &cfg.profiles {
        if p.devs.is_empty() {
            text += &format!("#prof\t{}\n", p.name);
        }
        for (k, st) in &p.devs {
            text += &format!("#prof\t{}\t{}\t{}\n", p.name, k, st.to_text());
        }
    }
    for (k, list) in &cfg.dprofiles {
        for (n, st) in list {
            text += &format!("#dprof\t{}\t{}\t{}\n", k, n, st.to_text());
        }
    }
    for (k, st) in &cfg.off_backup {
        text += &format!("#off\t{}\t{}\n", k, st.to_text());
    }
    for k in &cfg.auto_exclude {
        text += &format!("#noauto\t{}\n", k);
    }
    for h in &cfg.hotkeys {
        text += &format!("#hk\t{}\t{}\t{}\n", h.action, h.mods, h.vk);
    }
    // Escribir aparte y luego sustituir: un corte a mitad no deja la configuración a medias.
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, &path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

fn default_setting(dev: &Device) -> LedSetting {
    let mode = dev.led.as_ref().map(|l| l.modes[0].1).unwrap_or(1);
    LedSetting { mode, rgb: (0x69, 0x00, 0xFF), bright: 100, speed: 60, multi: false }
}

// ------------------------------------------------------------------ inicio con Windows

fn autostart_enabled() -> bool {
    unsafe {
        let mut key: HKEY = null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, KEY_QUERY_VALUE, &mut key) != 0 {
            return false;
        }
        let ok = RegQueryValueExW(key, wide(APP_NAME).as_ptr(), null(), null_mut(), null_mut(), null_mut()) == 0;
        RegCloseKey(key);
        ok
    }
}

/// Comando guardado en "Ejecutar al iniciar", si existe.
fn autostart_value() -> Option<String> {
    unsafe {
        let mut key: HKEY = null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, KEY_QUERY_VALUE, &mut key) != 0 {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        let ok = RegQueryValueExW(key, wide(APP_NAME).as_ptr(), null(), null_mut(), buf.as_mut_ptr() as *mut u8, &mut size) == 0;
        RegCloseKey(key);
        ok.then(|| String::from_utf16_lossy(&buf[..(size as usize / 2)]).trim_end_matches('\0').to_string())
    }
}

fn set_autostart(on: bool) {
    unsafe {
        let mut key: HKEY = null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, KEY_SET_VALUE, &mut key) != 0 {
            return;
        }
        if on {
            let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
            let value = wide(&format!("\"{exe}\" --tray"));
            RegSetValueExW(key, wide(APP_NAME).as_ptr(), 0, REG_SZ, value.as_ptr() as *const u8, (value.len() * 2) as u32);
        } else {
            RegDeleteValueW(key, wide(APP_NAME).as_ptr());
        }
        RegCloseKey(key);
    }
}

// ------------------------------------------------------------------ hilos de trabajo

fn redraw() {
    unsafe { InvalidateRect(app().hwnd, null(), 0) };
}

fn set_status(text: &str) {
    app().status = text.to_string();
    redraw();
}

fn start_scan() {
    let a = app();
    if a.scanning {
        return;
    }
    a.scanning = true;
    set_status("Buscando dispositivos…");
    let hwnd = a.hwnd as isize;
    std::thread::spawn(move || {
        let list = {
            let _g = HID_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            devices::scan()
        };
        unsafe { PostMessageW(hwnd as HWND, WM_SCANNED, 0, Box::into_raw(Box::new(list)) as LPARAM) };
    });
}

fn start_apply(dev: Device, st: LedSetting, extra: Option<Extra>, silent: bool) {
    let hwnd = app().hwnd as isize;
    if !silent {
        app().applying = true;
    }
    std::thread::spawn(move || {
        let res = {
            let _g = HID_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            devices::apply(&dev, &st, extra.as_ref())
        };
        if silent && res.is_err() {
            let key = Box::into_raw(Box::new(dev.key.clone()));
            unsafe { PostMessageW(hwnd as HWND, WM_APPLY_FAILED, 0, key as LPARAM) };
        }
        let msg = match res {
            Ok(()) if silent => String::new(),
            Ok(()) => format!("✓ Guardado en {}", dev.name),
            Err(e) => format!("{}: {}", dev.name, e),
        };
        unsafe { PostMessageW(hwnd as HWND, WM_APPLIED, silent as usize, Box::into_raw(Box::new(msg)) as LPARAM) };
    });
}

/// Color (sin aplicar el brillo) de una animación de la app en el instante `t` (segundos).
fn anim_color(st: &LedSetting, colors: &[[u8; 3]], t: f32) -> [u8; 3] {
    match st.mode {
        MODE_CYCLE if colors.len() >= 2 => {
            // De 8 s a 0,5 s por color.
            let t = t / (8.0 - st.speed as f32 * 0.075);
            let i = t.floor() as usize % colors.len();
            let (a, b) = (colors[i], colors[(i + 1) % colors.len()]);
            let f = t.fract();
            let f = f * f * (3.0 - 2.0 * f);
            [0, 1, 2].map(|n| (a[n] as f32 + (b[n] as f32 - a[n] as f32) * f).round() as u8)
        }
        MODE_SW_BREATH => {
            // Respiración de 6 s a 1 s.
            let ph = t / (6.0 - st.speed as f32 * 0.05);
            let k = 0.04 + 0.96 * (0.5 - 0.5 * (ph * std::f32::consts::TAU).cos());
            [st.rgb.0, st.rgb.1, st.rgb.2].map(|c| (c as f32 * k).round() as u8)
        }
        MODE_SW_RAINBOW => hue(t / (12.0 - st.speed as f32 * 0.11)),
        _ => [st.rgb.0, st.rgb.1, st.rgb.2],
    }
}

/// Arranca (o reinicia) una animación de la app en un mando (ciclo, respiración o arcoíris).
/// Mantiene el mando abierto y envía ~30 colores por segundo; si se desconecta, el hilo termina.
fn start_cycle(dev: Device, colors: Vec<[u8; 3]>, st: LedSetting, pad: Option<PadExtra>) {
    stop_cycle(&dev.key);
    if st.mode == MODE_CYCLE && colors.len() < 2 {
        return;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let alive = Arc::new(AtomicBool::new(true));
    app().cyclers.insert(dev.key.clone(), Cycler { stop: stop.clone(), alive: alive.clone() });
    std::thread::spawn(move || {
        if let Ok(lb) = devices::Lightbar::open(&dev) {
            if let Some(px) = pad {
                lb.set_extras(&px);
            }
            let k = st.bright as f32 / 100.0;
            let start = std::time::Instant::now();
            let mut last = None;
            let mut fails = 0;
            while !stop.load(Ordering::Relaxed) {
                let ac = anim_color(&st, &colors, start.elapsed().as_secs_f32());
                let ch = |n: usize| (ac[n] as f32 * k).round() as u8;
                let c = (ch(0), ch(1), ch(2));
                if last != Some(c) {
                    if lb.set(c) {
                        fails = 0;
                        last = Some(c);
                    } else {
                        fails += 1;
                        if fails > 30 {
                            break; // mando desconectado
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(33));
            }
        }
        alive.store(false, Ordering::Relaxed);
    });
}

fn stop_cycle(key: &str) {
    if let Some(c) = app().cyclers.remove(key) {
        c.stop.store(true, Ordering::Relaxed);
    }
}

fn cycle_running(key: &str) -> bool {
    app().cyclers.get(key).map(|c| c.alive.load(Ordering::Relaxed)).unwrap_or(false)
}

// ------------------------------------------------------------------ dispositivo seleccionado

fn selected() -> Option<Device> {
    let a = app();
    let key = a.sel.as_ref()?;
    a.devices.iter().find(|d| &d.key == key).cloned()
}

/// Teclado con editor tecla a tecla (Tempest, o cualquier teclado de OpenRGB / Dynamic Lighting).
fn is_keyboard(d: &Device) -> bool {
    d.led.as_ref().map(|l| l.keymap).unwrap_or(false)
}

/// Sólo el Tempest tiene una tira de luz en el borde con sus propios efectos.
fn has_rim(d: &Device) -> bool {
    matches!(d.driver, devices::Driver::Tempest { .. })
}

fn select_device(i: usize) {
    let a = app();
    let Some(d) = a.devices.get(i).cloned() else { return };
    a.sel = Some(d.key.clone());
    a.tab = 0;
    a.dirty = false;
    a.show_hist = false;
    a.cur = a.config.led.get(&d.key).copied().unwrap_or_else(|| default_setting(&d));
    a.rim = a
        .config
        .led
        .get(&format!("{}#borde", d.key))
        .copied()
        .unwrap_or(LedSetting { mode: 24, rgb: a.cur.rgb, bright: 100, speed: 60, multi: false });
    let c = [a.cur.rgb.0, a.cur.rgb.1, a.cur.rgb.2];
    a.keys = a.config.keys.get(&d.key).cloned().unwrap_or_else(|| Box::new([c; KB_LEDS]));
    a.cycle = a.config.cycles.get(&d.key).cloned().unwrap_or_else(|| vec![[0x69, 0x00, 0xFF], [0x00, 0xA2, 0xFF], [0xFF, 0x00, 0xB4]]);
    let nz = d.led.as_ref().map(|l| l.zones.len()).unwrap_or(0);
    let cur = a.cur;
    a.zones = (0..nz).map(|i| a.config.led.get(&format!("{}#z{}", d.key, i)).copied().unwrap_or(cur)).collect();
    a.pad = a.config.pads.get(&d.key).copied().unwrap_or(if matches!(d.driver, devices::Driver::Switch { .. }) {
        PadExtra { player: 0b0001, player_bright: 2, mic: 0 }
    } else {
        PadExtra::default()
    });
    redraw();
}

/// Ajuste que se está editando: la zona de la pestaña elegida, el borde del teclado o el principal.
fn editing() -> &'static mut LedSetting {
    let a = app();
    if !a.zones.is_empty() {
        let i = (a.tab as usize).min(a.zones.len() - 1);
        &mut a.zones[i]
    } else if a.tab == 1 {
        &mut a.rim
    } else {
        &mut a.cur
    }
}

fn on_apply() {
    let a = app();
    let Some(d) = selected() else { return };
    if d.led.is_none() || a.applying {
        return;
    }
    let caps = d.led.clone().unwrap();
    // Un cambio a mano deja de seguir el perfil activo y saca al dispositivo de "Apagar todo".
    a.config.active_profile.clear();
    a.config.off_backup.retain(|(k, _)| k != &d.key);
    if !a.zones.is_empty() {
        a.cur = a.zones[0];
        for (i, z) in a.zones.iter().enumerate() {
            a.config.led.insert(format!("{}#z{}", d.key, i), *z);
        }
    }
    a.config.led.insert(d.key.clone(), a.cur);
    let extra = if is_keyboard(&d) {
        a.config.led.insert(format!("{}#borde", d.key), a.rim);
        a.config.keys.insert(d.key.clone(), a.keys.clone());
        Some(Extra::Keyboard(KbExtra { rim: a.rim, keys: a.keys.clone() }))
    } else if !a.zones.is_empty() {
        Some(Extra::Zones(a.zones.clone()))
    } else if caps.pad_extras {
        a.config.pads.insert(d.key.clone(), a.pad);
        Some(Extra::Pad(a.pad))
    } else {
        None
    };
    if is_app_animated(a.cur.mode) {
        if a.cur.mode == MODE_CYCLE {
            if a.cycle.len() < 2 {
                set_status("Elige al menos 2 colores para el ciclo");
                return;
            }
            a.config.cycles.insert(d.key.clone(), a.cycle.clone());
        }
        save_config(&a.config);
        a.dirty = false;
        let pad = if caps.pad_extras { Some(a.pad) } else { None };
        set_status(&format!("✓ Efecto activo en {}", d.name));
        start_cycle(d, a.cycle.clone(), a.cur, pad);
        return;
    }
    stop_cycle(&d.key);
    save_config(&a.config);
    a.dirty = false;
    set_status(&format!("Aplicando a {}…", d.name));
    start_apply(d, a.cur, extra, false);
}

fn pick_more_colors() {
    let a = app();
    let e = editing();
    let mut cc: CHOOSECOLORW = unsafe { std::mem::zeroed() };
    cc.lStructSize = std::mem::size_of::<CHOOSECOLORW>() as u32;
    cc.hwndOwner = a.hwnd;
    cc.rgbResult = rgb(e.rgb.0, e.rgb.1, e.rgb.2);
    cc.lpCustColors = a.custom_colors.as_mut_ptr();
    cc.Flags = CC_RGBINIT | CC_FULLOPEN;
    if unsafe { ChooseColorW(&mut cc) } != 0 {
        let c = cc.rgbResult;
        set_color([c as u8, (c >> 8) as u8, (c >> 16) as u8]);
    }
}

fn cycle_editing() -> bool {
    let a = app();
    a.zones.is_empty() && a.tab == 0 && a.cur.mode == MODE_CYCLE
}

fn set_color(c: [u8; 3]) {
    let a = app();
    if cycle_editing() {
        if a.cycle.len() < MAX_CYCLE {
            a.cycle.push(c);
            a.dirty = true;
        }
        redraw();
        return;
    }
    let e = editing();
    e.rgb = (c[0], c[1], c[2]);
    e.multi = false;
    app().dirty = true;
    redraw();
}

// ------------------------------------------------------------------ primitivas de dibujo

struct Painter {
    dc: HDC,
}

/// Contorno para que un color muy claro se vea sobre el fondo claro.
fn swatch_edge(c: u32) -> Option<(u32, i32)> {
    (th().light && luma(c) > 0.85).then_some((th().line, 1))
}

/// Radio de las esquinas según el estilo elegido (rectas: casi sin redondeo).
fn corner(radius: i32) -> i32 {
    if th().square { radius.min(3) } else { radius }
}

impl Painter {
    fn rect(&self, l: i32, t: i32, w: i32, h: i32) -> RECT {
        RECT { left: s(l), top: s(t), right: s(l + w), bottom: s(t + h) }
    }

    fn fill(&self, r: &RECT, color: u32, radius: i32) {
        unsafe {
            let br = CreateSolidBrush(color);
            let old_b = SelectObject(self.dc, br);
            let old_p = SelectObject(self.dc, GetStockObject(NULL_PEN));
            let rad = s(corner(radius)) * 2;
            RoundRect(self.dc, r.left, r.top, r.right + 1, r.bottom + 1, rad, rad);
            SelectObject(self.dc, old_b);
            SelectObject(self.dc, old_p);
            DeleteObject(br);
        }
    }

    fn stroke(&self, r: &RECT, color: u32, radius: i32, width: i32) {
        unsafe {
            let pen = CreatePen(PS_SOLID, s(width).max(1), color);
            let old_p = SelectObject(self.dc, pen);
            let old_b = SelectObject(self.dc, GetStockObject(NULL_BRUSH));
            let rad = s(corner(radius)) * 2;
            RoundRect(self.dc, r.left, r.top, r.right, r.bottom, rad, rad);
            SelectObject(self.dc, old_b);
            SelectObject(self.dc, old_p);
            DeleteObject(pen);
        }
    }

    fn circle(&self, r: &RECT, fill: u32, border: Option<(u32, i32)>) {
        unsafe {
            let br = CreateSolidBrush(fill);
            let pen = match border {
                Some((c, w)) => CreatePen(PS_SOLID, s(w).max(1), c),
                None => null_mut(),
            };
            let old_b = SelectObject(self.dc, br);
            let old_p = SelectObject(self.dc, if pen.is_null() { GetStockObject(NULL_PEN) } else { pen });
            Ellipse(self.dc, r.left, r.top, r.right, r.bottom);
            SelectObject(self.dc, old_b);
            SelectObject(self.dc, old_p);
            DeleteObject(br);
            if !pen.is_null() {
                DeleteObject(pen);
            }
        }
    }

    fn text(&self, text: &str, r: &RECT, color: u32, font: HFONT, flags: u32) {
        if text.is_empty() {
            return; // DrawTextW falla con cadenas vacías
        }
        unsafe {
            SelectObject(self.dc, font);
            SetTextColor(self.dc, color);
            let w: Vec<u16> = text.encode_utf16().collect();
            let mut rc = *r;
            DrawTextW(self.dc, w.as_ptr(), w.len() as i32, &mut rc, flags | DT_NOPREFIX);
        }
    }

    /// Texto multilínea centrado vertical y horizontalmente en `r`.
    fn text_center(&self, text: &str, r: &RECT, color: u32, font: HFONT) {
        if text.is_empty() {
            return;
        }
        if !text.contains('\n') {
            return self.text(text, r, color, font, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
        unsafe {
            SelectObject(self.dc, font);
            let w: Vec<u16> = text.encode_utf16().collect();
            let mut calc = *r;
            DrawTextW(self.dc, w.as_ptr(), w.len() as i32, &mut calc, DT_CENTER | DT_CALCRECT | DT_NOPREFIX);
            let h = calc.bottom - calc.top;
            let top = r.top + ((r.bottom - r.top) - h) / 2;
            let rc = RECT { left: r.left, top, right: r.right, bottom: top + h };
            self.text(text, &rc, color, font, DT_CENTER);
        }
    }

    fn measure(&self, text: &str, font: HFONT) -> i32 {
        unsafe {
            SelectObject(self.dc, font);
            let w: Vec<u16> = text.encode_utf16().collect();
            let mut sz = SIZE { cx: 0, cy: 0 };
            GetTextExtentPoint32W(self.dc, w.as_ptr(), w.len() as i32, &mut sz);
            (sz.cx as f32 / app().scale).ceil() as i32
        }
    }
}

fn hit(r: RECT, act: Act) {
    let a = app();
    if a.partial {
        return;
    }
    let r = match a.clip {
        Some(c) => RECT { left: r.left.max(c.left), top: r.top.max(c.top), right: r.right.min(c.right), bottom: r.bottom.min(c.bottom) },
        None => r,
    };
    if r.bottom > r.top && r.right > r.left {
        a.hits.push((r, act));
    }
}

fn is_hot(act: Act) -> bool {
    app().hot == Some(act)
}

fn glyph(d: &Device) -> &'static str {
    match d.kind {
        "Teclado" => "\u{E765}",
        "Mando" => "\u{E7FC}",
        "Cascos" => "\u{E7F6}",
        "Ratón" => "\u{E962}",
        "Bluetooth" => "\u{E702}",
        "Lápiz" => "\u{EDC6}",
        "Reloj" => "\u{E916}",
        "Móvil" => "\u{E8EA}",
        "Placa base" | "Memoria RAM" | "Gráfica" | "Refrigeración" | "Caja" | "Almacenamiento" => "\u{E950}",
        "Tira LED" | "Luz" | "RGB" => "\u{E781}",
        "Altavoz" => "\u{E7F5}",
        "Micrófono" => "\u{E720}",
        "Alfombrilla" => "\u{E962}",
        _ => "\u{E772}",
    }
}

fn battery_color(pct: u8) -> u32 {
    if pct > 50 {
        th().green
    } else if pct > 20 {
        th().amber
    } else {
        th().red
    }
}

fn battery_text(d: &Device) -> String {
    match d.battery {
        Some(b) if b.approx => format!("≈ {} %", b.pct),
        Some(b) if b.charging => format!("{} % · cargando", b.pct),
        Some(b) => format!("{} %", b.pct),
        None => match last_battery(d) {
            Some(pct) => format!("{} · último {} %", d.battery_note, pct),
            None => d.battery_note.to_string(),
        },
    }
}

/// Último porcentaje leído de un dispositivo que ahora no responde (p. ej. ratón dormido).
fn last_battery(d: &Device) -> Option<u8> {
    unsafe {
        if APP.is_null() {
            return None;
        }
    }
    app().last_battery.get(&d.key).copied()
}

// ------------------------------------------------------------------ pintado

fn paint(p: &Painter) {
    let a = app();
    a.hits.clear();
    a.preview = None;
    let full = p.rect(0, 0, WIN_W, WIN_H);
    p.fill(&full, th().bg, 0);
    paint_sidebar(p);
    let content_x = SIDEBAR_W + 28;
    let content_w = WIN_W - content_x - 28;
    match (a.page, selected()) {
        (1, Some(d)) => paint_device(p, &d, content_x, content_w),
        (2, _) => paint_settings(p, content_x, content_w),
        _ => paint_home(p, content_x, content_w),
    }
}

/// Elemento de navegación de la barra lateral (Inicio / Ajustes).
fn nav_item(p: &Painter, y: i32, glyph_s: &str, label: &str, active: bool, act: Act) {
    let a = app();
    let r = p.rect(14, y, SIDEBAR_W - 28, 40);
    p.fill(&r, if active || is_hot(act) { th().card_hot } else { th().sidebar }, 10);
    if active {
        p.fill(&p.rect(14, y + 10, 4, 20), th().accent, 2);
    }
    p.text(glyph_s, &p.rect(30, y, 30, 40), if active { th().accent_hot } else { th().sub }, a.fonts.icon, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    p.text(label, &p.rect(70, y, 180, 40), th().text, a.fonts.bold, DT_SINGLELINE | DT_VCENTER);
    hit(r, act);
}

/// Registra una zona de clic recortada al área visible de una lista con desplazamiento.
fn hit_clipped(r: RECT, view: &RECT, act: Act) {
    let c = RECT { left: r.left.max(view.left), top: r.top.max(view.top), right: r.right.min(view.right), bottom: r.bottom.min(view.bottom) };
    if c.bottom > c.top {
        hit(c, act);
    }
}

fn paint_sidebar(p: &Painter) {
    let a = app();
    p.fill(&p.rect(0, 0, SIDEBAR_W, WIN_H), th().sidebar, 0);
    // Logotipo
    p.fill(&p.rect(22, 24, 36, 36), th().accent, 10);
    p.text("\u{E781}", &p.rect(22, 24, 36, 36), th().on_accent, a.fonts.icon, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    p.text("Centro LED", &p.rect(70, 22, 200, 24), th().text, a.fonts.h2, DT_SINGLELINE);
    paint_update(p);

    nav_item(p, 80, "\u{E80F}", "Inicio", a.page == 0, Act::Home);

    // Cabecera de la lista con el botón de buscar
    section(p, &format!("DISPOSITIVOS · {}", a.devices.len()), 28, 136);
    let sr = p.rect(SIDEBAR_W - 52, 128, 30, 30);
    if is_hot(Act::Scan) {
        p.fill(&sr, th().card_hot, 8);
    }
    p.text(if a.scanning { "\u{E895}" } else { "\u{E72C}" }, &sr, if a.scanning { th().accent_hot } else { th().sub }, a.fonts.small_icon, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    hit(sr, Act::Scan);

    // Lista con desplazamiento
    let view_top = 164;
    let view_bottom = WIN_H - 72;
    let view = p.rect(0, view_top, SIDEBAR_W, view_bottom - view_top);
    let total = a.devices.len() as i32 * 66;
    a.list_max = (total - (view_bottom - view_top)).max(0);
    a.list_scroll = a.list_scroll.clamp(0, a.list_max);
    unsafe {
        SaveDC(p.dc);
        IntersectClipRect(p.dc, view.left, view.top, view.right, view.bottom);
    }
    let mut y = view_top - a.list_scroll;
    for (i, d) in a.devices.iter().enumerate() {
        if y + 62 < view_top || y > view_bottom {
            y += 66;
            continue;
        }
        let r = p.rect(14, y, SIDEBAR_W - 28, 62);
        let selected = a.page == 1 && a.sel.as_deref() == Some(&d.key);
        let hot = is_hot(Act::Device(i));
        p.fill(&r, if selected || hot { th().card_hot } else { th().sidebar }, 10);
        if selected {
            p.fill(&p.rect(14, y + 14, 4, 34), th().accent, 2);
        }
        let ic = p.rect(28, y + 11, 40, 40);
        p.fill(&ic, if selected { th().accent } else { th().card }, 10);
        p.text(glyph(d), &ic, if selected { th().on_accent } else { th().text }, a.fonts.icon, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        p.text(&d.name, &p.rect(80, y + 10, SIDEBAR_W - 28 - 76, 20), th().text, a.fonts.bold, DT_SINGLELINE | DT_END_ELLIPSIS);
        match d.battery {
            Some(b) => {
                p.fill(&p.rect(80, y + 39, 96, 6), th().line, 3);
                p.fill(&p.rect(80, y + 39, (96 * b.pct as i32 / 100).max(4), 6), battery_color(b.pct), 3);
                let label = if b.charging { format!("{} % ⚡", b.pct) } else if b.approx { format!("≈{} %", b.pct) } else { format!("{} %", b.pct) };
                p.text(&label, &p.rect(184, y + 32, 70, 20), th().sub, a.fonts.small, DT_SINGLELINE | DT_VCENTER);
            }
            None if last_battery(d).is_some() => {
                let pct = last_battery(d).unwrap();
                p.fill(&p.rect(80, y + 39, 96, 6), th().line, 3);
                p.fill(&p.rect(80, y + 39, (96 * pct as i32 / 100).max(4), 6), mix(th().line, battery_color(pct), 0.45), 3);
                p.text(&format!("{} % · zZ", pct), &p.rect(184, y + 32, 70, 20), th().sub, a.fonts.small, DT_SINGLELINE | DT_VCENTER);
            }
            None => {
                let note = if d.battery_note.is_empty() || d.battery_note == "—" { d.conn } else { d.battery_note };
                p.text(&format!("{} · {}", d.kind, note), &p.rect(80, y + 32, SIDEBAR_W - 28 - 76, 20), th().sub, a.fonts.small, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
            }
        }
        hit_clipped(r, &view, Act::Device(i));
        y += 66;
    }
    if a.devices.is_empty() {
        let msg = if a.scanning { "Buscando…" } else { "No se encontraron dispositivos" };
        p.text(msg, &p.rect(28, view_top + 8, SIDEBAR_W - 56, 24), th().sub, a.fonts.body, DT_SINGLELINE);
    }
    unsafe {
        RestoreDC(p.dc, -1);
    }
    // Sombra suave cuando hay más elementos debajo / encima
    if a.list_scroll < a.list_max {
        p.fill(&p.rect(14, view_bottom - 2, SIDEBAR_W - 28, 2), th().line, 1);
    }
    if a.list_scroll > 0 {
        p.fill(&p.rect(14, view_top, SIDEBAR_W - 28, 2), th().line, 1);
    }

    nav_item(p, WIN_H - 60, "\u{E713}", "Ajustes", a.page == 2, Act::Settings);
}

/// Página de inicio: una tarjeta por dispositivo con su batería y estado de luces.
fn paint_home(p: &Painter, x: i32, w: i32) {
    let a = app();
    p.text("Resumen", &p.rect(x, 22, w, 34), th().text, a.fonts.title, DT_SINGLELINE);
    let with_bat = a.devices.iter().filter(|d| d.battery.is_some()).count();
    let sub = if a.status.is_empty() {
        format!("{} dispositivos · {} con batería", a.devices.len(), with_bat)
    } else {
        a.status.clone()
    };
    p.text(&sub, &p.rect(x, 58, w, 20), th().sub, a.fonts.body, DT_SINGLELINE | DT_END_ELLIPSIS);
    paint_home_bar(p, x, w);

    let cols = 3;
    let gap = 16;
    let cw = (w - gap * (cols - 1)) / cols;
    let ch = 140;
    let view_top = 152;
    let view_bottom = WIN_H - 16;
    let view = p.rect(x - 8, view_top, w + 16, view_bottom - view_top);
    let rows = (a.devices.len() as i32 + cols - 1) / cols;
    a.home_max = (rows * (ch + gap) - (view_bottom - view_top)).max(0);
    a.home_scroll = a.home_scroll.clamp(0, a.home_max);
    if a.devices.is_empty() {
        let msg = if a.scanning { "Buscando dispositivos…" } else { "No se encontraron dispositivos. Conecta alguno y pulsa el botón de buscar." };
        p.text(msg, &p.rect(x, 140, w, 30), th().sub, a.fonts.h2, DT_SINGLELINE);
        return;
    }
    unsafe {
        SaveDC(p.dc);
        IntersectClipRect(p.dc, view.left, view.top, view.right, view.bottom);
    }
    for (i, d) in a.devices.iter().enumerate() {
        let (col, row) = (i as i32 % cols, i as i32 / cols);
        let cx = x + col * (cw + gap);
        let cy = view_top + row * (ch + gap) - a.home_scroll;
        if cy + ch < view_top || cy > view_bottom {
            continue;
        }
        let r = p.rect(cx, cy, cw, ch);
        let hot = is_hot(Act::Device(i));
        p.fill(&r, if hot { th().card_hot } else { th().card }, 14);
        p.stroke(&r, if hot { mix(th().border, th().accent, 0.7) } else { th().border }, 14, 1);
        // Icono con el color de sus luces (si las tiene y están guardadas)
        let led_col = a.config.led.get(&d.key).filter(|s| s.mode != MODE_OFF).map(|s| rgb(s.rgb.0, s.rgb.1, s.rgb.2));
        let ic = p.rect(cx + 16, cy + 16, 44, 44);
        p.fill(&ic, led_col.map(|c| mix(th().bg, c, 0.55)).unwrap_or(th().line), 12);
        p.text(glyph(d), &ic, th().text, a.fonts.icon, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        p.text(&d.name, &p.rect(cx + 72, cy + 16, cw - 88, 22), th().text, a.fonts.bold, DT_SINGLELINE | DT_END_ELLIPSIS);
        p.text(&format!("{} · {}", d.kind, d.conn), &p.rect(cx + 72, cy + 38, cw - 88, 20), th().sub, a.fonts.small, DT_SINGLELINE | DT_END_ELLIPSIS);
        // Batería o estado
        let by = cy + 78;
        let bat = d.battery.map(|b| (b.pct, b.charging, false)).or_else(|| last_battery(d).map(|p| (p, false, true)));
        match bat {
            Some((pct, charging, stale)) => {
                let col = if stale { mix(th().line, battery_color(pct), 0.45) } else { battery_color(pct) };
                p.text(&format!("{} %", pct), &p.rect(cx + 16, by, 120, 30), th().text, a.fonts.h2, DT_SINGLELINE | DT_VCENTER);
                let est = if stale { None } else { history::estimate_text(d) };
                let note = est.unwrap_or_else(|| (if charging { "cargando ⚡" } else if stale { "último valor · dormido" } else { "batería" }).to_string());
                p.text(&note, &p.rect(cx + 84, by, cw - 100, 30), th().sub, a.fonts.small, DT_SINGLELINE | DT_VCENTER | DT_RIGHT | DT_END_ELLIPSIS);
                p.fill(&p.rect(cx + 16, by + 36, cw - 32, 8), th().line, 4);
                p.fill(&p.rect(cx + 16, by + 36, ((cw - 32) * pct as i32 / 100).max(8), 8), col, 4);
            }
            None => {
                let note = if d.battery_note.is_empty() || d.battery_note == "—" { "Sin batería" } else { d.battery_note };
                p.text(note, &p.rect(cx + 16, by, cw - 32, 24), th().sub, a.fonts.body, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
            }
        }
        if d.led.is_some() {
            let st = a.config.led.get(&d.key);
            let label = match st {
                Some(s) if s.mode == MODE_OFF => "Luces apagadas".to_string(),
                Some(_) => "Luces configuradas".to_string(),
                None => "Luces · sin configurar".to_string(),
            };
            // Sin batería: la segunda línea muestra el estado de las luces.
            if bat.is_none() {
                let ly = by + 30;
                if let Some(c) = led_col {
                    p.circle(&p.rect(cx + 16, ly + 5, 10, 10), c, None);
                }
                let lx = if led_col.is_some() { cx + 32 } else { cx + 16 };
                p.text(&label, &p.rect(lx, ly, cw - 48, 20), th().sub, a.fonts.small, DT_SINGLELINE | DT_VCENTER);
            }
        }
        hit_clipped(r, &view, Act::Device(i));
    }
    unsafe {
        RestoreDC(p.dc, -1);
    }
}

/// Botón redondeado con texto (chip). Devuelve su ancho lógico.
fn chip(p: &Painter, label: &str, x: i32, y: i32, h: i32, on: bool, act: Act) -> i32 {
    chip_bg(p, label, x, y, h, on, act, th().card)
}

/// Igual que `chip` pero sobre otro fondo (dentro de una tarjeta se usa LINE para que se vea).
fn chip_bg(p: &Painter, label: &str, x: i32, y: i32, h: i32, on: bool, act: Act, bg: u32) -> i32 {
    let a = app();
    let tw = p.measure(label, a.fonts.body) + 28;
    let r = p.rect(x, y, tw, h);
    let on_card = bg == th().line;
    let hot = if on_card { mix(th().line, th().text, 0.12) } else { th().card_hot };
    p.fill(&r, if on { th().accent } else if is_hot(act) { hot } else { bg }, h / 2);
    if !on && !on_card {
        p.stroke(&r, th().border, h / 2, 1);
    }
    p.text(label, &r, if on { th().on_accent } else { th().text }, a.fonts.body, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    hit(r, act);
    tw
}

/// Barra de la página de inicio: perfiles a la izquierda y "todas las luces" a la derecha.
fn paint_home_bar(p: &Painter, x: i32, w: i32) {
    let a = app();
    let ty = 92;
    let lit = a.devices.iter().any(|d| d.led.is_some());
    const ALL: [[u8; 3]; 7] = [[0xFF, 0, 0], [0xFF, 0x6A, 0], [0xFF, 0xD5, 0], [0, 0xFF, 0x40], [0, 0xA2, 0xFF], [0x69, 0, 0xFF], [0xFF, 0xFF, 0xFF]];
    let btn = if state::all_lights_off() { "Encender todo" } else { "Apagar todo" };
    let bw = p.measure(btn, a.fonts.body) + 28;
    let rx = x + w - (ALL.len() as i32 * 30 + 10 + bw);
    if lit {
        section(p, "TODAS LAS LUCES", rx, ty);
        let mut cx = rx;
        for c in ALL {
            let r = p.rect(cx + 3, ty + 23, 24, 24);
            if is_hot(Act::AllColor(c)) {
                p.circle(&p.rect(cx, ty + 20, 30, 30), th().bg, Some((th().sub, 2)));
            }
            p.circle(&r, to_ref(c), swatch_edge(to_ref(c)));
            hit(r, Act::AllColor(c));
            cx += 30;
        }
        chip(p, btn, cx + 10, ty + 20, 30, false, Act::AllToggle);
    }
    section(p, "PERFILES · CLIC DERECHO PARA EDITAR", x, ty);
    let limit = if lit { rx - 24 } else { x + w };
    let reserve = p.measure("+ Guardar", a.fonts.body).max(p.measure("Más ▾", a.fonts.body)) + 36;
    let mut cx = x;
    for (i, pr) in a.config.profiles.iter().enumerate() {
        let tw = p.measure(&pr.name, a.fonts.body) + 28;
        if cx + tw + reserve > limit {
            chip(p, "Más ▾", cx, ty + 20, 30, false, Act::ProfileMore(i));
            return;
        }
        cx += chip(p, &pr.name, cx, ty + 20, 30, pr.name == a.config.active_profile, Act::Profile(i)) + 8;
    }
    chip(p, "+ Guardar", cx, ty + 20, 30, false, Act::ProfileSave);
}

/// Fila de ajuste con interruptor, título y descripción.
fn setting_row(p: &Painter, x: i32, y: i32, w: i32, title: &str, desc: &str, on: bool, act: Act) -> i32 {
    let a = app();
    p.text(title, &p.rect(x + 20, y + 14, w - 110, 22), th().text, a.fonts.bold, DT_SINGLELINE);
    let lines = if desc.is_empty() { 0 } else { 2 };
    if lines > 0 {
        p.text(desc, &p.rect(x + 20, y + 36, w - 110, 36), th().sub, a.fonts.small, DT_WORDBREAK);
    }
    paint_switch(p, x + w - 62, y + 18, on, act);
    y + if lines > 0 { 80 } else { 50 }
}

fn card(p: &Painter, x: i32, y: i32, w: i32, h: i32, title: &str) {
    section(p, title, x, y);
    let r = p.rect(x, y + 22, w, h);
    p.fill(&r, th().card, 14);
    p.stroke(&r, th().border, 14, 1);
}

/// Miniatura de la ventana con los colores de un tema (con el acento actual).
fn theme_thumb(p: &Painter, b: &Base, x: i32, y: i32, w: i32, h: i32) {
    let acc = th().accent;
    let (bg, side, card, line, text, sub) = (b.c[0], b.c[1], b.c[2], b.c[4], b.c[6], b.c[7]);
    p.fill(&p.rect(x, y, w, h), bg, 8);
    let sw = w * 30 / 100;
    p.fill(&p.rect(x, y, sw, h), side, 8);
    p.fill(&p.rect(x + sw - 8, y, 8, h), side, 0);
    p.fill(&p.rect(x + 6, y + 7, 8, 8), acc, 2);
    for (i, lw) in [16, 12, 14].iter().enumerate() {
        p.fill(&p.rect(x + 6, y + 22 + i as i32 * 9, *lw, 3), if i == 0 { text } else { sub }, 1);
    }
    let cx = x + sw + 7;
    let cw = (w - sw - 19) / 2;
    p.fill(&p.rect(cx, y + 7, 28, 4), text, 2);
    for k in 0..2 {
        let r = p.rect(cx + k * (cw + 5), y + 18, cw, h - 26);
        p.fill(&r, card, 5);
        p.fill(&p.rect(cx + k * (cw + 5) + 5, y + 24, cw - 14, 3), sub, 1);
        p.fill(&p.rect(cx + k * (cw + 5) + 5, y + h - 16, cw - 10, 4), line, 2);
        p.fill(&p.rect(cx + k * (cw + 5) + 5, y + h - 16, (cw - 10) * if k == 0 { 70 } else { 40 } / 100, 4), acc, 2);
    }
    p.stroke(&p.rect(x, y, w, h), if b.light { b.c[5] } else { mix(line, bg, 0.3) }, 8, 1);
}

/// Ajustes › Apariencia: tema, color de acento y esquinas. Devuelve dónde empieza lo siguiente.
fn paint_appearance(p: &Painter, x: i32, y: i32, w: i32) -> i32 {
    let a = app();
    let h = 190;
    card(p, x, y, w, h, "APARIENCIA");
    // Temas
    let n = THEME_NAMES.len() as i32;
    let gap = 14;
    let tw = (w - 40 - gap * (n - 1)) / n;
    let (ty, tht) = (y + 42, 64);
    for i in 0..n {
        let tx = x + 20 + i * (tw + gap);
        let act = Act::Theme(i as u8);
        let on = a.config.theme == i as u8;
        if i as u8 == THEME_SYSTEM {
            // Mitad oscura y mitad clara
            let half = p.rect(tx, ty, tw / 2, tht);
            unsafe {
                SaveDC(p.dc);
                IntersectClipRect(p.dc, half.left, half.top, half.right, half.bottom);
            }
            theme_thumb(p, &BASES[0], tx, ty, tw, tht);
            unsafe {
                RestoreDC(p.dc, -1);
                SaveDC(p.dc);
                IntersectClipRect(p.dc, half.right, half.top, s(tx + tw) + 1, half.bottom + 1);
            }
            theme_thumb(p, &BASES[4], tx, ty, tw, tht);
            unsafe {
                RestoreDC(p.dc, -1);
            }
        } else {
            theme_thumb(p, &BASES[i as usize], tx, ty, tw, tht);
        }
        if on || is_hot(act) {
            p.stroke(&p.rect(tx - 4, ty - 4, tw + 8, tht + 8), if on { th().accent } else { th().sub }, 11, 2);
        }
        p.text(THEME_NAMES[i as usize], &p.rect(tx - 6, ty + tht + 7, tw + 12, 18), if on { th().text } else { th().sub }, if on { a.fonts.bold } else { a.fonts.small }, DT_CENTER | DT_SINGLELINE);
        hit(p.rect(tx - 4, ty - 4, tw + 8, tht + 30), act);
    }
    // Color de acento
    let ay = y + 154;
    let shown = match a.hot {
        Some(Act::Accent(i)) => i,
        _ => a.config.accent,
    };
    section(p, &format!("COLOR DE ACENTO · {}", ACCENTS[shown as usize].0.to_uppercase()), x + 20, ay);
    let mut cx = x + 20;
    for (i, (_, c)) in ACCENTS.iter().enumerate() {
        let act = Act::Accent(i as u8);
        let on = a.config.accent == i as u8;
        let col = if i as u8 == ACCENT_WINDOWS { windows_accent() } else { *c };
        if on || is_hot(act) {
            p.circle(&p.rect(cx - 4, ay + 22, 36, 36), th().card, Some((if on { th().text } else { th().sub }, 2)));
        }
        let r = p.rect(cx, ay + 26, 28, 28);
        p.circle(&r, col, None);
        if i as u8 == ACCENT_WINDOWS {
            // Cuatro cuadritos: el color de Windows
            let fg = if luma(col) > 0.62 { rgb(0x14, 0x14, 0x18) } else { rgb(0xFF, 0xFF, 0xFF) };
            for (dx, dy) in [(0, 0), (5, 0), (0, 5), (5, 5)] {
                p.fill(&p.rect(cx + 9 + dx, ay + 35 + dy, 4, 4), fg, 0);
            }
        }
        hit(r, act);
        cx += 40;
    }
    // Esquinas
    let ex = x + 20 + ACCENTS.len() as i32 * 40 + 40;
    section(p, "ESQUINAS", ex, ay);
    let mut bx = ex;
    for (label, sq) in [("Redondeadas", false), ("Rectas", true)] {
        bx += chip_bg(p, label, bx, ay + 25, 30, a.config.square == sq, Act::Corners(sq), th().line) + 8;
    }
    y + h + 40
}

/// Página de ajustes (se desplaza con la rueda si no cabe).
fn paint_settings(p: &Painter, x: i32, w: i32) {
    let a = app();
    p.text("Ajustes", &p.rect(x, 22, w, 34), th().text, a.fonts.title, DT_SINGLELINE);
    p.text(if portable_dir().is_some() { "Versión portable · la configuración se guarda junto al programa" } else { "Se guardan al momento" }, &p.rect(x, 58, w, 20), th().sub, a.fonts.body, DT_SINGLELINE);
    let view_top = 86;
    let view_bottom = WIN_H - 8;
    let view = p.rect(x - 8, view_top, w + 16, view_bottom - view_top);
    unsafe {
        SaveDC(p.dc);
        IntersectClipRect(p.dc, view.left, view.top, view.right, view.bottom);
    }
    a.clip = Some(view);
    let top = paint_appearance(p, x, view_top + 10 - a.set_scroll, w);
    let half = (w - 20) / 2;

    // Columna izquierda
    let mut y = top;
    card(p, x, y, half, 176, "BARRA DE TAREAS");
    let yy = setting_row(p, x, y + 22, half, "Batería junto al reloj", "Un icono por dispositivo con su tipo y porcentaje.", !a.config.hide_tray_battery, Act::TrayBattery);
    setting_row(p, x, yy, half, "Batería en las otras pantallas", "Windows sólo los pone junto al reloj en la pantalla principal.", !a.config.hide_widget, Act::Widget);
    y += 216;
    card(p, x, y, half, 96, "AVISO DE BATERÍA BAJA");
    let mut cx = x + 20;
    for (label, v) in [("No", 0u8), ("10 %", 10), ("15 %", 15), ("20 %", 20), ("30 %", 30)] {
        let tw = p.measure(label, a.fonts.body) + 26;
        let r = p.rect(cx, y + 40, tw, 30);
        let on = a.config.low_battery == v;
        p.fill(&r, if on { th().accent } else if is_hot(Act::LowBattery(v)) { mix(th().line, th().text, 0.12) } else { th().line }, 15);
        p.text(label, &r, if on { th().on_accent } else { th().text }, a.fonts.body, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        hit(r, Act::LowBattery(v));
        cx += tw + 8;
    }
    p.text("Aviso de Windows al bajar de ese nivel sin cargar.", &p.rect(x + 20, y + 80, half - 40, 20), th().sub, a.fonts.small, DT_SINGLELINE | DT_END_ELLIPSIS);
    y += 136;
    card(p, x, y, half, 160, "INICIO");
    let yy = setting_row(p, x, y + 22, half, "Iniciar con Windows", "Se abre minimizada en segundo plano: sólo el icono de la bandeja.", a.autostart || a.config.fast_start, Act::Autostart);
    setting_row(p, x, yy, half, "Inicio rápido · prioridad alta", "Arranca nada más iniciar sesión, sin la espera que Windows pone a los programas de inicio.", a.config.fast_start, Act::FastStart);
    y += 200;
    let acts = state::hk_actions();
    let hh = 44 + acts.len() as i32 * 52;
    card(p, x, y, half, hh, "ATAJOS DE TECLADO · TAMBIÉN EN SEGUNDO PLANO");
    let mut ry = y + 32;
    for (i, (action, label)) in acts.iter().enumerate() {
        let rec = a.recording.as_deref() == Some(action.as_str());
        let hk = state::hotkey_of(action);
        let (txt, col) = match hk {
            _ if rec => ("Pulsa la combinación… (Esc cancela)".to_string(), th().amber),
            Some(h) if a.hk_failed.contains(action) => (format!("{} · en uso por otro programa", state::combo_name(h.mods, h.vk)), th().red),
            Some(h) => (state::combo_name(h.mods, h.vk), th().accent_hot),
            None => ("Sin atajo".to_string(), th().sub),
        };
        // Botones a la derecha
        let mut bx = x + half - 16;
        if hk.is_some() && !rec {
            bx -= p.measure("Quitar", a.fonts.body) + 28;
            chip_bg(p, "Quitar", bx, ry + 6, 30, false, Act::HkClear(i), th().line);
            bx -= 8;
        }
        let lbl = if rec { "Cancelar" } else if hk.is_some() { "Cambiar" } else { "Grabar" };
        bx -= p.measure(lbl, a.fonts.body) + 28;
        chip_bg(p, lbl, bx, ry + 6, 30, rec, Act::HkRecord(i), th().line);
        p.text(label, &p.rect(x + 20, ry, bx - x - 28, 22), th().text, a.fonts.bold, DT_SINGLELINE | DT_END_ELLIPSIS);
        p.text(&txt, &p.rect(x + 20, ry + 22, bx - x - 28, 18), col, a.fonts.small, DT_SINGLELINE | DT_END_ELLIPSIS);
        ry += 52;
    }
    p.text("Pulsa Grabar y luego la combinación, p. ej. Ctrl + Alt + L.", &p.rect(x + 20, ry, half - 40, 20), th().sub, a.fonts.small, DT_SINGLELINE | DT_END_ELLIPSIS);
    let left_end = y + 22 + hh;

    // Columna derecha
    let rx = x + half + 20;
    let mut y = top;
    let devs: Vec<usize> = (0..a.devices.len()).filter(|&i| a.devices[i].led.is_some() && !is_keyboard(&a.devices[i])).collect();
    let mut chips = Vec::new();
    let (mut cx, mut cy) = (rx + 20, 0);
    for &i in &devs {
        let tw = (p.measure(&a.devices[i].name, a.fonts.small) + 24).min(half - 40);
        if cx + tw > rx + half - 20 {
            cx = rx + 20;
            cy += 34;
        }
        chips.push((i, cx, cy, tw));
        cx += tw + 6;
    }
    let chips_h = if devs.is_empty() { 22 } else { cy + 28 };
    let ah = 262 + chips_h;
    card(p, rx, y, half, ah, "APAGADO AUTOMÁTICO · VUELVEN SOLAS AL REGRESAR");
    let mut yy = y + 22;
    for (n, title) in ["Al bloquear el equipo (Win + L)", "Al apagarse la pantalla", "Al suspender el equipo", "Al apagar el equipo o cerrar sesión"].iter().enumerate() {
        yy = setting_row(p, rx, yy, half, title, "", a.config.auto[n], Act::AutoOpt(n as u8));
    }
    p.text("QUÉ SE APAGA · EL TECLADO NUNCA", &p.rect(rx + 20, yy + 6, half - 40, 18), th().sub, a.fonts.small, DT_SINGLELINE);
    let base = yy + 30;
    if devs.is_empty() {
        p.text("Conecta un dispositivo con luces.", &p.rect(rx + 20, base, half - 40, 22), th().sub, a.fonts.small, DT_SINGLELINE);
    }
    for (i, cx, cy, tw) in chips {
        let d = &a.devices[i];
        let configured = a.config.led.contains_key(&d.key);
        let on = configured && !a.config.auto_exclude.contains(&d.key);
        let r = p.rect(cx, base + cy, tw, 28);
        p.fill(&r, if on { th().accent } else if is_hot(Act::AutoDev(i)) { mix(th().line, th().text, 0.12) } else { th().line }, 14);
        p.text(&d.name, &RECT { left: r.left + s(10), right: r.right - s(10), ..r }, if on { th().on_accent } else if configured { th().text } else { th().sub }, a.fonts.small, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        hit(r, Act::AutoDev(i));
    }
    p.text("Sólo los que tienen sus luces guardadas en Centro LED.", &p.rect(rx + 20, base + chips_h + 6, half - 40, 20), th().sub, a.fonts.small, DT_SINGLELINE | DT_END_ELLIPSIS);
    y += ah + 40;
    let orgb = a.devices.iter().filter(|d| d.conn == "OpenRGB").count();
    card(p, rx, y, half, 176, "OPENRGB · PLACA BASE, RAM, GRÁFICA…");
    let (status, col) = if orgb > 0 { (format!("● Conectado · {} dispositivo{}", orgb, if orgb == 1 { "" } else { "s" }), th().green) } else { ("● No detectado".to_string(), th().sub) };
    p.text(&status, &p.rect(rx + 20, y + 36, half - 40, 22), col, a.fonts.bold, DT_SINGLELINE);
    p.text(
        "Instala OpenRGB, abre la pestaña SDK Server y pulsa Start Server (puedes hacer que arranque minimizado con el servidor). Sus dispositivos aparecerán aquí.",
        &p.rect(rx + 20, y + 64, half - 40, 120),
        th().sub,
        a.fonts.small,
        DT_WORDBREAK,
    );
    y += 216;
    card(p, rx, y, half, 118, "DIAGNÓSTICO");
    p.text("Informe de tus dispositivos para investigar un problema. Se guarda donde elijas y no se envía a ningún sitio.", &p.rect(rx + 20, y + 34, half - 40, 40), th().sub, a.fonts.small, DT_WORDBREAK);
    let label = "Exportar diagnóstico…";
    let tw = p.measure(label, a.fonts.body) + 32;
    let b = p.rect(rx + 20, y + 80, tw, 34);
    p.fill(&b, if is_hot(Act::Diagnostics) { mix(th().line, th().text, 0.12) } else { th().line }, 10);
    p.text(label, &b, th().text, a.fonts.body, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    hit(b, Act::Diagnostics);
    y += 158;
    card(p, rx, y, half, 128, "ACERCA DE");
    p.text(&format!("Centro LED {}", update::VERSION), &p.rect(rx + 20, y + 36, half - 40, 22), th().text, a.fonts.bold, DT_SINGLELINE);
    let state = match &a.upd {
        Upd::Available(r) => format!("Hay una versión nueva: {}", r.version),
        Upd::Checking => "Comprobando actualizaciones…".into(),
        Upd::UpToDate => "Estás al día · las versiones nuevas llegan firmadas".into(),
        Upd::Installing => "Descargando y comprobando la firma…".into(),
        Upd::Error(e) if e.contains("firma") || e.contains("alterado") || e.contains("firmada") => e.clone(),
        Upd::Error(_) => "No se pudo comprobar (sin conexión)".into(),
        Upd::Idle => "Se actualiza sola desde GitHub (con firma comprobada)".into(),
    };
    p.text(&state, &p.rect(rx + 20, y + 58, half - 40, 20), th().sub, a.fonts.small, DT_SINGLELINE | DT_END_ELLIPSIS);
    let label = if matches!(a.upd, Upd::Available(_)) { "Actualizar ahora" } else { "Buscar actualizaciones" };
    let tw = p.measure(label, a.fonts.body) + 32;
    let b = p.rect(rx + 20, y + 90, tw, 34);
    p.fill(&b, if is_hot(Act::Update) { th().accent_hot } else { th().accent }, 10);
    p.text(label, &b, th().on_accent, a.fonts.body, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    hit(b, Act::Update);
    let g = p.rect(rx + 30 + tw, y + 90, 130, 34);
    p.fill(&g, if is_hot(Act::OpenUrl) { mix(th().line, th().text, 0.12) } else { th().line }, 10);
    p.text("Ver en GitHub", &g, th().text, a.fonts.body, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    hit(g, Act::OpenUrl);
    let right_end = y + 22 + 128;

    unsafe {
        RestoreDC(p.dc, -1);
    }
    a.clip = None;
    a.set_max = (left_end.max(right_end) + a.set_scroll + 16 - view_bottom).max(0);
    if a.set_scroll > 0 {
        p.fill(&p.rect(x, view_top, w, 2), th().line, 1);
    }
    if a.set_scroll < a.set_max {
        p.fill(&p.rect(x, view_bottom - 2, w, 2), th().line, 1);
    }
}

/// Versión y estado de las actualizaciones, bajo el título.
fn paint_update(p: &Painter) {
    let a = app();
    let r = p.rect(70, 44, 200, 20);
    if let Upd::Available(rel) = &a.upd {
        let label = format!("↓ Actualizar a v{}", rel.version);
        let w = p.measure(&label, a.fonts.small) + 18;
        let pill = p.rect(68, 43, w, 22);
        p.fill(&pill, if is_hot(Act::Update) { th().accent_hot } else { th().accent }, 11);
        p.text(&label, &pill, th().on_accent, a.fonts.small, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        hit(pill, Act::Update);
        return;
    }
    let state = match &a.upd {
        Upd::Idle => "buscar actualizaciones".to_string(),
        Upd::Checking => "comprobando…".to_string(),
        Upd::UpToDate => "al día ✓".to_string(),
        Upd::Installing => "descargando actualización…".to_string(),
        Upd::Error(e) if e.contains("404") => "sin versiones publicadas".to_string(),
        Upd::Error(_) => "sin conexión · reintentar".to_string(),
        Upd::Available(_) => unreachable!(),
    };
    let clickable = matches!(a.upd, Upd::Idle | Upd::UpToDate | Upd::Error(_));
    let col = if clickable && is_hot(Act::Update) { th().text } else { th().sub };
    p.text(&format!("v{} · {}", update::VERSION, state), &r, col, a.fonts.small, DT_SINGLELINE | DT_END_ELLIPSIS);
    if clickable {
        hit(r, Act::Update);
    }
}

fn start_update_check() {
    let a = app();
    if matches!(a.upd, Upd::Checking | Upd::Installing) {
        return;
    }
    a.upd = Upd::Checking;
    redraw();
    let hwnd = a.hwnd as isize;
    std::thread::spawn(move || {
        let r = update::check();
        unsafe { PostMessageW(hwnd as HWND, WM_UPD_INFO, 0, Box::into_raw(Box::new(r)) as LPARAM) };
    });
}

fn start_update_install() {
    let a = app();
    let Upd::Available(rel) = std::mem::replace(&mut a.upd, Upd::Installing) else { return };
    set_status(&format!("Descargando la versión {}…", rel.version));
    let hwnd = a.hwnd as isize;
    std::thread::spawn(move || {
        let r = update::install(&rel).map(|exe| (exe, rel.version.clone()));
        unsafe { PostMessageW(hwnd as HWND, WM_UPD_DONE, 0, Box::into_raw(Box::new(r)) as LPARAM) };
    });
}

/// Arranca la versión nueva y cierra esta (suelta antes el bloqueo de instancia única).
fn restart_into(exe: &std::path::Path) {
    let a = app();
    let visible = unsafe { IsWindowVisible(a.hwnd) } != 0;
    unsafe {
        CloseHandle(a.mutex);
    }
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--updated");
    if !visible {
        cmd.arg("--tray");
    }
    if cmd.spawn().is_ok() {
        unsafe { DestroyWindow(a.hwnd) };
    } else {
        set_status("Actualizado: reinicia Centro LED para usar la versión nueva");
    }
}

fn paint_switch(p: &Painter, x: i32, y: i32, on: bool, act: Act) {
    let r = p.rect(x, y, 38, 20);
    p.fill(&r, if on { th().accent } else { th().line }, 10);
    let kx = if on { x + 21 } else { x + 3 };
    let knob = if on { th().on_accent } else if th().light { th().sub } else { th().text };
    if th().square {
        p.fill(&p.rect(kx, y + 3, 14, 14), knob, 2);
    } else {
        p.circle(&p.rect(kx, y + 3, 14, 14), knob, None);
    }
    hit(p.rect(x - 4, y - 4, 46, 28), act);
}

fn section(p: &Painter, title: &str, x: i32, y: i32) {
    p.text(title, &p.rect(x, y, 640, 18), th().sub, app().fonts.small, DT_SINGLELINE);
}

fn paint_device(p: &Painter, d: &Device, x: i32, w: i32) {
    let a = app();
    // Cabecera: nombre y, a la derecha, historial de batería y perfiles del dispositivo
    let mut bx = x + w;
    if d.led.is_some() && (d.battery.is_some() || last_battery(d).is_some()) {
        let label = if a.show_hist { "‹ Luces" } else { "Historial de batería" };
        bx -= p.measure(label, a.fonts.body) + 28;
        chip(p, label, bx, 26, 32, false, Act::HistToggle);
        bx -= 8;
    }
    if d.led.is_some() && !a.show_hist {
        let n = a.config.dprofiles.get(&d.key).map(|l| l.len()).unwrap_or(0);
        let label = if n == 0 { "Perfiles ▾".to_string() } else { format!("Perfiles ({n}) ▾") };
        bx -= p.measure(&label, a.fonts.body) + 28;
        chip(p, &label, bx, 26, 32, false, Act::DevProfiles);
        bx -= 8;
    }
    p.text(&d.name, &p.rect(x, 22, bx - x - 12, 34), th().text, a.fonts.title, DT_SINGLELINE | DT_END_ELLIPSIS);
    let mut sub = format!("{} · {}", d.kind, d.conn);
    if d.battery.is_some() {
        sub += &format!(" · batería {}", battery_text(d));
    }
    p.text(&sub, &p.rect(x, 58, w, 20), th().sub, a.fonts.body, DT_SINGLELINE);

    let Some(caps) = d.led.clone() else {
        paint_battery_only(p, d, x, w);
        return;
    };
    if a.show_hist {
        paint_battery_only(p, d, x, w);
        return;
    }
    let keyboard = is_keyboard(d);
    let mut y = 96;
    // Pestañas: Teclas / Borde en el teclado, o una por zona de luz.
    let tab_names: Vec<String> = if keyboard && has_rim(d) { vec!["Teclas".into(), "Borde".into()] } else if keyboard { Vec::new() } else { caps.zones.clone() };
    if !tab_names.is_empty() {
        let widths: Vec<i32> = tab_names.iter().map(|n| (p.measure(n, a.fonts.bold) + 40).max(116)).collect();
        let tabs = p.rect(x, y, widths.iter().sum::<i32>() + 8, 36);
        p.fill(&tabs, th().card, 10);
        p.stroke(&tabs, th().border, 10, 1);
        let mut tx = x + 4;
        for (i, name) in tab_names.iter().enumerate() {
            let r = p.rect(tx, y + 4, widths[i], 28);
            if a.tab == i as u8 {
                p.fill(&r, th().accent, 8);
            } else if is_hot(Act::Tab(i as u8)) {
                p.fill(&r, th().card_hot, 8);
            }
            p.text(name, &r, if a.tab == i as u8 { th().on_accent } else { th().text }, a.fonts.bold, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
            hit(r, Act::Tab(i as u8));
            tx += widths[i];
        }
        if !keyboard {
            p.text("Cada zona tiene su propio efecto y color", &p.rect(tx + 20, y, 400, 36), th().sub, a.fonts.small, DT_SINGLELINE | DT_VCENTER);
        }
    }
    if keyboard {
        y += if tab_names.is_empty() { 4 } else { 52 };
        a.preview = Some((true, x, y, w));
        y = paint_keyboard(p, x, y, w) + 18;
    } else {
        if !tab_names.is_empty() {
            y += 44;
        }
        a.preview = Some((false, x, y, w));
        y = paint_device_hero(p, d, x, y, w) + 18;
    }

    let switch = matches!(d.driver, devices::Driver::Switch { .. });
    let custom = !switch && keyboard && a.tab == 0 && editing().mode == MODE_CUSTOM;
    if switch {
        paint_pad_extras(p, x, y, w, true);
    } else {
        // Efectos
        let rim_tab = keyboard && a.tab == 1;
        let modes: Vec<(&str, u8)> = if rim_tab { RIM_MODES.to_vec() } else { caps.modes.clone() };
        let e = *editing();
        section(p, if keyboard { "EFECTO · LA VISTA PREVIA DE ARRIBA ES APROXIMADA" } else { "EFECTO" }, x, y);
        y += 22;
        let mut cx = x;
        for (name, id) in modes {
            let tw = p.measure(name, a.fonts.body) + 28;
            if cx + tw > x + w {
                cx = x;
                y += 38;
            }
            let r = p.rect(cx, y, tw, 30);
            let on = e.mode == id;
            p.fill(&r, if on { th().accent } else if is_hot(Act::Mode(id)) { th().card_hot } else { th().card }, 15);
            if !on {
                p.stroke(&r, th().border, 15, 1);
            }
            p.text(name, &r, if on { th().on_accent } else { mix(th().sub, th().text, 0.5) }, a.fonts.body, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
            hit(r, Act::Mode(id));
            cx += tw + 8;
        }
        y += 50;

        // Color
        let custom = !rim_tab && keyboard && e.mode == MODE_CUSTOM;
        let cycle = !keyboard && cycle_editing();
        if cycle {
            // Lista de colores del ciclo (clic para quitar)
            section(p, &format!("COLORES DEL CICLO ({}/{}) · CLIC EN UNO PARA QUITARLO", a.cycle.len(), MAX_CYCLE), x, y);
            y += 22;
            let mut cx = x;
            for (i, c) in a.cycle.iter().enumerate() {
                let r = p.rect(cx, y, 34, 34);
                p.circle(&r, to_ref(*c), Some((th().text, 2)));
                if is_hot(Act::CycleRemove(i)) {
                    p.circle(&r, mix(to_ref(*c), th().bg, 0.55), Some((th().text, 2)));
                    p.text("×", &r, th().text, a.fonts.h2, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                }
                hit(r, Act::CycleRemove(i));
                if i + 1 < a.cycle.len() {
                    p.text("›", &p.rect(cx + 34, y, 18, 34), th().sub, a.fonts.h2, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                }
                cx += 52;
            }
            if a.cycle.len() < 2 {
                p.text("Añade al menos 2 colores", &p.rect(cx, y, 300, 34), th().amber, a.fonts.body, DT_SINGLELINE | DT_VCENTER);
            }
            y += 50;
        }
        let no_color = matches!(e.mode, MODE_SW_RAINBOW | MODE_OFF) || (!keyboard && matches!(e.mode, 0x03 | 0x0E));
        if no_color {
            section(p, "COLOR", x, y);
            p.text("Este efecto recorre todos los colores por sí mismo.", &p.rect(x, y + 22, w, 30), th().sub, a.fonts.body, DT_SINGLELINE | DT_VCENTER);
        }
        section(p, if no_color { "" } else if custom { "COLOR DEL PINCEL" } else if cycle { "AÑADIR COLOR" } else { "COLOR" }, x, y);
        y += 22;
        let mut cx = x;
        for c in if no_color { &[][..] } else { &PALETTE[..] } {
            let c = *c;
            let r = p.rect(cx, y, 30, 30);
            let current = !cycle && !e.multi && [e.rgb.0, e.rgb.1, e.rgb.2] == c;
            if current || is_hot(Act::Color(c)) {
                p.circle(&p.rect(cx - 4, y - 4, 38, 38), th().bg, Some((if current { th().text } else { th().sub }, 2)));
            }
            p.circle(&r, to_ref(c), swatch_edge(to_ref(c)));
            hit(r, Act::Color(c));
            cx += 40;
        }
        // Color libre
        let r = if no_color { RECT { left: 0, top: 0, right: 0, bottom: 0 } } else { p.rect(cx, y, 30, 30) };
        let in_palette = cycle || PALETTE.contains(&[e.rgb.0, e.rgb.1, e.rgb.2]);
        if !no_color {
            p.circle(&r, if in_palette { th().card } else { rgb(e.rgb.0, e.rgb.1, e.rgb.2) }, Some((if is_hot(Act::MoreColors) { th().text } else { th().sub }, 2)));
            if in_palette {
                p.text("+", &r, th().text, a.fonts.h2, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
            }
            hit(r, Act::MoreColors);
        }
        cx += 52;
        if custom {
            for (label, act) in [("Rellenar todo", Act::FillAll), ("Apagar todo", Act::ClearAll)] {
                let tw = p.measure(label, a.fonts.body) + 28;
                let r = p.rect(cx, y, tw, 30);
                p.fill(&r, if is_hot(act) { th().card_hot } else { th().card }, 8);
                p.stroke(&r, th().border, 8, 1);
                p.text(label, &r, th().text, a.fonts.body, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                hit(r, act);
                cx += tw + 8;
            }
        } else if caps.multicolor || rim_tab {
            paint_switch(p, cx, y + 5, e.multi, Act::Multi);
            p.text("Arcoíris", &p.rect(cx + 46, y, 100, 30), th().sub, a.fonts.body, DT_SINGLELINE | DT_VCENTER);
        }
        y += 48;

        // Brillo / velocidad
        let half = (w - 40) / 2;
        paint_slider(p, "Brillo", e.bright, x, y, half, 0, true);
        let speed_on = caps.speed && !custom && !matches!(e.mode, 1 | 24 | 25 | MODE_OFF);
        let speed_label = if cycle { "Velocidad del ciclo" } else { "Velocidad" };
        paint_slider(p, speed_label, e.speed, x + half + 40, y, half, 1, speed_on);
        y += 62;
        if caps.pad_extras {
            paint_pad_extras(p, x, y, w, false);
        }
    }

    // Barra inferior: estado + aplicar
    let by = WIN_H - 64;
    let hint = if custom { "Clic o arrastra sobre las teclas para pintarlas · clic derecho copia su color" } else { "" };
    let status = if a.status.is_empty() { hint } else { a.status.as_str() };
    let mut status_line = String::from(status);
    if a.dirty && !a.applying {
        status_line = format!("● Cambios sin guardar   {status}");
    }
    p.text(&status_line, &p.rect(x, by, w - 200, 40), if a.dirty { th().amber } else { th().sub }, a.fonts.small, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
    let ar = p.rect(x + w - 180, by, 180, 40);
    let label = if a.applying { "Aplicando…" } else { "Aplicar" };
    p.fill(&ar, if a.applying { th().line } else if is_hot(Act::Apply) { th().accent_hot } else { th().accent }, 10);
    p.text(label, &ar, if a.applying { th().text } else { th().on_accent }, a.fonts.bold, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    hit(ar, Act::Apply);
}

/// LEDs de jugador con su brillo y LED del micrófono (DualSense), o LEDs de jugador y
/// luz del botón Home (mandos de Switch).
fn paint_pad_extras(p: &Painter, x: i32, y: i32, w: i32, switch: bool) {
    let a = app();
    if switch {
        section(p, "LEDS DE JUGADOR · CLIC PARA ENCENDER O APAGAR", x, y);
        section(p, "LUZ DEL BOTÓN HOME (MANDO PRO Y JOY-CON D)", x + w / 2 + 20, y);
        let y = y + 22;
        for i in 0..4u8 {
            let on = a.pad.player & (1 << i) != 0;
            let r = p.rect(x + i as i32 * 30, y + 9, 14, 14);
            p.fill(&r, if on { rgb(0x6C, 0xF0, 0x5A) } else { th().line }, 7);
            if is_hot(Act::PlayerLed(i)) {
                p.stroke(&r, th().accent, 7, 2);
            }
            hit(p.rect(x + i as i32 * 30 - 6, y, 28, 32), Act::PlayerLed(i));
        }
        let mut cx = x + w / 2 + 20;
        for (n, label) in ["Apagada", "Baja", "Media", "Máxima"].iter().enumerate() {
            let tw = p.measure(label, a.fonts.small) + 20;
            let r = p.rect(cx, y + 2, tw, 26);
            let on = a.pad.player_bright == n as u8;
            p.fill(&r, if on { th().accent } else if is_hot(Act::PlayerBright(n as u8)) { th().card_hot } else { th().card }, 13);
            p.text(label, &r, if on { th().on_accent } else { mix(th().sub, th().text, 0.5) }, a.fonts.small, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
            hit(r, Act::PlayerBright(n as u8));
            cx += tw + 6;
        }
        return;
    }
    section(p, "LEDS DE JUGADOR · CLIC PARA ENCENDER O APAGAR", x, y);
    section(p, "LED DEL MICRÓFONO", x + w / 2 + 20, y);
    let y = y + 22;
    for i in 0..5u8 {
        let on = a.pad.player & (1 << i) != 0;
        let r = p.rect(x + i as i32 * 30, y + 9, 22, 12);
        p.fill(&r, if on { rgb(0xF4, 0xF4, 0xFF) } else { th().line }, 6);
        if on && th().light {
            p.stroke(&r, th().sub, 6, 1);
        }
        if is_hot(Act::PlayerLed(i)) {
            p.stroke(&r, th().accent, 6, 2);
        }
        hit(p.rect(x + i as i32 * 30 - 4, y, 30, 30), Act::PlayerLed(i));
    }
    let mut cx = x + 160;
    for (n, label) in ["Alto", "Medio", "Bajo"].iter().enumerate() {
        let tw = p.measure(label, a.fonts.small) + 20;
        let r = p.rect(cx, y + 2, tw, 26);
        let on = a.pad.player_bright == n as u8;
        p.fill(&r, if on { th().accent } else if is_hot(Act::PlayerBright(n as u8)) { th().card_hot } else { th().card }, 13);
        p.text(label, &r, if on { th().on_accent } else { mix(th().sub, th().text, 0.5) }, a.fonts.small, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        hit(r, Act::PlayerBright(n as u8));
        cx += tw + 6;
    }
    let mut cx = x + w / 2 + 20;
    for (n, label) in ["Apagado", "Encendido", "Parpadeo"].iter().enumerate() {
        let tw = p.measure(label, a.fonts.small) + 20;
        let r = p.rect(cx, y + 2, tw, 26);
        let on = a.pad.mic == n as u8;
        let col = if on { if n == 0 { th().accent } else { rgb(0xE8, 0x6A, 0x1A) } } else if is_hot(Act::Mic(n as u8)) { th().card_hot } else { th().card };
        p.fill(&r, col, 13);
        p.text(label, &r, if on { rgb(0xFF, 0xFF, 0xFF) } else { mix(th().sub, th().text, 0.5) }, a.fonts.small, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        hit(r, Act::Mic(n as u8));
        cx += tw + 6;
    }
}

fn paint_slider(p: &Painter, label: &str, value: u8, x: i32, y: i32, w: i32, id: u8, enabled: bool) {
    let a = app();
    let col = if enabled { th().sub } else { th().line };
    p.text(&label.to_uppercase(), &p.rect(x, y, 200, 18), col, a.fonts.small, DT_SINGLELINE);
    p.text(&format!("{value} %"), &p.rect(x + w - 80, y, 80, 18), col, a.fonts.small, DT_SINGLELINE | DT_RIGHT);
    let ty = y + 30;
    p.fill(&p.rect(x, ty, w, 6), th().line, 3);
    if enabled {
        let fw = (w * value as i32 / 100).max(6);
        p.fill(&p.rect(x, ty, fw, 6), th().accent, 3);
        let kx = x + fw - 9;
        let knob = if th().light { rgb(0xFF, 0xFF, 0xFF) } else { th().text };
        let border = if is_hot(Act::Slider(id)) { Some((th().accent, 3)) } else if th().light { Some((th().line, 1)) } else { None };
        if th().square {
            p.fill(&p.rect(kx + 3, ty - 6, 12, 18), knob, 2);
            if let Some((c, w)) = border {
                p.stroke(&p.rect(kx + 3, ty - 6, 12, 18), c, 2, w);
            }
        } else {
            p.circle(&p.rect(kx, ty - 6, 18, 18), knob, border);
        }
        hit(p.rect(x - 8, ty - 12, w + 16, 30), Act::Slider(id));
    }
}

// ------------------------------------------------------------------ simulación de efectos (vista previa)

const TAU: f32 = std::f32::consts::TAU;

/// Número pseudoaleatorio estable en [0, 1) para (a, b).
fn rnd(a: u32, b: u32) -> f32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77) ^ 0x27D4_EB2F;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0xFFFF) as f32 / 65536.0
}

/// Segundos desde que arrancó la app (reloj de las animaciones).
fn anim_time() -> f32 {
    app().t0.elapsed().as_secs_f32()
}

/// Tiempo escalado por la velocidad del efecto (0..100).
fn effect_time(speed: u8) -> f32 {
    anim_time() * (0.3 + speed as f32 / 100.0 * 1.4)
}

/// Pulsos/destellos aleatorios que se apagan poco a poco (reactivo, estrellas).
fn flashes(i: u32, tt: f32, rate: f32, prob: f32, life: f32) -> f32 {
    let slot = (tt * rate).floor();
    let mut k: f32 = 0.0;
    for j in 0..(life * rate).ceil() as i32 + 1 {
        let s = slot - j as f32;
        if s >= 0.0 && rnd(i, s as u32) < prob {
            let age = (tt * rate - s) / (life * rate);
            k = k.max(1.0 - age.clamp(0.0, 1.0));
        }
    }
    k
}

/// Color e intensidad (0..1) que tendría la tecla `i` (posición normalizada x, y) en el instante actual.
fn key_effect(i: usize, x: f32, y: f32) -> ([u8; 3], f32) {
    let a = app();
    let e = a.cur;
    let base = [e.rgb.0, e.rgb.1, e.rgb.2];
    let tt = effect_time(e.speed);
    let col = |h: f32| if e.multi { hue(h) } else { base };
    let (dx, dy) = ((x - 0.5) * 4.0, y - 0.5);
    let iu = i as u32;
    match e.mode {
        MODE_OFF => ([0; 3], 0.0),
        MODE_CUSTOM => (a.keys[i], 1.0),
        1 => (col(x), 1.0),
        // Respiración: sube y baja; en arcoíris cambia de color en cada respiración
        7 => {
            let ph = tt * 0.45;
            (if e.multi { hue((ph.floor() * 0.17).fract()) } else { base }, 0.5 - 0.5 * (ph * TAU).cos())
        }
        8 => (hue(tt * 0.12), 1.0),
        11 => (hue(x - tt * 0.3), 1.0),
        9 => (hue((x - 0.5).abs() * 1.4 - tt * 0.3), 1.0),
        6 => (hue(rnd(iu, 7) + tt * 0.05), 1.0),
        2 => (col(rnd(iu, 3)), flashes(iu, tt, 3.0, 0.05, 0.9)),
        3 => (col(rnd(iu, 3)), 1.0 - flashes(iu, tt, 3.0, 0.05, 0.9)),
        4 => (col(rnd(iu, 5)), flashes(iu, tt, 1.5, 0.08, 1.4)),
        5 => {
            let column = (x * 22.0).floor() as u32;
            let p = (tt * 0.5 + rnd(column, 9)).fract() * 1.3 - 0.15;
            (col(rnd(column, 4)), (1.0 - (y - p).abs() * 5.0).max(0.0))
        }
        10 => {
            if e.multi {
                (hue(y * 0.8 - tt * 0.3), 1.0)
            } else {
                (base, 0.5 + 0.5 * (TAU * (y - tt * 0.35)).sin())
            }
        }
        12 => (hue(dy.atan2(dx) / TAU + tt * 0.2), 1.0),
        13 => {
            let pos = (x - 0.5).abs() * 2.0;
            let front = (tt * 0.45).fract() * 1.3;
            let c = if e.multi { hue((tt * 0.45).floor() * 0.17) } else { base };
            (c, (1.0 - (pos - front).abs() * 5.0).max(0.0))
        }
        14 | 15 => {
            // Simula pulsaciones en teclas al azar; la luz se expande desde la tecla pulsada.
            let rate = 0.9;
            let mut best = ([0u8; 3], 0.0f32);
            for back in 0..2 {
                let slot = (tt * rate).floor() - back as f32;
                if slot < 0.0 {
                    continue;
                }
                let age = tt * rate - slot; // 0..2
                let pick = (rnd(slot as u32, 11) * kb_layout::KEYS.len() as f32) as usize % kb_layout::KEYS.len();
                let (_, _, kx, ky, kw2, kh2) = kb_layout::KEYS[pick];
                let (px, py) = ((kx as f32 + kw2 as f32 / 2.0) / 673.0, (ky as f32 + kh2 as f32 / 2.0) / 168.0);
                let d = (((x - px) * 4.0).powi(2) + (y - py).powi(2)).sqrt();
                let radius = age * 1.6;
                let fade = (1.0 - age / 2.0).max(0.0);
                let k = if e.mode == 15 {
                    (1.0 - (d - radius).abs() * 3.0).max(0.0) * fade // anillo
                } else if d <= radius {
                    fade // explosión: disco que se llena y se apaga
                } else {
                    0.0
                };
                if k > best.1 {
                    best = (col(rnd(slot as u32, 3)), k);
                }
            }
            best
        }
        16 => {
            let rows = 6.0;
            let prog = tt * 0.12;
            let cycle = prog.floor();
            let p = prog.fract() * rows;
            let (row, cur) = ((y * rows).floor().min(rows - 1.0), p.floor());
            let done = row < cur || (row == cur && x <= p.fract());
            let pick = |n: f32| if e.multi { hue((n * 0.17).fract()) } else if n as i32 % 2 == 0 { base } else { [0; 3] };
            let c = if done { pick(cycle + 1.0) } else { pick(cycle) };
            (c, if c == [0; 3] { 0.0 } else { 1.0 })
        }
        17 => (col(tt * 0.1), (0.5 + 0.5 * (TAU * tt).sin()).powi(3)),
        18 => {
            let diag = (x * 4.0 + (1.0 - y)) / 5.0; // 0 abajo-izquierda .. 1 arriba-derecha
            let f = (diag * 2.5 - tt * 0.4).rem_euclid(1.0);
            (if e.multi { hue(diag) } else { base }, (1.0 - (f - 0.25).abs() * 4.0).max(0.0))
        }
        19 => {
            let row = (y * 6.0).floor() as i32;
            let dir = if row % 2 == 0 { 1.0 } else { -1.0 };
            (col(y), 0.5 + 0.5 * (TAU * (x * 2.0 + dir * tt * 0.5)).sin())
        }
        _ => (base, 1.0),
    }
}

/// Color del borde en la posición `u` (0..1 recorriendo el perímetro) en el instante actual.
fn rim_effect(u: f32) -> ([u8; 3], f32) {
    let r = app().rim;
    let base = [r.rgb.0, r.rgb.1, r.rgb.2];
    let tt = effect_time(r.speed);
    match r.mode {
        25 => ([0; 3], 0.0),
        20 => (hue(u * 2.0 - tt * 0.3), 1.0),
        21 => (hue(0.33 + 0.22 * (TAU * (u * 2.0 + tt * 0.12)).sin() + 0.1 * (TAU * (u * 5.0 - tt * 0.2)).sin()), 0.75 + 0.25 * (TAU * (u * 3.0 + tt * 0.3)).sin()),
        22 => (hue(tt * 0.12), 1.0),
        23 => {
            let ph = tt * 0.45;
            (if r.multi { hue((ph.floor() * 0.17).fract()) } else { base }, 0.5 - 0.5 * (ph * TAU).cos())
        }
        _ => (if r.multi { hue(u) } else { base }, 1.0),
    }
}

/// ¿Se mueve algo en la vista previa del dispositivo seleccionado?
fn preview_animates() -> bool {
    let a = app();
    if a.page != 1 {
        return false;
    }
    match selected() {
        Some(d) if has_rim(&d) => !matches!(a.cur.mode, 1 | MODE_CUSTOM | MODE_OFF) || !matches!(a.rim.mode, 24 | 25),
        Some(d) if is_keyboard(&d) => false,
        Some(d) => d.led.is_some() && a.zones.is_empty() && is_app_animated(a.cur.mode) && (a.cur.mode != MODE_CYCLE || a.cycle.len() >= 2),
        None => false,
    }
}

fn paint_keyboard(p: &Painter, x: i32, y: i32, w: i32) -> i32 {
    let a = app();
    let (kw, kh) = (673.0, 168.0);
    let k = ((w - 60) as f32 / kw).min(1.25);
    let ox = x + ((w as f32 - kw * k) / 2.0) as i32;
    let oy = y + 22;
    let frame = p.rect(ox - 22, y, (kw * k) as i32 + 44, (kh * k) as i32 + 44);
    a.anim_rect = RECT { left: frame.left - s(14), top: frame.top - s(14), right: frame.right + s(14), bottom: frame.bottom + s(14) };

    // Borde (tira de luz): resplandor exterior y trazo por segmentos con el efecto simulado.
    let rim = selected().map(|d| has_rim(&d)).unwrap_or(false);
    let rb = a.rim.bright as f32 / 100.0;
    let (glow_c, glow_k) = rim_effect(0.0);
    if rim && a.rim.mode != 25 {
        for g in 0..3 {
            let grow = 3 + g * 3;
            let fr = RECT { left: frame.left - s(grow), top: frame.top - s(grow), right: frame.right + s(grow), bottom: frame.bottom + s(grow) };
            p.stroke(&fr, mix(th().bg, to_ref(glow_c), (0.3 - g as f32 * 0.09) * rb * glow_k), 22 + grow, 2);
        }
    }
    p.fill(&frame, rgb(0x16, 0x16, 0x1D), 18);
    p.stroke(&frame, th().line, 18, 1);
    if rim && a.rim.mode != 25 {
        let inset = s(3);
        let (l, t, r, b) = (frame.left + inset, frame.top + inset, frame.right - inset, frame.bottom - inset);
        let (fw, fh) = ((r - l) as f32, (b - t) as f32);
        let per = 2.0 * (fw + fh);
        let point = |d: f32| -> (i32, i32) {
            let d = d.rem_euclid(per);
            if d < fw {
                (l + d as i32, t)
            } else if d < fw + fh {
                (r, t + (d - fw) as i32)
            } else if d < 2.0 * fw + fh {
                (r - (d - fw - fh) as i32, b)
            } else {
                (l, b - (d - 2.0 * fw - fh) as i32)
            }
        };
        let segs = 96;
        for i in 0..segs {
            let u = i as f32 / segs as f32;
            let (c, kk) = rim_effect(u);
            let col = mix(rgb(0x16, 0x16, 0x1D), to_ref(c), (0.2 + 0.8 * rb) * kk);
            let (x0, y0) = point(u * per);
            let (x1, y1) = point((i + 1) as f32 / segs as f32 * per);
            unsafe {
                let pen = CreatePen(PS_SOLID, s(3), col);
                let old = SelectObject(p.dc, pen);
                MoveToEx(p.dc, x0, y0, null_mut());
                LineTo(p.dc, x1, y1);
                SelectObject(p.dc, old);
                DeleteObject(pen);
            }
        }
    }
    if rim && a.tab == 1 {
        hit(frame, Act::Tab(1));
    }

    // Teclas con el efecto simulado
    let small_labels = k < 1.05;
    let kb_bright = if a.cur.mode == MODE_CUSTOM { a.cur.bright } else { a.cur.bright.max(8) } as f32 / 100.0;
    for &(idx, label, kx, ky, kw2, kh2) in kb_layout::KEYS {
        let lx = ox + (kx as f32 * k) as i32;
        let ly = oy + (ky as f32 * k) as i32;
        let r = p.rect(lx, ly, (kw2 as f32 * k) as i32, (kh2 as f32 * k) as i32);
        let cx = (kx as f32 + kw2 as f32 / 2.0) / kw;
        let cy = (ky as f32 + kh2 as f32 / 2.0) / kh;
        let (c, kk) = key_effect(idx, cx, cy);
        let col = mix(KEYCAP, to_ref(c), (0.15 + 0.85 * kb_bright) * kk);
        let lit = kk > 0.15;
        p.fill(&r, mix(KEYCAP, col, 0.32), 4);
        let hot = a.tab == 0 && is_hot(Act::Key(idx));
        p.stroke(&r, if hot { KB_TEXT } else { mix(KB_LINE, col, kk.max(0.2)) }, 4, if hot { 2 } else { 1 });
        let font = if label.contains('\n') || label.chars().count() > 4 || small_labels { a.fonts.key_small } else { a.fonts.key };
        p.text_center(label, &r, if lit { mix(KB_TEXT, col, 0.25) } else { KB_SUB }, font);
        if a.tab == 0 {
            hit(r, Act::Key(idx));
        }
    }
    y + (kh * k) as i32 + 44
}

fn paint_device_hero(p: &Painter, d: &Device, x: i32, y: i32, w: i32) -> i32 {
    let a = app();
    let e = *editing();
    let cycle = a.zones.is_empty() && e.mode == MODE_CYCLE && a.cycle.len() >= 2;
    let col = if e.mode == MODE_OFF {
        th().line
    } else if a.zones.is_empty() && is_app_animated(e.mode) {
        to_ref(anim_color(&e, &a.cycle, anim_time()))
    } else if e.multi || matches!(e.mode, 0x03 | 0x0E) && !a.zones.is_empty() {
        to_ref(hue(0.75))
    } else {
        rgb(e.rgb.0, e.rgb.1, e.rgb.2)
    };
    if cycle {
        // Vista previa del ciclo: degradado suave entre los colores elegidos.
        let (bx, bw) = (x + w / 2 + 110, w / 2 - 130);
        let n = a.cycle.len();
        for px in 0..bw {
            let t = px as f32 / bw as f32 * n as f32;
            let i = t.floor() as usize % n;
            let f = t.fract();
            let f = f * f * (3.0 - 2.0 * f);
            let c = mix(to_ref(a.cycle[i]), to_ref(a.cycle[(i + 1) % n]), f);
            p.fill(&p.rect(bx + px, y + 74, 1, 26), mix(th().bg, c, 0.2 + 0.8 * e.bright as f32 / 100.0), 0);
        }
        section(p, "VISTA PREVIA DEL CICLO", bx, y + 50);
    }
    let size = if d.led.as_ref().map(|l| l.pad_extras).unwrap_or(false) { 110 } else { 150 };
    let cx = x + (w - size) / 2;
    a.anim_rect = p.rect(cx - 30, y - 20, size + 60, size + 70);
    let bright = e.bright as f32 / 100.0;
    for g in (0..4).rev() {
        let grow = g * 9;
        p.circle(&p.rect(cx - grow, y - grow + 14, size + grow * 2, size + grow * 2), mix(th().bg, col, (0.12 + (3 - g) as f32 * 0.07) * bright), None);
    }
    p.circle(&p.rect(cx + 10, y + 24, size - 20, size - 20), th().card, Some((mix(th().bg, col, 0.3 + 0.7 * bright), 4)));
    p.text(glyph(d), &p.rect(cx, y + 14, size, size), th().text, a.fonts.icon_big, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    y + size + 30
}

fn paint_battery_only(p: &Painter, d: &Device, x: i32, w: i32) {
    let a = app();
    let card = p.rect(x, 104, w, 176);
    p.fill(&card, th().card, 16);
    p.stroke(&card, th().border, 16, 1);
    p.text(glyph(d), &p.rect(x + 30, 122, 80, 80), th().text, a.fonts.icon_big, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    match d.battery {
        Some(b) => {
            let txt = if b.approx { format!("≈ {} %", b.pct) } else { format!("{} %", b.pct) };
            p.text(&txt, &p.rect(x + 140, 122, 300, 56), th().text, a.fonts.title, DT_SINGLELINE | DT_VCENTER);
            let mut state = (if b.charging { "Cargando" } else if b.approx { "Nivel aproximado" } else { "Batería" }).to_string();
            match history::estimate(d) {
                Some((true, secs)) => state += &format!(" · se llenará en ≈ {}", history::fmt_dur(secs)),
                Some((false, secs)) => state += &format!(" · quedan ≈ {} al ritmo de uso actual", history::fmt_dur(secs)),
                None if !b.approx => state += " · la estimación aparece tras un rato de uso",
                None => {}
            }
            p.text(&state, &p.rect(x + 140, 178, w - 170, 22), th().sub, a.fonts.body, DT_SINGLELINE | DT_END_ELLIPSIS);
            let bw = w - 170;
            p.fill(&p.rect(x + 140, 226, bw, 14), th().line, 7);
            p.fill(&p.rect(x + 140, 226, (bw * b.pct as i32 / 100).max(14), 14), battery_color(b.pct), 7);
        }
        None => {
            p.text(&battery_text(d), &p.rect(x + 140, 134, w - 170, 40), th().text, a.fonts.h2, DT_SINGLELINE | DT_VCENTER);
            if d.led.is_none() {
                p.text("Este dispositivo no tiene luces configurables.", &p.rect(x + 140, 180, w - 170, 22), th().sub, a.fonts.body, DT_SINGLELINE);
            }
        }
    }
    let has_hist = d.battery.is_some() || !history::get(&d.key).is_empty();
    if has_hist {
        section(p, "HISTORIAL DE BATERÍA", x, 300);
        let mut cx = x + w;
        for (label, days) in [("7 días", 7u8), ("24 h", 1)] {
            let tw = p.measure(label, a.fonts.small) + 22;
            cx -= tw;
            let r = p.rect(cx, 294, tw, 26);
            let on = a.hist_days == days as u64;
            p.fill(&r, if on { th().accent } else if is_hot(Act::HistRange(days)) { th().card_hot } else { th().card }, 13);
            p.text(label, &r, if on { th().on_accent } else { th().text }, a.fonts.small, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
            hit(r, Act::HistRange(days));
            cx -= 6;
        }
        let gr = p.rect(x, 326, w, WIN_H - 80 - 326);
        p.fill(&gr, th().card, 16);
        p.stroke(&gr, th().border, 16, 1);
        history::paint_graph(p, &d.key, x + 16, 372, w - 40, WIN_H - 80 - 372 - 44, a.hist_days);
    } else if d.kind == "Placa base" {
        let hint = "Para controlar sus luces (y las de RAM, gráfica, ventiladores…), instala OpenRGB, \
                    actívale el servidor SDK (pestaña SDK Server › Start Server) y déjalo abierto: \
                    sus dispositivos aparecerán aquí en la lista.";
        p.text(hint, &p.rect(x + 30, 310, w - 60, 80), th().sub, a.fonts.body, DT_WORDBREAK);
    }
    if !a.status.is_empty() {
        p.text(&a.status, &p.rect(x, WIN_H - 64, w, 40), th().sub, a.fonts.small, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
    }
}

// ------------------------------------------------------------------ ratón

fn hit_test(x: i32, y: i32) -> Option<Act> {
    app().hits.iter().rev().find(|(r, _)| x >= r.left && x < r.right && y >= r.top && y < r.bottom).map(|(_, a)| *a)
}

fn slider_set(id: u8, x: i32) {
    let a = app();
    if let Some((r, _)) = a.hits.iter().find(|(_, act)| *act == Act::Slider(id)) {
        let left = r.left + s(8);
        let width = (r.right - s(8)) - left;
        let v = ((x - left) as f32 / width as f32 * 100.0).round().clamp(0.0, 100.0) as u8;
        let e = editing();
        if id == 0 {
            e.bright = v;
        } else {
            e.speed = v;
        }
        a.dirty = true;
        redraw();
    }
}

fn paint_key(i: usize) {
    let a = app();
    // Al pintar una tecla se pasa al modo personalizado.
    a.cur.mode = MODE_CUSTOM;
    let c = [a.cur.rgb.0, a.cur.rgb.1, a.cur.rgb.2];
    if a.keys[i] != c {
        a.keys[i] = c;
        a.dirty = true;
    }
    redraw();
}

fn on_click(act: Act, x: i32) {
    let a = app();
    match act {
        Act::Device(i) => {
            a.page = 1;
            select_device(i);
        }
        Act::Home => {
            a.page = 0;
            redraw();
        }
        Act::Settings => {
            a.page = 2;
            redraw();
        }
        Act::Theme(i) => {
            a.config.theme = i;
            save_config(&a.config);
            apply_theme();
        }
        Act::Accent(i) => {
            a.config.accent = i;
            save_config(&a.config);
            apply_theme();
        }
        Act::Corners(sq) => {
            a.config.square = sq;
            save_config(&a.config);
            apply_theme();
        }
        Act::Widget => {
            a.config.hide_widget = !a.config.hide_widget;
            save_config(&a.config);
            update_widgets();
            redraw();
        }
        Act::LowBattery(v) => {
            a.config.low_battery = v;
            a.low_warned.clear();
            save_config(&a.config);
            redraw();
        }
        Act::OpenUrl => unsafe {
            ShellExecuteW(null_mut(), wide("open").as_ptr(), wide(&format!("https://github.com/{}", update::REPO)).as_ptr(), null(), null(), SW_SHOWNORMAL);
        },
        Act::Tab(t) => {
            a.tab = t;
            redraw();
        }
        Act::Mode(m) => {
            editing().mode = m;
            a.dirty = true;
            redraw();
        }
        Act::Color(c) => set_color(c),
        Act::MoreColors => pick_more_colors(),
        Act::Multi => {
            let e = editing();
            e.multi = !e.multi;
            a.dirty = true;
            redraw();
        }
        Act::Slider(id) => {
            a.drag = Some(act);
            unsafe { SetCapture(a.hwnd) };
            slider_set(id, x);
        }
        Act::Key(i) => {
            a.drag = Some(act);
            unsafe { SetCapture(a.hwnd) };
            paint_key(i);
        }
        Act::FillAll => {
            let c = [a.cur.rgb.0, a.cur.rgb.1, a.cur.rgb.2];
            *a.keys = [c; KB_LEDS];
            a.dirty = true;
            redraw();
        }
        Act::ClearAll => {
            *a.keys = [[0; 3]; KB_LEDS];
            a.dirty = true;
            redraw();
        }
        Act::PlayerLed(i) => {
            a.pad.player ^= 1 << i;
            a.dirty = true;
            redraw();
        }
        Act::PlayerBright(n) => {
            a.pad.player_bright = n;
            a.dirty = true;
            redraw();
        }
        Act::Mic(n) => {
            a.pad.mic = n;
            a.dirty = true;
            redraw();
        }
        Act::CycleRemove(i) => {
            if i < a.cycle.len() {
                a.cycle.remove(i);
                a.dirty = true;
            }
            redraw();
        }
        Act::Apply => on_apply(),
        Act::Scan => start_scan(),
        Act::Autostart => {
            if a.autostart || a.config.fast_start {
                set_autostart(false);
                if a.config.fast_start {
                    let _ = state::fast_start_set(false);
                    a.config.fast_start = false;
                    save_config(&a.config);
                }
                a.autostart = false;
            } else {
                set_autostart(true);
                a.autostart = true;
            }
            redraw();
        }
        Act::FastStart => {
            if a.config.fast_start {
                let _ = state::fast_start_set(false);
                a.config.fast_start = false;
                set_autostart(true);
                a.autostart = true;
                set_status("Inicio rápido desactivado: sigue iniciando con Windows de la forma normal");
            } else {
                match state::fast_start_set(true) {
                    Ok(()) => {
                        a.config.fast_start = true;
                        set_autostart(false);
                        a.autostart = false;
                        set_status("✓ Centro LED arrancará nada más iniciar sesión");
                    }
                    Err(e) => set_status(&e),
                }
            }
            save_config(&a.config);
            redraw();
        }
        Act::Profile(i) => state::apply_profile(i),
        Act::ProfileSave => state::save_new_profile(),
        Act::ProfileMore(i) => state::more_profiles_menu(i),
        Act::AllColor(c) => state::all_color(c),
        Act::AllToggle => state::toggle_all(),
        Act::DevProfiles => {
            if let Some(d) = selected() {
                state::device_profiles_menu(&d);
            }
        }
        Act::HistToggle => {
            a.show_hist = !a.show_hist;
            redraw();
        }
        Act::HistRange(n) => {
            a.hist_days = n as u64;
            redraw();
        }
        Act::AutoOpt(n) => {
            a.config.auto[n as usize] ^= true;
            save_config(&a.config);
            redraw();
        }
        Act::AutoDev(i) => {
            if let Some(d) = a.devices.get(i) {
                if !a.config.led.contains_key(&d.key) {
                    set_status(&format!("Guarda antes las luces de {} en su página para que pueda apagarse solo", d.name));
                } else if !a.config.auto_exclude.remove(&d.key) {
                    a.config.auto_exclude.insert(d.key.clone());
                }
                save_config(&a.config);
                redraw();
            }
        }
        Act::HkRecord(i) => {
            if let Some((action, _)) = state::hk_actions().get(i) {
                state::toggle_recording(action);
            }
            unsafe { SetFocus(a.hwnd) };
        }
        Act::HkClear(i) => {
            if let Some((action, _)) = state::hk_actions().get(i) {
                state::clear_hotkey(action);
            }
        }
        Act::Diagnostics => diag::export(),
        Act::Update => {
            if matches!(a.upd, Upd::Available(_)) {
                start_update_install();
            } else {
                start_update_check();
            }
        }
        Act::TrayBattery => {
            a.config.hide_tray_battery = !a.config.hide_tray_battery;
            save_config(&a.config);
            update_battery_icons();
            redraw();
        }
    }
}

// ------------------------------------------------------------------ resultados de los hilos

fn on_scanned(list: Vec<Device>) {
    let a = app();
    a.scanning = false;
    // Volver a aplicar: las luces que se pierden al reconectar (mandos, cascos), las que se
    // cambiaron mientras el dispositivo no estaba, los ciclos que se cortaron y, tras apagar el
    // equipo con el apagado automático, las de antes. Con las luces apagadas solas, se apagan.
    let restore = std::mem::take(&mut a.config.pending_restore);
    for d in &list {
        let Some(caps) = d.led.as_ref() else { continue };
        if !a.config.led.contains_key(&d.key) {
            continue;
        }
        let new = !a.known.contains(&d.key);
        let pending = a.pending.remove(&d.key);
        let st = state::reconnect_state(d);
        let dead_cycle = !new && st.led.map(|l| is_app_animated(l.mode)).unwrap_or(false) && !cycle_running(&d.key);
        if (caps.volatile && new) || pending || dead_cycle || (restore && new && state::auto_included(d)) {
            state::send(d, &st, true);
        }
    }
    if restore {
        save_config(&a.config);
    }
    // Sólo cuentan los que ya tenían luces: unos cascos que estaban apagados (sin luces visibles)
    // reciben su configuración al encenderse.
    a.known = list.iter().filter(|d| d.led.is_some()).map(|d| d.key.clone()).collect();
    for d in &list {
        if let Some(b) = d.battery {
            a.last_battery.insert(d.key.clone(), b.pct);
            history::record(&d.key, b.pct, b.charging);
            let limit = a.config.low_battery;
            if b.charging || b.pct >= limit.saturating_add(10) {
                a.low_warned.remove(&d.key);
            } else if limit > 0 && b.pct <= limit && !b.approx && a.low_warned.insert(d.key.clone()) {
                notify("Batería baja", &format!("{} está al {} %. Conviene cargarlo.", d.name, b.pct));
            }
        }
    }
    a.devices = list;
    state::seed_profiles();
    if std::mem::take(&mut a.first_scan) {
        // El inicio rápido arranca con prioridad alta; una vez listo, volver a la normal.
        unsafe { SetPriorityClass(GetCurrentProcess(), NORMAL_PRIORITY_CLASS) };
    }
    let still = a.sel.as_ref().map(|k| a.devices.iter().any(|d| &d.key == k)).unwrap_or(false);
    if !still {
        a.sel = None;
        if a.page == 1 {
            a.page = 0;
        }
    }
    let mut st: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut st) };
    set_status(&format!("{} dispositivos · actualizado a las {:02}:{:02}", a.devices.len(), st.wHour, st.wMinute));
    update_tray(false);
    update_battery_icons();
    update_widgets();
    // Windows crea su ficha del icono un instante después de añadirlo.
    unsafe { SetTimer(a.hwnd, TIMER_PROMOTE, 1500, None) };
    if a.quick_scans > 0 {
        a.quick_scans -= 1;
        unsafe { SetTimer(a.hwnd, TIMER_RETRY, 15_000, None) };
    }
}

// ------------------------------------------------------------------ bandeja del sistema e icono

unsafe fn pixels_to_icon(size: i32, px: &[u32]) -> HICON {
    let screen = GetDC(null_mut());
    let dc = CreateCompatibleDC(screen);
    let mut bmi: BITMAPINFO = std::mem::zeroed();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = size;
    bmi.bmiHeader.biHeight = -size;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    let mut bits: *mut core::ffi::c_void = null_mut();
    let color = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
    std::slice::from_raw_parts_mut(bits as *mut u32, px.len()).copy_from_slice(px);
    let mask = CreateBitmap(size, size, 1, 1, null());
    let info = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
    let icon = CreateIconIndirect(&info);
    DeleteObject(mask);
    DeleteObject(color);
    DeleteDC(dc);
    ReleaseDC(null_mut(), screen);
    icon
}

/// Dibuja con GDI a 4x (fondo magenta = transparente) y reduce promediando: bordes suaves y
/// transparencia real. Devuelve píxeles ARGB (alfa sin premultiplicar), de arriba abajo.
unsafe fn render_pixels(size: i32, draw: impl FnOnce(HDC, i32)) -> Vec<u32> {
    const SS: i32 = 4;
    let big = size * SS;
    let screen = GetDC(null_mut());
    let dc = CreateCompatibleDC(screen);
    let dib = |w: i32, bits: &mut *mut core::ffi::c_void| {
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = w;
        bmi.bmiHeader.biHeight = -w;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, bits, null_mut(), 0)
    };
    let mut big_bits: *mut core::ffi::c_void = null_mut();
    let big_bmp = dib(big, &mut big_bits);
    let old = SelectObject(dc, big_bmp);
    let all = RECT { left: 0, top: 0, right: big, bottom: big };
    let kb = CreateSolidBrush(rgb(0xFF, 0x00, 0xFF));
    FillRect(dc, &all, kb);
    DeleteObject(kb);
    SetBkMode(dc, TRANSPARENT as i32);
    draw(dc, big);
    GdiFlush();
    SelectObject(dc, old);
    let src = std::slice::from_raw_parts(big_bits as *const u32, (big * big) as usize);
    let mut dst = vec![0u32; (size * size) as usize];
    for y in 0..size {
        for x in 0..size {
            let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let v = src[((y * SS + sy) * big + x * SS + sx) as usize] & 0x00FF_FFFF;
                    if v != 0x00FF_00FF {
                        r += (v >> 16) & 0xFF;
                        g += (v >> 8) & 0xFF;
                        b += v & 0xFF;
                        n += 1;
                    }
                }
            }
            dst[(y * size + x) as usize] = if n == 0 {
                0
            } else {
                let alpha = n * 255 / (SS * SS) as u32;
                (alpha << 24) | ((r / n) << 16) | ((g / n) << 8) | (b / n)
            };
        }
    }
    DeleteObject(big_bmp);
    DeleteDC(dc);
    ReleaseDC(null_mut(), screen);
    dst
}

unsafe fn gdi_round(dc: HDC, r: RECT, rad: i32, fill: Option<u32>, pen: Option<(u32, i32)>) {
    let br = fill.map(|c| CreateSolidBrush(c));
    let pn = pen.map(|(c, w)| CreatePen(PS_SOLID, w, c));
    let ob = SelectObject(dc, br.unwrap_or(GetStockObject(NULL_BRUSH)));
    let op = SelectObject(dc, pn.unwrap_or(GetStockObject(NULL_PEN)));
    let adj = if pen.is_some() { 0 } else { 1 };
    RoundRect(dc, r.left, r.top, r.right + adj, r.bottom + adj, rad, rad);
    SelectObject(dc, ob);
    SelectObject(dc, op);
    if let Some(b) = br {
        DeleteObject(b);
    }
    if let Some(p) = pn {
        DeleteObject(p);
    }
}

unsafe fn gdi_text(dc: HDC, text: &str, rc: RECT, face: &str, height: i32, width: i32, weight: i32, color: u32) {
    let mut lf: LOGFONTW = std::mem::zeroed();
    lf.lfHeight = -height;
    lf.lfWidth = width;
    lf.lfWeight = weight;
    lf.lfQuality = 3; // sin suavizado GDI: el suavizado lo da la reducción 4x
    for (i, c) in face.encode_utf16().take(31).enumerate() {
        lf.lfFaceName[i] = c;
    }
    let font = CreateFontIndirectW(&lf);
    let of = SelectObject(dc, font);
    SetTextColor(dc, color);
    let w: Vec<u16> = text.encode_utf16().collect();
    let mut rc = rc;
    DrawTextW(dc, w.as_ptr(), w.len() as i32, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
    SelectObject(dc, of);
    DeleteObject(font);
}

/// Icono de batería para la barra de tareas: arriba el dibujo del tipo de dispositivo y
/// debajo una pila con el nivel relleno y el porcentaje dentro.
/// `stale` = último valor conocido (dispositivo dormido): se dibuja apagado.
/// ¿La barra de tareas usa el tema claro? (los iconos se dibujan entonces en oscuro)
fn taskbar_light() -> bool {
    unsafe {
        let mut key: HKEY = null_mut();
        let path = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
        if RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, KEY_QUERY_VALUE, &mut key) != 0 {
            return false;
        }
        let mut v: u32 = 0;
        let mut size = 4u32;
        let ok = RegQueryValueExW(key, wide("SystemUsesLightTheme").as_ptr(), null(), null_mut(), &mut v as *mut u32 as *mut u8, &mut size) == 0;
        RegCloseKey(key);
        ok && v == 1
    }
}

unsafe fn make_battery_icon(size: i32, pct: u8, charging: bool, stale: bool, glyph: &str) -> HICON {
    pixels_to_icon(size, &battery_pixels(size, pct, charging, stale, glyph))
}

unsafe fn battery_pixels(size: i32, pct: u8, charging: bool, stale: bool, glyph: &str) -> Vec<u32> {
    let light = taskbar_light();
    render_pixels(size, |dc, s| {
        let u = |f: f32| (f * s as f32).round() as i32;
        let white = match (light, stale) {
            (false, false) => rgb(0xFF, 0xFF, 0xFF),
            (false, true) => rgb(0xA8, 0xA8, 0xB4),
            (true, false) => rgb(0x1A, 0x1A, 0x1A),
            (true, true) => rgb(0x70, 0x70, 0x78),
        };
        // Dibujo del dispositivo (mitad superior)
        gdi_text(dc, glyph, RECT { left: 0, top: 0, right: s, bottom: u(0.50) }, "Segoe Fluent Icons", u(0.46), 0, 400, white);
        // Pila (mitad inferior)
        let body = RECT { left: u(0.02), top: u(0.52), right: s - u(0.10), bottom: s - u(0.02) };
        let outline = if charging { rgb(0xFF, 0xD3, 0x3D) } else { white };
        gdi_round(dc, RECT { left: body.right - u(0.02), top: u(0.66), right: s, bottom: s - u(0.16) }, u(0.06), Some(outline), None);
        gdi_round(dc, body, u(0.14), Some(outline), None);
        let line = u(0.05).max(2);
        let inner = RECT { left: body.left + line, top: body.top + line, right: body.right - line, bottom: body.bottom - line };
        gdi_round(dc, inner, u(0.10), Some(rgb(0x1C, 0x1C, 0x24)), None);
        let level = if stale { rgb(0x55, 0x55, 0x60) } else if pct > 50 { rgb(0x23, 0xA8, 0x5E) } else if pct > 20 { rgb(0xD4, 0x88, 0x0C) } else { rgb(0xD9, 0x3A, 0x3F) };
        let fw = ((inner.right - inner.left) as f32 * pct.min(100) as f32 / 100.0).round() as i32;
        if fw > 0 {
            gdi_round(dc, RECT { right: inner.left + fw.max(line), ..inner }, u(0.08), Some(level), None);
        }
        // Porcentaje
        let text = pct.min(100).to_string();
        let h = (inner.bottom - inner.top) as f32;
        let wdt = if text.len() >= 3 { ((inner.right - inner.left) as f32 / 3.4) as i32 } else { 0 };
        gdi_text(dc, &text, inner, "Segoe UI", (h * 1.05) as i32, wdt, 800, rgb(0xFF, 0xFF, 0xFF));
    })
}

// ------------------------------------------------------------------ widget en pantallas secundarias

/// Elementos de batería que se muestran (tooltip, estado, dibujo del dispositivo).
fn battery_items() -> Vec<(String, (u8, bool, bool), &'static str)> {
    let a = app();
    a.devices
        .iter()
        .filter_map(|d| {
            let st = match d.battery {
                Some(b) => (b.pct, b.charging, false),
                None => (*a.last_battery.get(&d.key)?, false, true),
            };
            Some((format!("{} · {}", d.name, battery_text(d)), st, glyph(d)))
        })
        .collect()
}

/// Barras de tareas de las pantallas secundarias (Windows sólo pone la bandeja en la principal).
fn secondary_taskbars() -> Vec<HWND> {
    let mut out = Vec::new();
    let class = wide("Shell_SecondaryTrayWnd");
    let mut h: HWND = null_mut();
    loop {
        h = unsafe { FindWindowExW(null_mut(), h, class.as_ptr(), null()) };
        if h.is_null() {
            break;
        }
        out.push(h);
    }
    out
}

/// ¿Hay una ventana a pantalla completa (juego, vídeo, navegador con F11…) en la pantalla de esa
/// barra? Se mira la geometría de las ventanas, no el aviso de Windows (que sólo vale para la
/// pantalla principal), y da igual que la ventana tenga el foco o no.
fn fullscreen_on(taskbar: HWND) -> bool {
    unsafe extern "system" fn each(h: HWND, lp: LPARAM) -> BOOL {
        let st = &mut *(lp as *mut (RECT, bool, u32));
        if IsWindowVisible(h) == 0 || IsIconic(h) != 0 {
            return 1;
        }
        let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
        if ex & (WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE) != 0 {
            return 1; // superposiciones (Discord, NVIDIA…) y el propio widget
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        if pid == st.2 {
            return 1;
        }
        let mut cloaked: u32 = 0;
        windows_sys::Win32::Graphics::Dwm::DwmGetWindowAttribute(h, 14 /* DWMWA_CLOAKED */, &mut cloaked as *mut u32 as _, 4);
        if cloaked != 0 {
            return 1;
        }
        let mut cls = [0u16; 32];
        let n = GetClassNameW(h, cls.as_mut_ptr(), 32) as usize;
        let c = String::from_utf16_lossy(&cls[..n]);
        if c == "Progman" || c == "WorkerW" || c.starts_with("Shell_") {
            return 1;
        }
        let mut r: RECT = std::mem::zeroed();
        GetWindowRect(h, &mut r);
        let m = st.0;
        if r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom {
            st.1 = true;
            return 0;
        }
        1
    }
    unsafe {
        let mon = MonitorFromWindow(taskbar, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(mon, &mut mi);
        let mut st = (mi.rcMonitor, false, std::process::id());
        EnumWindows(Some(each), &mut st as *mut _ as LPARAM);
        st.1
    }
}

/// Crea, coloca y redibuja los widgets. Barato: sólo redibuja si algo cambió.
/// Trabaja con la escala propia de cada pantalla (per-monitor), para que funcione aunque las
/// pantallas tengan escalas distintas (p. ej. 125 % y 100 %).
fn update_widgets() {
    unsafe {
        let old = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        update_widgets_inner();
        SetThreadDpiAwarenessContext(old);
    }
}

fn update_widgets_inner() {
    let a = app();
    let items = battery_items();
    let bars = if a.config.hide_widget || items.is_empty() { Vec::new() } else { secondary_taskbars() };
    // Quitar los de barras que ya no existen
    a.widgets.retain(|w| {
        let keep = bars.contains(&w.taskbar);
        if !keep {
            unsafe {
                DestroyWindow(w.hwnd);
            }
        }
        keep
    });
    for tb in bars {
        if !a.widgets.iter().any(|w| w.taskbar == tb) {
            unsafe {
                let hwnd = CreateWindowExW(
                    WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_LAYERED,
                    wide("CentroLEDWidget").as_ptr(),
                    wide("Centro LED - batería").as_ptr(),
                    WS_POPUP,
                    0,
                    0,
                    1,
                    1,
                    null_mut(),
                    null_mut(),
                    GetModuleHandleW(null()),
                    null(),
                );
                let tip = CreateWindowExW(WS_EX_TOPMOST, wide("tooltips_class32").as_ptr(), null(), WS_POPUP | 0x01 /* TTS_ALWAYSTIP */, 0, 0, 0, 0, hwnd, null_mut(), GetModuleHandleW(null()), null());
                a.widgets.push(Widget { hwnd, taskbar: tb, tip, sig: String::new(), dpi: 96 });
            }
        }
    }
    for w in a.widgets.iter_mut() {
        unsafe {
            // Escala de la pantalla de esa barra
            let dpi = GetDpiForWindow(w.taskbar).max(96);
            let sc = |v: i32| v * dpi as i32 / 96;
            // Icono a media altura de la barra (más legible que el tamaño de la bandeja)
            let mut tr0: RECT = std::mem::zeroed();
            GetWindowRect(w.taskbar, &mut tr0);
            let icon = ((tr0.bottom - tr0.top) / 2).clamp(GetSystemMetricsForDpi(SM_CXSMICON, dpi).max(16), sc(32));
            let item = icon + sc(14);
            w.dpi = dpi;
            let mut tr: RECT = std::mem::zeroed();
            GetWindowRect(w.taskbar, &mut tr);
            let h = tr.bottom - tr.top;
            if IsWindowVisible(w.taskbar) == 0 || h < 16 || fullscreen_on(w.taskbar) {
                ShowWindow(w.hwnd, SW_HIDE);
                w.sig.clear();
                continue;
            }
            let width = items.len() as i32 * item + sc(8);
            let x = tr.right - sc(a.config.widget_offset) - width;
            let sig = format!("{:?}{}{}{}{}", items, tr.left, tr.top, tr.right, a.config.widget_offset);
            if sig != w.sig {
                render_widget(w, &items, x, tr.top, width, h, icon, item, sc(4), sc(5));
                w.sig = sig;
            }
            // Mantenerlo por encima de la barra de tareas
            SetWindowPos(w.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
    }
}

unsafe fn render_widget(w: &Widget, items: &[(String, (u8, bool, bool), &'static str)], x: i32, y: i32, width: i32, h: i32, icon: i32, item: i32, pad: i32, tip_pad: i32) {
    let screen = GetDC(null_mut());
    let dc = CreateCompatibleDC(screen);
    let mut bmi: BITMAPINFO = std::mem::zeroed();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = width;
    bmi.bmiHeader.biHeight = -h;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    let mut bits: *mut core::ffi::c_void = null_mut();
    let bmp = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
    let old = SelectObject(dc, bmp);
    let dst = std::slice::from_raw_parts_mut(bits as *mut u32, (width * h) as usize);
    // Fondo casi transparente para que toda la zona responda al ratón
    dst.fill(0x0100_0000);
    let top = (h - icon) / 2;
    // Quitar herramientas anteriores del tooltip
    SendMessageW(w.tip, WM_USER + 50 + 1, 0, 0);
    DestroyWindow(w.tip);
    let tip = CreateWindowExW(WS_EX_TOPMOST, wide("tooltips_class32").as_ptr(), null(), WS_POPUP | 0x01, 0, 0, 0, 0, w.hwnd, null_mut(), GetModuleHandleW(null()), null());
    for (k, (text, st, g)) in items.iter().enumerate() {
        let px = battery_pixels(icon, st.0, st.1, st.2, g);
        let left = pad + k as i32 * item + (item - icon) / 2;
        for yy in 0..icon {
            for xx in 0..icon {
                let v = px[(yy * icon + xx) as usize];
                let al = v >> 24;
                if al == 0 {
                    continue;
                }
                // Premultiplicar para UpdateLayeredWindow
                let pm = |c: u32| ((c & 0xFF) * al / 255) & 0xFF;
                let out = (al << 24) | (pm(v >> 16) << 16) | (pm(v >> 8) << 8) | pm(v);
                dst[((top + yy) * width + left + xx) as usize] = out;
            }
        }
        // Tooltip con el nombre y la batería
        let t = wide(text);
        let mut ti: TTTOOLINFOW = std::mem::zeroed();
        ti.cbSize = std::mem::size_of::<TTTOOLINFOW>() as u32;
        ti.uFlags = 0x0010; // TTF_SUBCLASS
        ti.hwnd = w.hwnd;
        ti.uId = k;
        ti.rect = RECT { left: left - tip_pad, top: 0, right: left + icon + tip_pad, bottom: h };
        ti.lpszText = t.as_ptr() as *mut u16;
        SendMessageW(tip, WM_USER + 50, 0, &ti as *const _ as LPARAM); // TTM_ADDTOOLW
    }
    let wp = &mut *(app().widgets.iter_mut().find(|x| x.hwnd == w.hwnd).unwrap() as *mut Widget);
    wp.tip = tip;
    let pos = POINT { x, y };
    let size = SIZE { cx: width, cy: h };
    let src = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: 1 };
    UpdateLayeredWindow(w.hwnd, screen, &pos, &size, dc, &src, 0, &blend, ULW_ALPHA);
    SelectObject(dc, old);
    DeleteObject(bmp);
    DeleteDC(dc);
    ReleaseDC(null_mut(), screen);
}

/// Ventana del widget: clic abre Centro LED, clic derecho el menú, arrastrar lo mueve.
unsafe extern "system" fn widget_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if APP.is_null() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let a = app();
    match msg {
        WM_LBUTTONDOWN => {
            let mut pt = POINT { x: 0, y: 0 };
            GetCursorPos(&mut pt);
            a.widget_drag = Some((pt.x, a.config.widget_offset, false));
            SetCapture(hwnd);
            0
        }
        WM_MOUSEMOVE => {
            if let Some((x0, off0, moved)) = a.widget_drag {
                let mut pt = POINT { x: 0, y: 0 };
                GetCursorPos(&mut pt);
                let dx = pt.x - x0;
                if moved || dx.abs() > 4 {
                    a.widget_drag = Some((x0, off0, true));
                    let dpi = a.widgets.iter().find(|w| w.hwnd == hwnd).map(|w| w.dpi).unwrap_or(96);
                    a.config.widget_offset = (off0 - dx * 96 / dpi as i32).clamp(0, 1600);
                    update_widgets();
                }
            }
            0
        }
        WM_LBUTTONUP => {
            ReleaseCapture();
            if let Some((_, _, moved)) = a.widget_drag.take() {
                if moved {
                    save_config(&a.config);
                } else {
                    show_main();
                }
            }
            0
        }
        WM_RBUTTONUP => {
            tray_menu();
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Sincroniza los iconos de batería de la barra de tareas con los dispositivos actuales.
fn update_battery_icons() {
    let a = app();
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
    let mut wanted: Vec<(String, String, (u8, bool, bool), &'static str)> = Vec::new();
    if !a.config.hide_tray_battery {
        for d in &a.devices {
            let state = match d.battery {
                Some(b) => (b.pct, b.charging, false),
                None => match a.last_battery.get(&d.key) {
                    Some(&pct) => (pct, false, true),
                    None => continue,
                },
            };
            wanted.push((d.key.clone(), format!("{}\n{}", d.name, battery_text(d)), state, glyph(d)));
        }
    }
    // Quitar los que sobran
    let gone: Vec<String> = a.tray_batt.keys().filter(|k| !wanted.iter().any(|w| &w.0 == *k)).cloned().collect();
    for k in gone {
        if let Some(t) = a.tray_batt.remove(&k) {
            tray_delete(t.id);
            unsafe { DestroyIcon(t.icon) };
        }
    }
    // Añadir o actualizar
    for (key, tip, state, g) in wanted {
        let existing = a.tray_batt.get(&key).map(|t| (t.id, t.state));
        let (id, add) = match existing {
            Some((id, st)) if st == state => {
                tray_set(id, None, &tip, false);
                continue;
            }
            Some((id, _)) => (id, false),
            None => {
                a.next_tray_id += 1;
                (a.next_tray_id, true)
            }
        };
        let icon = unsafe { make_battery_icon(size, state.0, state.1, state.2, g) };
        tray_set(id, Some(icon), &tip, add);
        if let Some(old) = a.tray_batt.insert(key, TrayBatt { id, state, icon }) {
            unsafe { DestroyIcon(old.icon) };
        }
    }
}

/// Windows 11 esconde los iconos nuevos tras la flecha "^". Marca como visibles en la barra
/// de tareas los iconos de batería de esta app que Windows aún no ha clasificado (si el
/// usuario ya eligió ocultar alguno, se respeta).
fn promote_battery_icons() {
    let exe = match std::env::current_exe() {
        Ok(p) => p.display().to_string().to_lowercase(),
        Err(_) => return,
    };
    let names: Vec<String> = app().devices.iter().map(|d| d.name.clone()).collect();
    unsafe {
        let root = wide("Control Panel\\NotifyIconSettings");
        let mut key: HKEY = null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, root.as_ptr(), 0, KEY_READ, &mut key) != 0 {
            return;
        }
        let mut i = 0;
        loop {
            let mut name = [0u16; 64];
            let mut len = name.len() as u32;
            if RegEnumKeyExW(key, i, name.as_mut_ptr(), &mut len, null(), null_mut(), null_mut(), null_mut()) != 0 {
                break;
            }
            i += 1;
            let mut sub: HKEY = null_mut();
            if RegOpenKeyExW(key, name.as_ptr(), 0, KEY_QUERY_VALUE | KEY_SET_VALUE, &mut sub) != 0 {
                continue;
            }
            let read = |value: &str| -> Option<String> {
                let mut buf = [0u16; 1024];
                let mut size = (buf.len() * 2) as u32;
                (RegQueryValueExW(sub, wide(value).as_ptr(), null(), null_mut(), buf.as_mut_ptr() as *mut u8, &mut size) == 0)
                    .then(|| String::from_utf16_lossy(&buf[..size as usize / 2]).trim_end_matches('\0').to_string())
            };
            let path = read("ExecutablePath").unwrap_or_default().to_lowercase();
            let tip = read("InitialTooltip").unwrap_or_default();
            let decided = RegQueryValueExW(sub, wide("IsPromoted").as_ptr(), null(), null_mut(), null_mut(), null_mut()) == 0;
            if path == exe && !decided && names.iter().any(|n| tip.starts_with(n.as_str())) {
                let one: u32 = 1;
                RegSetValueExW(sub, wide("IsPromoted").as_ptr(), 0, REG_DWORD, &one as *const u32 as *const u8, 4);
            }
            RegCloseKey(sub);
        }
        RegCloseKey(key);
    }
}

fn tray_set(id: u32, icon: Option<HICON>, tip: &str, add: bool) {
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = app().hwnd;
        nid.uID = id;
        nid.uFlags = NIF_MESSAGE | NIF_TIP | if icon.is_some() { NIF_ICON } else { 0 };
        nid.uCallbackMessage = WM_TRAY;
        if let Some(i) = icon {
            nid.hIcon = i;
        }
        let w: Vec<u16> = tip.encode_utf16().take(nid.szTip.len() - 1).collect();
        nid.szTip[..w.len()].copy_from_slice(&w);
        if !(add && Shell_NotifyIconW(NIM_ADD, &nid) != 0) {
            Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }
}

fn tray_delete(id: u32) {
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = app().hwnd;
        nid.uID = id;
        Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

/// Icono de la app dibujado al vuelo: cuadrado redondeado violeta con una bombilla.
unsafe fn make_icon(size: i32) -> HICON {
    let screen = GetDC(null_mut());
    let dc = CreateCompatibleDC(screen);
    let mut bmi: BITMAPINFO = std::mem::zeroed();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = size;
    bmi.bmiHeader.biHeight = -size;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    let mut bits: *mut core::ffi::c_void = null_mut();
    let color = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
    let old = SelectObject(dc, color);
    const KEY: u32 = rgb(0xFF, 0x00, 0xFF);
    let all = RECT { left: 0, top: 0, right: size, bottom: size };
    let kb = CreateSolidBrush(KEY);
    FillRect(dc, &all, kb);
    DeleteObject(kb);
    let br = CreateSolidBrush(th().accent);
    let ob = SelectObject(dc, br);
    let op = SelectObject(dc, GetStockObject(NULL_PEN));
    RoundRect(dc, 0, 0, size + 1, size + 1, size / 2, size / 2);
    SelectObject(dc, ob);
    SelectObject(dc, op);
    DeleteObject(br);
    let mut lf: LOGFONTW = std::mem::zeroed();
    lf.lfHeight = -(size * 7 / 10);
    lf.lfQuality = 5;
    for (i, c) in "Segoe Fluent Icons".encode_utf16().enumerate() {
        lf.lfFaceName[i] = c;
    }
    let font = CreateFontIndirectW(&lf);
    let of = SelectObject(dc, font);
    SetBkMode(dc, TRANSPARENT as i32);
    SetTextColor(dc, rgb(0xFF, 0xFF, 0xFF));
    let g: Vec<u16> = "\u{E781}".encode_utf16().collect();
    let mut rc = all;
    DrawTextW(dc, g.as_ptr(), g.len() as i32, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    SelectObject(dc, of);
    DeleteObject(font);
    GdiFlush();
    // Canal alfa: transparente donde quedó el color clave.
    let px = std::slice::from_raw_parts_mut(bits as *mut u32, (size * size) as usize);
    for v in px.iter_mut() {
        let bgr = *v & 0x00FF_FFFF;
        *v = if bgr == 0x00FF_00FF { 0 } else { bgr | 0xFF00_0000 };
    }
    SelectObject(dc, old);
    let mask = CreateBitmap(size, size, 1, 1, null());
    let info = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
    let icon = CreateIconIndirect(&info);
    DeleteObject(mask);
    DeleteObject(color);
    DeleteDC(dc);
    ReleaseDC(null_mut(), screen);
    icon
}

fn update_tray(add_icon: bool) -> bool {
    let a = app();
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = a.hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = a.icon;
        let mut tip = String::from("Centro LED");
        for d in a.devices.iter().filter(|d| d.battery.is_some()) {
            let short: String = d.name.chars().take(18).collect();
            tip += &format!("\n{}: {}", short, battery_text(d));
        }
        let w: Vec<u16> = tip.encode_utf16().take(nid.szTip.len() - 1).collect();
        nid.szTip[..w.len()].copy_from_slice(&w);
        Shell_NotifyIconW(if add_icon { NIM_ADD } else { NIM_MODIFY }, &nid) != 0
    }
}

/// Notificación de Windows desde el icono de la bandeja.
fn notify(title: &str, text: &str) {
    let a = app();
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = a.hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_INFO;
        nid.dwInfoFlags = NIIF_USER | NIIF_LARGE_ICON;
        nid.hBalloonIcon = a.icon;
        let t: Vec<u16> = title.encode_utf16().take(nid.szInfoTitle.len() - 1).collect();
        nid.szInfoTitle[..t.len()].copy_from_slice(&t);
        let m: Vec<u16> = text.encode_utf16().take(nid.szInfo.len() - 1).collect();
        nid.szInfo[..m.len()].copy_from_slice(&m);
        Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

fn remove_tray() {
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = app().hwnd;
        nid.uID = 1;
        Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

fn show_main() {
    let h = app().hwnd;
    unsafe {
        ShowWindow(h, SW_SHOW);
        ShowWindow(h, SW_RESTORE);
        SetForegroundWindow(h);
    }
}

/// Oculta la ventana y devuelve la memoria al sistema: en segundo plano sólo queda
/// el icono de la bandeja y un temporizador por minuto para la batería.
fn hide_to_tray() {
    unsafe {
        ShowWindow(app().hwnd, SW_HIDE);
        KillTimer(app().hwnd, TIMER_ANIM);
        SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

fn tray_menu() {
    let h = app().hwnd;
    unsafe {
        let a = app();
        let menu = CreatePopupMenu();
        AppendMenuW(menu, MF_STRING, MENU_OPEN, wide("Abrir Centro LED").as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        if !a.config.profiles.is_empty() {
            let sub = CreatePopupMenu();
            for (i, p) in a.config.profiles.iter().enumerate() {
                let checked = if p.name == a.config.active_profile { MF_CHECKED } else { 0 };
                AppendMenuW(sub, MF_STRING | checked, 200 + i, wide(&p.name).as_ptr());
            }
            AppendMenuW(menu, MF_STRING | MF_POPUP, sub as usize, wide("Perfiles").as_ptr());
        }
        let toggle = if state::all_lights_off() { "Encender todas las luces" } else { "Apagar todas las luces" };
        AppendMenuW(menu, MF_STRING, MENU_TOGGLE, wide(toggle).as_ptr());
        AppendMenuW(menu, MF_STRING, MENU_SCAN, wide("Buscar dispositivos").as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, MF_STRING, MENU_EXIT, wide("Salir").as_ptr());
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        SetForegroundWindow(h);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, h, null()) as usize;
        DestroyMenu(menu);
        match cmd {
            MENU_OPEN => show_main(),
            MENU_SCAN => start_scan(),
            MENU_TOGGLE => state::toggle_all(),
            200..=299 => state::apply_profile(cmd - 200),
            MENU_EXIT => {
                DestroyWindow(h);
            }
            _ => {}
        }
    }
}

// ------------------------------------------------------------------ ventana

fn point(lp: LPARAM) -> (i32, i32) {
    ((lp & 0xFFFF) as i16 as i32, ((lp >> 16) & 0xFFFF) as i16 as i32)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if APP.is_null() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let a = app();
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rc: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rc);
            let pr = ps.rcPaint;
            let ar = a.anim_rect;
            let inside = pr.left >= ar.left && pr.top >= ar.top && pr.right <= ar.right && pr.bottom <= ar.bottom;
            if let (true, Some((kb, px, py, pw)), Some(d)) = (inside, a.preview, selected()) {
                // Sólo la vista previa, en un búfer pequeño desplazado a su posición.
                let (w, h) = (pr.right - pr.left, pr.bottom - pr.top);
                let mem = CreateCompatibleDC(hdc);
                let bmp = CreateCompatibleBitmap(hdc, w, h);
                let old = SelectObject(mem, bmp);
                SetBkMode(mem, TRANSPARENT as i32);
                SetViewportOrgEx(mem, -pr.left, -pr.top, null_mut());
                let painter = Painter { dc: mem };
                painter.fill(&pr, th().bg, 0);
                a.partial = true;
                if kb {
                    paint_keyboard(&painter, px, py, pw);
                } else {
                    paint_device_hero(&painter, &d, px, py, pw);
                }
                a.partial = false;
                SetViewportOrgEx(mem, 0, 0, null_mut());
                BitBlt(hdc, pr.left, pr.top, w, h, mem, 0, 0, SRCCOPY);
                SelectObject(mem, old);
                DeleteObject(bmp);
                DeleteDC(mem);
                EndPaint(hwnd, &ps);
                return 0;
            }
            // Doble búfer temporal: se crea y se libera en cada pintado.
            let mem = CreateCompatibleDC(hdc);
            let bmp = CreateCompatibleBitmap(hdc, rc.right, rc.bottom);
            let old = SelectObject(mem, bmp);
            SetBkMode(mem, TRANSPARENT as i32);
            // Recortar al área que hay que repintar: GDI descarta todo lo de fuera.
            IntersectClipRect(mem, pr.left, pr.top, pr.right, pr.bottom);
            paint(&Painter { dc: mem });
            if preview_animates() {
                SetTimer(hwnd, TIMER_ANIM, 50, None);
            } else {
                KillTimer(hwnd, TIMER_ANIM);
            }
            BitBlt(hdc, pr.left, pr.top, pr.right - pr.left, pr.bottom - pr.top, mem, pr.left, pr.top, SRCCOPY);
            SelectObject(mem, old);
            DeleteObject(bmp);
            DeleteDC(mem);
            EndPaint(hwnd, &ps);
            0
        }
        WM_ERASEBKGND => 1,
        0x020A => {
            // WM_MOUSEWHEEL: desplazar la lista o el resumen
            let delta = ((wp >> 16) & 0xFFFF) as i16 as i32;
            let mut pt = POINT { x: (lp & 0xFFFF) as i16 as i32, y: ((lp >> 16) & 0xFFFF) as i16 as i32 };
            ScreenToClient(hwnd, &mut pt);
            let step = -delta * 66 / 120;
            if pt.x < s(SIDEBAR_W) {
                a.list_scroll = (a.list_scroll + step).clamp(0, a.list_max);
            } else if a.page == 0 {
                a.home_scroll = (a.home_scroll + step).clamp(0, a.home_max);
            } else if a.page == 2 {
                a.set_scroll = (a.set_scroll + step).clamp(0, a.set_max);
            }
            redraw();
            0
        }
        WM_LBUTTONDOWN => {
            let (x, y) = point(lp);
            if let Some(act) = hit_test(x, y) {
                on_click(act, x);
            }
            0
        }
        WM_RBUTTONDOWN => {
            // Cuentagotas: copiar el color de una tecla. En un perfil: su menú.
            let (x, y) = point(lp);
            match hit_test(x, y) {
                Some(Act::Key(i)) => {
                    let c = a.keys[i];
                    a.cur.rgb = (c[0], c[1], c[2]);
                    a.cur.multi = false;
                    redraw();
                }
                Some(Act::Profile(i)) => state::profile_menu(i),
                _ => {}
            }
            0
        }
        WM_MOUSEMOVE => {
            let (x, y) = point(lp);
            if !a.tracking {
                let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                TrackMouseEvent(&mut tme);
                a.tracking = true;
            }
            match a.drag {
                Some(Act::Slider(id)) => slider_set(id, x),
                Some(Act::Key(_)) => {
                    if let Some(Act::Key(i)) = hit_test(x, y) {
                        paint_key(i);
                    }
                }
                _ => {}
            }
            let hot = hit_test(x, y);
            if hot != a.hot {
                a.hot = hot;
                SetCursor(LoadCursorW(null_mut(), if hot.is_some() { IDC_HAND } else { IDC_ARROW }));
                redraw();
            }
            0
        }
        WM_SETCURSOR => {
            if (lp & 0xFFFF) as u32 == HTCLIENT {
                SetCursor(LoadCursorW(null_mut(), if a.hot.is_some() { IDC_HAND } else { IDC_ARROW }));
                return 1;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        0x02A3 => {
            // WM_MOUSELEAVE
            a.tracking = false;
            if a.hot.is_some() {
                a.hot = None;
                redraw();
            }
            0
        }
        WM_LBUTTONUP => {
            if a.drag.take().is_some() {
                ReleaseCapture();
            }
            0
        }
        WM_TIMER => {
            if wp == TIMER_ANIM {
                if IsWindowVisible(hwnd) != 0 && IsIconic(hwnd) == 0 {
                    InvalidateRect(hwnd, &a.anim_rect, 0);
                } else {
                    KillTimer(hwnd, TIMER_ANIM);
                }
                return 0;
            }
            if wp == TIMER_WIDGET {
                update_widgets();
                return 0;
            }
            if wp == TIMER_RESUME {
                KillTimer(hwnd, TIMER_RESUME);
                state::auto_set(state::AUTO_SLEEP, false, false);
                return 0;
            }
            if wp == TIMER_TRAY {
                if update_tray(true) {
                    KillTimer(hwnd, TIMER_TRAY);
                    update_battery_icons();
                }
                return 0;
            }
            if wp == TIMER_UPDATE {
                // Primera comprobación a los pocos segundos y luego cada 6 horas.
                SetTimer(hwnd, TIMER_UPDATE, 6 * 3600 * 1000, None);
                if !matches!(a.upd, Upd::Available(_)) {
                    start_update_check();
                }
                return 0;
            }
            if wp == TIMER_PROMOTE {
                KillTimer(hwnd, TIMER_PROMOTE);
                if !a.config.hide_tray_battery {
                    promote_battery_icons();
                }
                return 0;
            }
            if wp == TIMER_DEVCHANGE || wp == TIMER_RETRY {
                KillTimer(hwnd, wp);
            }
            start_scan();
            0
        }
        WM_DEVICECHANGE => {
            // Esperar a que Windows termine de instalar el dispositivo antes de buscar.
            SetTimer(hwnd, TIMER_DEVCHANGE, 2000, None);
            1
        }
        WM_SCANNED => {
            let list = Box::from_raw(lp as *mut Vec<Device>);
            on_scanned(*list);
            0
        }
        WM_QUIT_APP => {
            DestroyWindow(hwnd);
            0
        }
        0x02E0 => {
            // WM_DPICHANGED: la ventana pasó a una pantalla con otra escala.
            let dpi = (wp & 0xFFFF) as u32;
            set_scale(dpi);
            let r = &*(lp as *const RECT);
            let mut rc = RECT { left: 0, top: 0, right: s(WIN_W), bottom: s(WIN_H) };
            AdjustWindowRectExForDpi(&mut rc, WIN_STYLE, 0, 0, dpi);
            SetWindowPos(hwnd, null_mut(), r.left, r.top, rc.right - rc.left, rc.bottom - rc.top, SWP_NOZORDER | SWP_NOACTIVATE);
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_HOTKEY => {
            state::on_hotkey(wp as i32);
            0
        }
        WM_SETTINGCHANGE | 0x0320 => {
            // Cambió el modo claro/oscuro o el color de Windows (0x0320 = WM_DWMCOLORIZATIONCOLORCHANGED)
            if a.config.theme == THEME_SYSTEM || a.config.accent == ACCENT_WINDOWS {
                apply_theme();
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN if a.recording.is_some() => {
            state::record_key(wp as u32);
            redraw();
            0
        }
        0x02B1 => {
            // WM_WTSSESSION_CHANGE: 7 = equipo bloqueado, 8 = desbloqueado
            match wp {
                7 if a.config.auto[0] => state::auto_set(state::AUTO_LOCK, true, false),
                8 => state::auto_set(state::AUTO_LOCK, false, false),
                _ => {}
            }
            0
        }
        WM_POWERBROADCAST => {
            match wp as u32 {
                0x8013 => {
                    // Pantalla: 0 apagada, 1 encendida, 2 atenuada
                    let ps = &*(lp as *const POWERBROADCAST_SETTING);
                    if ps.PowerSetting.data1 == GUID_CONSOLE_DISPLAY_STATE.data1 {
                        match ps.Data[0] {
                            0 if a.config.auto[1] => state::auto_set(state::AUTO_DISPLAY, true, false),
                            1 => state::auto_set(state::AUTO_DISPLAY, false, false),
                            _ => {}
                        }
                    }
                }
                4 if a.config.auto[2] => state::auto_set(state::AUTO_SLEEP, true, true), // a punto de suspender
                0x12 | 7 if a.auto_off & state::AUTO_SLEEP != 0 => {
                    SetTimer(hwnd, TIMER_RESUME, 4000, None);
                }
                _ => {}
            }
            1
        }
        WM_QUERYENDSESSION => 1,
        WM_ENDSESSION => {
            if wp != 0 && a.config.auto[3] {
                state::auto_lights_off(true);
                a.config.pending_restore = true;
                save_config(&a.config);
            }
            0
        }
        WM_UPD_INFO => {
            let r = *Box::from_raw(lp as *mut Result<Option<update::Release>, String>);
            a.upd = match r {
                Ok(Some(rel)) => {
                    let msg = format!("Hay una versión nueva de Centro LED: v{}", rel.version);
                    update_tray(false);
                    set_status(&msg);
                    Upd::Available(rel)
                }
                Ok(None) => Upd::UpToDate,
                Err(e) => Upd::Error(e),
            };
            redraw();
            0
        }
        WM_UPD_DONE => {
            let r = *Box::from_raw(lp as *mut Result<(std::path::PathBuf, String), String>);
            match r {
                Ok((exe, _ver)) => restart_into(&exe),
                Err(e) => {
                    a.upd = Upd::Error(e.clone());
                    set_status(&format!("No se pudo actualizar: {e}"));
                }
            }
            0
        }
        WM_APPLY_FAILED => {
            let key = *Box::from_raw(lp as *mut String);
            // El teclado no se reintenta solo (cada escritura desgasta su memoria).
            if !a.devices.iter().any(|d| d.key == key && is_keyboard(d)) {
                a.pending.insert(key);
                a.quick_scans = a.quick_scans.max(4);
                SetTimer(hwnd, TIMER_RETRY, 15_000, None);
            }
            0
        }
        WM_APPLIED => {
            let text = Box::from_raw(lp as *mut String);
            if wp == 0 {
                a.applying = false;
            }
            if !text.is_empty() {
                set_status(&text);
            }
            0
        }
        WM_TRAY => {
            match (lp as u32) & 0xFFFF {
                WM_LBUTTONUP => {
                    if IsWindowVisible(hwnd) != 0 {
                        hide_to_tray();
                    } else {
                        show_main();
                    }
                }
                WM_RBUTTONUP => tray_menu(),
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            // Cerrar la ventana deja la app en segundo plano, en la bandeja del sistema.
            hide_to_tray();
            0
        }
        WM_DESTROY => {
            remove_tray();
            for w in a.widgets.drain(..) {
                DestroyWindow(w.hwnd);
            }
            for t in a.tray_batt.values() {
                tray_delete(t.id);
            }
            PostQuitMessage(0);
            0
        }
        _ if msg == a.taskbar_created && msg != 0 => {
            // El explorador se reinició: volver a crear todos los iconos.
            update_tray(true);
            for (_, t) in a.tray_batt.drain() {
                DestroyIcon(t.icon);
            }
            update_battery_icons();
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

// ------------------------------------------------------------------ modo consola (pruebas / scripts)

fn cli(args: &[String]) -> bool {
    let out_file = |p: &str, text: String| {
        let _ = std::fs::write(p, text);
    };
    match args.get(1).map(|s| s.as_str()) {
        Some("--uninstall") => {
            unsafe { uninstall() };
            true
        }
        // CentroLED --version salida.txt
        Some("--version") => {
            let mode = if portable_dir().is_some() { " portable" } else { "" };
            out_file(args.get(2).map(|s| s.as_str()).unwrap_or("version.txt"), format!("{}{}", update::VERSION, mode));
            true
        }
        // CentroLED --check-update salida.txt
        Some("--check-update") => {
            let text = match update::check() {
                Ok(Some(r)) => format!("nueva {} {}", r.version, r.url),
                Ok(None) => format!("al día {}", update::VERSION),
                Err(e) => format!("error {e}"),
            };
            out_file(args.get(2).map(|s| s.as_str()).unwrap_or("update.txt"), text);
            true
        }
        // CentroLED --self-update salida.txt  (descarga e instala sin abrir la ventana)
        Some("--self-update") => {
            let text = match update::check() {
                Ok(Some(r)) => match update::install(&r) {
                    Ok(_) => format!("instalada {}", r.version),
                    Err(e) => format!("error {e}"),
                },
                Ok(None) => format!("al día {}", update::VERSION),
                Err(e) => format!("error {e}"),
            };
            out_file(args.get(2).map(|s| s.as_str()).unwrap_or("update.txt"), text);
            true
        }
        // CentroLED --verify <archivo> <nombre> <SHA256SUMS.txt> <firma> salida.txt  (comprueba una firma)
        Some("--verify") if args.len() >= 7 => {
            let rd = |p: &str| std::fs::read(p).unwrap_or_default();
            let r = update::verify(&rd(&args[2]), &args[3], &rd(&args[4]), &rd(&args[5]));
            out_file(&args[6], r.map(|_| "OK".to_string()).unwrap_or_else(|e| e));
            true
        }
        // CentroLED --test-dialog salida.bmp salida.txt  (prueba del cuadro de nombre)
        Some("--test-dialog") if args.len() >= 4 => {
            unsafe { state::test_dialog(&args[2], &args[3]) };
            true
        }
        // CentroLED --fast-start on|off salida.txt  (inicio rápido con una tarea programada)
        Some("--fast-start") if args.len() >= 4 => {
            let r = state::fast_start_set(args[2] == "on");
            out_file(&args[3], r.map(|_| "OK".to_string()).unwrap_or_else(|e| e));
            true
        }
        // CentroLED --probe salida.txt
        Some("--probe") => {
            let mut text = String::new();
            for d in devices::scan() {
                text += &format!("{}\t{}\t{}\t{}\t{}\tluces={}\n", d.key, d.name, d.kind, d.conn, battery_text(&d), d.led.is_some());
            }
            out_file(args.get(2).map(|s| s.as_str()).unwrap_or("probe.txt"), text);
            true
        }
        // CentroLED --set <clave> <modo|off> <rrggbb> <brillo> <velocidad> salida.txt
        Some("--set") if args.len() >= 8 => {
            let c = parse_rgb(&args[4]).unwrap_or([0xFF; 3]);
            let st = LedSetting {
                mode: if args[3] == "off" { MODE_OFF } else { args[3].parse().unwrap_or(1) },
                rgb: (c[0], c[1], c[2]),
                bright: args[5].parse().unwrap_or(100),
                speed: args[6].parse().unwrap_or(50),
                multi: false,
            };
            let res = match devices::scan().into_iter().find(|d| d.key == args[2]) {
                Some(d) => devices::apply(&d, &st, None).map(|_| "OK".to_string()).unwrap_or_else(|e| e),
                None => "Dispositivo no encontrado".into(),
            };
            out_file(&args[7], res);
            true
        }
        _ => false,
    }
}

/// Dibuja en un mapa de bits y lo guarda como .bmp.
unsafe fn render_to_file(w: i32, h: i32, path: &str, draw: impl FnOnce(HDC)) {
    let screen = GetDC(null_mut());
    let dc = CreateCompatibleDC(screen);
    let mut bmi: BITMAPINFO = std::mem::zeroed();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = w;
    bmi.bmiHeader.biHeight = -h;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    let mut bits: *mut core::ffi::c_void = null_mut();
    let bmp = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
    let old = SelectObject(dc, bmp);
    SetBkMode(dc, TRANSPARENT as i32);
    draw(dc);
    GdiFlush();
    let px = std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize);
    let mut out = Vec::with_capacity(54 + px.len());
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(54 + px.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&(-h).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 24]);
    out.extend_from_slice(px);
    let _ = std::fs::write(path, out);
    SelectObject(dc, old);
    DeleteObject(bmp);
    DeleteDC(dc);
    ReleaseDC(null_mut(), screen);
}

/// --render <clave> <pestaña> <salida.bmp> [modo]   |   --render-icons <salida.bmp>
unsafe fn do_render(args: &[String]) {
    let a = app();
    if args[1] == "--render-icons" {
        let samples: [(u8, bool, bool, &str); 5] = [(100, true, false, "\u{E7FC}"), (64, false, false, "\u{E7F6}"), (15, false, false, "\u{E962}"), (45, false, true, "\u{E765}"), (8, false, false, "\u{E702}")];
        let sizes = [16, 20, 24, 32, 48];
        let cell = 60;
        let w = cell * samples.len() as i32;
        let h = cell * sizes.len() as i32 * 2;
        render_to_file(w, h, args.get(2).map(|s| s.as_str()).unwrap_or("icons.bmp"), |dc| {
            for (row, &sz) in sizes.iter().enumerate() {
                for (dark, bg) in [(0, rgb(0x20, 0x20, 0x20)), (1, rgb(0xF3, 0xF3, 0xF3))] {
                    let y = (row as i32 * 2 + dark) * cell;
                    let br = CreateSolidBrush(bg);
                    FillRect(dc, &RECT { left: 0, top: y, right: w, bottom: y + cell }, br);
                    DeleteObject(br);
                    for (i, &(pct, ch, st, g)) in samples.iter().enumerate() {
                        let icon = make_battery_icon(sz, pct, ch, st, g);
                        DrawIconEx(dc, i as i32 * cell + (cell - sz) / 2, y + (cell - sz) / 2, icon, sz, sz, 0, null_mut(), DI_NORMAL);
                        DestroyIcon(icon);
                    }
                }
            }
        });
        return;
    }
    let key = args.get(2).cloned().unwrap_or_default();
    // --tema <tema> <acento> <0|1 rectas>: probar un aspecto sin cambiar la configuración
    if let Some(i) = args.iter().position(|x| x == "--tema") {
        let n = |k: usize| args.get(i + k).and_then(|v| v.parse::<u8>().ok()).unwrap_or(0);
        a.config.theme = n(1);
        a.config.accent = n(2);
        a.config.square = n(3) == 1;
        THEME = make_theme(a.config.theme, a.config.accent, a.config.square);
    }
    a.devices = devices::scan();
    state::seed_profiles_mem(); // sólo en memoria: las capturas no tocan la configuración
    if let Some(h) = args.iter().position(|x| x == "--hist").and_then(|i| args.get(i + 1)) {
        history::demo(h); // historial inventado para revisar la gráfica
    }
    // "inicio" y "ajustes" dibujan esas páginas
    if key == "inicio" || key == "ajustes" {
        a.page = if key == "inicio" { 0 } else { 2 };
        a.set_scroll = args.get(3).and_then(|t| t.parse().ok()).unwrap_or(0);
        let out = args.get(4).cloned().unwrap_or_else(|| "render.bmp".into());
        render_to_file(s(WIN_W), s(WIN_H), &out, |dc| paint(&Painter { dc }));
        return;
    }
    a.page = 1;
    let Some(i) = a.devices.iter().position(|d| d.key == key) else { return };
    select_device(i);
    a.tab = args.get(3).and_then(|t| t.parse().ok()).unwrap_or(0);
    if let Some(m) = args.get(5).and_then(|m| m.parse().ok()) {
        editing().mode = m;
    }
    let out = args.get(4).cloned().unwrap_or_else(|| "render.bmp".into());
    render_to_file(s(WIN_W), s(WIN_H), &out, |dc| paint(&Painter { dc }));
}

/// Desinstala la versión instalada con el instalador (sólo si el programa está en su carpeta).
unsafe fn uninstall() {
    let title = wide("Centro LED");
    if MessageBoxW(null_mut(), wide("¿Quieres desinstalar Centro LED?").as_ptr(), title.as_ptr(), MB_YESNO | MB_ICONQUESTION) != IDYES {
        return;
    }
    // Cerrar la app si está abierta.
    let h = FindWindowW(wide("CentroLEDWindow").as_ptr(), null());
    if !h.is_null() {
        PostMessageW(h, WM_QUIT_APP, 0, 0);
        std::thread::sleep(std::time::Duration::from_millis(1500));
    }
    set_autostart(false);
    let _ = state::fast_start_set(false);
    RegDeleteTreeW(HKEY_CURRENT_USER, wide(UNINSTALL_KEY).as_ptr());
    let appdata = std::env::var_os("APPDATA").map(std::path::PathBuf::from).unwrap_or_default();
    let _ = std::fs::remove_file(appdata.join("Microsoft\\Windows\\Start Menu\\Programs\\Centro LED.lnk"));
    let mut desk: windows_sys::core::PWSTR = null_mut();
    if SHGetKnownFolderPath(&FOLDERID_Desktop, 0, null_mut(), &mut desk) == 0 {
        let mut n = 0;
        while *desk.add(n) != 0 {
            n += 1;
        }
        let d = String::from_utf16_lossy(std::slice::from_raw_parts(desk, n));
        let _ = std::fs::remove_file(std::path::Path::new(&d).join("Centro LED.lnk"));
        windows_sys::Win32::System::Com::CoTaskMemFree(desk as _);
    }
    if MessageBoxW(null_mut(), wide("¿Borrar también tu configuración de luces?").as_ptr(), title.as_ptr(), MB_YESNO | MB_ICONQUESTION) == IDYES {
        let _ = std::fs::remove_dir_all(appdata.join(APP_NAME));
    }
    // Borrar la carpeta del programa cuando este proceso termine (sólo la carpeta de instalación).
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
        if dir.to_string_lossy().to_lowercase().ends_with("programs\\centroled") {
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("cmd")
                .args(["/c", &format!("ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{}\"", dir.display())])
                .creation_flags(0x0800_0000)
                .spawn();
        }
    }
    MessageBoxW(null_mut(), wide("Centro LED se ha desinstalado.").as_ptr(), title.as_ptr(), MB_OK | MB_ICONINFORMATION);
}

unsafe fn font(face: &str, px: i32, weight: i32, scale: f32) -> HFONT {
    let mut lf: LOGFONTW = std::mem::zeroed();
    lf.lfHeight = -((px as f32 * scale).round() as i32);
    lf.lfWeight = weight;
    lf.lfQuality = 5; // ClearType
    lf.lfCharSet = 1; // DEFAULT_CHARSET
    for (i, c) in face.encode_utf16().take(31).enumerate() {
        lf.lfFaceName[i] = c;
    }
    CreateFontIndirectW(&lf)
}

unsafe fn make_fonts(scale: f32) -> Fonts {
    let ui = "Segoe UI Variable Text";
    Fonts {
        title: font("Segoe UI Variable Display", 28, 600, scale),
        h2: font("Segoe UI Variable Display", 18, 600, scale),
        body: font(ui, 14, 400, scale),
        bold: font(ui, 14, 600, scale),
        small: font(ui, 12, 400, scale),
        key: font(ui, 11, 600, scale),
        key_small: font(ui, 8, 600, scale),
        icon: font("Segoe Fluent Icons", 20, 400, scale),
        icon_big: font("Segoe Fluent Icons", 56, 400, scale),
        small_icon: font("Segoe Fluent Icons", 15, 400, scale),
    }
}

/// Cambia la escala de la interfaz (al pasar la ventana a otra pantalla) y rehace las fuentes.
fn set_scale(dpi: u32) {
    let a = app();
    let scale = dpi.max(96) as f32 / 96.0;
    if (scale - a.scale).abs() < 0.01 {
        return;
    }
    let old = std::mem::replace(&mut a.fonts, unsafe { make_fonts(scale) });
    a.scale = scale;
    for f in [old.title, old.h2, old.body, old.bold, old.small, old.key, old.key_small, old.icon, old.icon_big, old.small_icon] {
        unsafe { DeleteObject(f) };
    }
}

fn main() {
    // Si algo falla, dejar constancia en %APPDATA%\CentroLED\error.txt
    std::panic::set_hook(Box::new(|info| {
        let path = config_path().with_file_name("error.txt");
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::write(path, info.to_string());
    }));
    let args: Vec<String> = std::env::args().collect();
    if cli(&args) {
        return;
    }
    let start_hidden = args.iter().any(|a| a == "--tray");
    // Dibujar la interfaz o los iconos en una imagen, sin mostrar nada (para revisar el diseño).
    let render = matches!(args.get(1).map(|s| s.as_str()), Some("--render") | Some("--render-icons"));
    unsafe {
        let class = wide(if render { "CentroLEDRender" } else { "CentroLEDWindow" });
        let updated = args.iter().any(|a| a == "--updated");
        let mut mutex: HANDLE = null_mut();
        if !render {
            update::cleanup();
            mutex = CreateMutexW(null(), 0, wide("CentroLED_SingleInstance").as_ptr());
        // Tras actualizar, esperar a que la versión anterior termine de cerrarse.
        let mut tries = 0;
        while updated && GetLastError() == ERROR_ALREADY_EXISTS && tries < 50 {
            CloseHandle(mutex);
            std::thread::sleep(std::time::Duration::from_millis(200));
            mutex = CreateMutexW(null(), 0, wide("CentroLED_SingleInstance").as_ptr());
            tries += 1;
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // Ya está abierta. Si esta copia se lanzó al iniciar (--tray), no sacar la ventana.
            let h = FindWindowW(class.as_ptr(), null());
            if !h.is_null() && !start_hidden {
                ShowWindow(h, SW_SHOW);
                ShowWindow(h, SW_RESTORE);
                SetForegroundWindow(h);
            }
            return;
        }
        }
        // Cada pantalla con su escala: la ventana se ve nítida también en la secundaria.
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let scale = GetDpiForSystem() as f32 / 96.0;
        let fonts = make_fonts(scale);

        let hinst = GetModuleHandleW(null());
        let icon = make_icon(64);
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: icon,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class.as_ptr(),
        };
        RegisterClassW(&wc);
        let wclass = wide("CentroLEDWidget");
        let wcw = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(widget_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: null_mut(),
            hCursor: LoadCursorW(null_mut(), IDC_HAND),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: wclass.as_ptr(),
        };
        RegisterClassW(&wcw);

        let app_box = Box::new(App {
            hwnd: null_mut(),
            scale,
            fonts,
            icon,
            devices: Vec::new(),
            sel: None,
            tab: 0,
            cur: LedSetting { mode: 1, rgb: (0x69, 0, 0xFF), bright: 100, speed: 60, multi: false },
            rim: LedSetting { mode: 24, rgb: (0x69, 0, 0xFF), bright: 100, speed: 60, multi: false },
            keys: Box::new([[0x69, 0, 0xFF]; KB_LEDS]),
            cycle: Vec::new(),
            cyclers: HashMap::new(),
            zones: Vec::new(),
            pad: PadExtra::default(),
            dirty: false,
            config: load_config(),
            known: HashSet::new(),
            scanning: false,
            applying: false,
            status: String::new(),
            hits: Vec::new(),
            hot: None,
            drag: None,
            tracking: false,
            custom_colors: [0x00FF_FFFF; 16],
            taskbar_created: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
            autostart: autostart_enabled(),
            t0: std::time::Instant::now(),
            anim_rect: RECT { left: 0, top: 0, right: 0, bottom: 0 },
            preview: None,
            partial: false,
            last_battery: HashMap::new(),
            low_warned: HashSet::new(),
            tray_batt: HashMap::new(),
            next_tray_id: 100,
            upd: Upd::Idle,
            mutex,
            page: 0,
            list_scroll: 0,
            list_max: 0,
            home_scroll: 0,
            home_max: 0,
            widgets: Vec::new(),
            widget_drag: None,
            auto_off: 0,
            pending: HashSet::new(),
            recording: None,
            hk_ids: Vec::new(),
            hk_failed: HashSet::new(),
            set_scroll: 0,
            set_max: 0,
            clip: None,
            hist: HashMap::new(),
            hist_days: 1,
            show_hist: false,
            first_scan: true,
            quick_scans: 4,
        });
        APP = Box::into_raw(app_box);
        THEME = make_theme(app().config.theme, app().config.accent, app().config.square);

        let style = WIN_STYLE;
        let mut rc = RECT { left: 0, top: 0, right: s(WIN_W), bottom: s(WIN_H) };
        AdjustWindowRectExForDpi(&mut rc, style, 0, 0, GetDpiForSystem());
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("Centro LED").as_ptr(),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            rc.right - rc.left,
            rc.bottom - rc.top,
            null_mut(),
            null_mut(),
            hinst,
            null(),
        );
        app().hwnd = hwnd;
        if render {
            do_render(&args);
            return;
        }
        // Si la ventana nace en una pantalla con otra escala, ajustarla ya.
        let dpi = GetDpiForWindow(hwnd);
        if dpi != GetDpiForSystem() {
            set_scale(dpi);
            let mut rc = RECT { left: 0, top: 0, right: s(WIN_W), bottom: s(WIN_H) };
            AdjustWindowRectExForDpi(&mut rc, WIN_STYLE, 0, 0, dpi);
            SetWindowPos(hwnd, null_mut(), 0, 0, rc.right - rc.left, rc.bottom - rc.top, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
        // Versión instalada: mantener al día la versión que muestra "Aplicaciones instaladas".
        let mut ukey: HKEY = null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, wide(UNINSTALL_KEY).as_ptr(), 0, KEY_SET_VALUE, &mut ukey) == 0 {
            let v = wide(update::VERSION);
            RegSetValueExW(ukey, wide("DisplayVersion").as_ptr(), 0, REG_SZ, v.as_ptr() as *const u8, (v.len() * 2) as u32);
            RegCloseKey(ukey);
        }
        // Si el inicio automático apunta a un .exe que ya no existe (se movió), corregirlo.
        // Si apunta a otra copia que sí existe (p. ej. se abrió una portable), no tocarlo.
        if app().config.fast_start {
            if autostart_value().is_some() {
                set_autostart(false);
                app().autostart = false;
            }
            state::fast_start_check();
        } else if let Some(v) = autostart_value() {
            let target = v.trim_start_matches('"').split('"').next().unwrap_or("").to_string();
            if !std::path::Path::new(&target).exists() {
                set_autostart(true);
            }
        }
        // Barra de título y menús con los colores del tema
        style_window(hwnd);
        menu_mode(th().light);
        if !update_tray(true) {
            // Con el inicio rápido la barra de tareas puede no existir todavía.
            SetTimer(hwnd, TIMER_TRAY, 2000, None);
        }
        // Bloqueo / desbloqueo, pantalla encendida / apagada y atajos de teclado.
        WTSRegisterSessionNotification(hwnd, 0);
        RegisterPowerSettingNotification(hwnd as HANDLE, &GUID_CONSOLE_DISPLAY_STATE, 0);
        state::register_hotkeys();
        if start_hidden {
            hide_to_tray();
            // Windows puede ignorar el primer ShowWindow si quien la lanzó pidió mostrarla.
            if IsWindowVisible(hwnd) != 0 {
                ShowWindow(hwnd, SW_HIDE);
            }
        } else {
            ShowWindow(hwnd, SW_SHOW);
        }
        start_scan();
        // Batería cada minuto: el resto del tiempo la app está dormida.
        SetTimer(hwnd, TIMER_BATTERY, 60_000, None);
        SetTimer(hwnd, TIMER_UPDATE, 8_000, None);
        // Widgets de batería en pantallas secundarias: reposicionar y mantener encima
        SetTimer(hwnd, TIMER_WIDGET, 1_500, None);
        if updated {
            set_status(&format!("✓ Centro LED actualizado a la versión {}", update::VERSION));
        }

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
