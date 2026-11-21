//! "Exportar diagnóstico": un .txt con lo necesario para investigar un dispositivo que no va.
//! Se guarda donde elija el usuario y nunca se envía a ningún sitio. No incluye números de serie,
//! identificadores de instancia ni rutas con el nombre de usuario.

use super::*;
use std::fmt::Write as _;
use windows_sys::Win32::System::Registry::{RegGetValueW, RRF_RT_REG_DWORD, RRF_RT_REG_SZ};

fn reg_str(key: &str, name: &str) -> String {
    unsafe {
        let mut buf = [0u16; 256];
        let mut size = (buf.len() * 2) as u32;
        if RegGetValueW(HKEY_LOCAL_MACHINE, wide(key).as_ptr(), wide(name).as_ptr(), RRF_RT_REG_SZ, null_mut(), buf.as_mut_ptr() as _, &mut size) != 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..(size as usize / 2)]).trim_end_matches('\0').to_string()
    }
}

fn reg_dword(key: &str, name: &str) -> u32 {
    unsafe {
        let mut v = 0u32;
        let mut size = 4u32;
        RegGetValueW(HKEY_LOCAL_MACHINE, wide(key).as_ptr(), wide(name).as_ptr(), RRF_RT_REG_DWORD, null_mut(), &mut v as *mut u32 as _, &mut size);
        v
    }
}

unsafe extern "system" fn monitor_cb(m: HMONITOR, _dc: HDC, _r: *mut RECT, data: LPARAM) -> BOOL {
    let out = &mut *(data as *mut String);
    let mut mi: MONITORINFO = std::mem::zeroed();
    mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    GetMonitorInfoW(m, &mut mi);
    let (mut dx, mut dy) = (0u32, 0u32);
    GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
    let r = mi.rcMonitor;
    let _ = writeln!(
        out,
        "  {}x{} en ({}, {}) · {} ppp ({} %){}",
        r.right - r.left,
        r.bottom - r.top,
        r.left,
        r.top,
        dx,
        dx * 100 / 96,
        if mi.dwFlags & 1 != 0 { " · principal" } else { "" }
    );
    1
}

fn driver_name(d: &Device) -> &'static str {
    use devices::Driver::*;
    match &d.driver {
        Tempest { .. } => "Tempest",
        DualSense { bt: true, .. } => "DualSense (Bluetooth)",
        DualSense { .. } => "DualSense (USB)",
        DualShock4 { bt: true, .. } => "DualShock 4 (Bluetooth)",
        DualShock4 { .. } => "DualShock 4 (USB)",
        Logitech { .. } => "Logitech HID++",
        Switch { .. } => "Nintendo Switch",
        OpenRgb { .. } => "OpenRGB",
        LampArray { .. } => "HID LampArray",
        None => "sólo batería / informativo",
    }
}

fn save_dialog() -> Option<String> {
    let mut buf = [0u16; 520];
    let name: Vec<u16> = "CentroLED-diagnostico.txt".encode_utf16().collect();
    buf[..name.len()].copy_from_slice(&name);
    let filter: Vec<u16> = "Texto (*.txt)\0*.txt\0\0".encode_utf16().collect();
    let ext = wide("txt");
    let title = wide("Guardar diagnóstico de Centro LED");
    let mut ofn: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    ofn.hwndOwner = app().hwnd;
    ofn.lpstrFilter = filter.as_ptr();
    ofn.lpstrFile = buf.as_mut_ptr();
    ofn.nMaxFile = buf.len() as u32;
    ofn.lpstrDefExt = ext.as_ptr();
    ofn.lpstrTitle = title.as_ptr();
    ofn.Flags = OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
    if unsafe { GetSaveFileNameW(&mut ofn) } == 0 {
        return None;
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..n]))
}

pub fn export() {
    let Some(path) = save_dialog() else { return };
    let a = app();
    let mut t = String::new();
    let mode = if portable_dir().is_some() {
        "portable"
    } else if std::env::current_exe().map(|e| e.display().to_string().to_lowercase().contains("programs\\centroled")).unwrap_or(false) {
        "instalada"
    } else {
        "sin instalar"
    };
    let mut st: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut st) };
    let _ = writeln!(t, "Diagnóstico de Centro LED · {:02}/{:02}/{} {:02}:{:02}", st.wDay, st.wMonth, st.wYear, st.wHour, st.wMinute);
    let _ = writeln!(t, "Este archivo sólo se ha guardado en tu equipo. No contiene números de serie ni tu nombre de usuario.\n");
    let _ = writeln!(t, "Centro LED {} ({mode})", update::VERSION);
    let nt = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion";
    let build: u32 = reg_str(nt, "CurrentBuild").parse().unwrap_or(0);
    let product = reg_str(nt, "ProductName").replace("Windows 10", if build >= 22000 { "Windows 11" } else { "Windows 10" });
    let _ = writeln!(t, "{} {} · compilación {}.{}", product, reg_str(nt, "DisplayVersion"), build, reg_dword(nt, "UBR"));
    let _ = writeln!(t, "\nPantallas (ventana a {} ppp):", unsafe { GetDpiForWindow(a.hwnd) });
    unsafe { EnumDisplayMonitors(null_mut(), null(), Some(monitor_cb), &mut t as *mut String as LPARAM) };
    let _ = writeln!(t, "\nAjustes:");
    let c = &a.config;
    let _ = writeln!(
        t,
        "  batería junto al reloj: {} · widget en otras pantallas: {} · aviso batería baja: {} %",
        !c.hide_tray_battery, !c.hide_widget, c.low_battery
    );
    let _ = writeln!(t, "  inicio con Windows: {} · inicio rápido: {}", a.autostart || c.fast_start, c.fast_start);
    let _ = writeln!(t, "  apagado automático: bloqueo {} · pantalla {} · suspensión {} · apagar equipo {}", c.auto[0], c.auto[1], c.auto[2], c.auto[3]);
    let _ = writeln!(t, "  perfiles: {} · atajos: {} ({} no disponibles)", c.profiles.len(), c.hotkeys.len(), a.hk_failed.len());

    let _ = writeln!(t, "\nDispositivos reconocidos ({}):", a.devices.len());
    for d in &a.devices {
        let lights = match &d.led {
            Some(l) => format!("luces: {} efectos{}{}", l.modes.len(), if l.zones.is_empty() { String::new() } else { format!(", {} zonas", l.zones.len()) }, if l.volatile { ", se reaplican al conectar" } else { "" }),
            None => "sin luces".into(),
        };
        let _ = writeln!(t, "  {} · {} · {} · {} · batería: {} · {}", d.name, d.kind, d.conn, driver_name(d), battery_text(d), lights);
    }
    let count = |c: &str| a.devices.iter().filter(|d| d.conn == c).count();
    let _ = writeln!(t, "  Bluetooth: {} · OpenRGB: {}", count("Bluetooth"), if count("OpenRGB") > 0 { "conectado" } else { "no detectado" });

    if let Ok(e) = std::fs::read_to_string(config_path().with_file_name("error.txt")) {
        let e: String = e.chars().take(1500).collect();
        let _ = writeln!(t, "\nÚltimo error guardado:\n  {}", e.replace('\n', "\n  "));
    }

    set_status("Generando el diagnóstico…");
    let hwnd = a.hwnd as isize;
    std::thread::spawn(move || {
        let list = {
            let _g = HID_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            hid::enumerate()
        };
        let _ = writeln!(t, "\nColecciones HID ({}):", list.len());
        let _ = writeln!(t, "  VID:PID    página:uso   in/out/feature  interfaz  producto");
        for i in &list {
            let _ = writeln!(
                t,
                "  {:04X}:{:04X}  {:04X}:{:04X}    {:>3}/{:>3}/{:>3}     {:<8}  {}",
                i.vid,
                i.pid,
                i.usage_page,
                i.usage,
                i.in_len,
                i.out_len,
                i.feat_len,
                i.interface.map(|n| format!("mi_{n:02}")).unwrap_or_else(|| "—".into()),
                i.product
            );
        }
        let file = std::path::Path::new(&path).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let msg = match std::fs::write(&path, t.replace('\n', "\r\n")) {
            Ok(()) => format!("✓ Diagnóstico guardado en {file}"),
            Err(e) => format!("No se pudo guardar el diagnóstico: {e}"),
        };
        unsafe { PostMessageW(hwnd as HWND, WM_APPLIED, 1, Box::into_raw(Box::new(msg)) as LPARAM) };
    });
}
