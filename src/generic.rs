//! Compatibilidad genérica basada en estándares:
//! - Batería HID estándar ("Battery Strength", página 0x06) que exponen muchos periféricos.
//! - Ratones, teclados y cascos Razer (protocolo de informes de 90 bytes): batería.
//! - Luces HID LampArray ("Dynamic Lighting" de Windows 11): color de todo el dispositivo.

use crate::devices::{Battery, Device, Driver, LedCaps, MODE_OFF};
use crate::hid::{HidDev, HidInfo, FEATURE, INPUT};
use std::collections::HashSet;
use std::time::Duration;

/// Tipo de dispositivo según las colecciones estándar que tiene.
fn kind_of(i: &HidInfo, all: &[HidInfo]) -> &'static str {
    let sib: Vec<&HidInfo> = all.iter().filter(|o| o.vid == i.vid && o.pid == i.pid).collect();
    let has = |page: u16, usage: u16| sib.iter().any(|o| o.usage_page == page && o.usage == usage);
    let name = i.product.to_lowercase();
    if name.contains("headset") || name.contains("cascos") || name.contains("headphone") {
        "Cascos"
    } else if has(1, 2) && !has(1, 6) {
        "Ratón"
    } else if has(1, 6) {
        "Teclado"
    } else if has(1, 4) || has(1, 5) {
        "Mando"
    } else {
        "Otro"
    }
}

fn product_name(i: &HidInfo, all: &[HidInfo]) -> Option<String> {
    all.iter()
        .filter(|o| o.vid == i.vid && o.pid == i.pid)
        .map(|o| o.product.trim().to_string())
        .find(|p| !p.is_empty() && !p.eq_ignore_ascii_case("usb device"))
}

// ---------------------------------------------------------------- batería HID estándar

const PAGE_GENERIC_DEVICE: u16 = 0x06;
const USAGE_BATTERY_STRENGTH: u16 = 0x20;

fn hid_battery(i: &HidInfo) -> Option<u8> {
    let dev = HidDev::open_feature(&i.path)?;
    let pp = dev.preparsed()?;
    for (kind, len) in [(FEATURE, pp.caps.FeatureReportByteLength), (INPUT, pp.caps.InputReportByteLength)] {
        let Some(cap) = pp.value_cap(kind, PAGE_GENERIC_DEVICE, USAGE_BATTERY_STRENGTH) else { continue };
        let mut buf = vec![0u8; len as usize];
        if buf.is_empty() {
            continue;
        }
        buf[0] = cap.report_id;
        let ok = if kind == FEATURE { dev.get_feature(&mut buf) } else { dev.get_input_report(&mut buf) };
        if !ok {
            continue;
        }
        let v = pp.get(kind, PAGE_GENERIC_DEVICE, &cap, USAGE_BATTERY_STRENGTH, &buf)? as i32;
        let (min, max) = if cap.max > cap.min { (cap.min, cap.max) } else { (0, 100) };
        return Some(((v - min) * 100 / (max - min).max(1)).clamp(0, 100) as u8);
    }
    None
}

// ---------------------------------------------------------------- Razer

const RAZER_VID: u16 = 0x1532;

/// Petición Razer: 90 bytes [estado, transacción, restantes(2), protocolo, tamaño, clase, comando,
/// argumentos(80), crc, reservado] precedidos del report ID 0.
fn razer_query(dev: &HidDev, tid: u8, class: u8, cmd: u8) -> Option<[u8; 91]> {
    let mut req = [0u8; 91];
    req[2] = tid;
    req[6] = 0x02;
    req[7] = class;
    req[8] = cmd;
    req[89] = req[3..89].iter().fold(0, |a, b| a ^ b);
    if !dev.set_feature(&req) {
        return None;
    }
    for _ in 0..6 {
        std::thread::sleep(Duration::from_millis(40));
        let mut r = [0u8; 91];
        if dev.get_feature(&mut r) && r[7] == class && r[8] == cmd {
            match r[1] {
                0x02 => return Some(r),  // correcto
                0x01 => continue,        // ocupado
                _ => return None,        // no admitido
            }
        }
    }
    None
}

fn razer_battery(i: &HidInfo) -> Option<Battery> {
    let dev = HidDev::open_feature(&i.path)?;
    for tid in [0x1F, 0x3F, 0xFF, 0x9F] {
        if let Some(r) = razer_query(&dev, tid, 0x07, 0x80) {
            let pct = (r[10] as u32 * 100 / 255) as u8;
            let charging = razer_query(&dev, tid, 0x07, 0x84).map(|c| c[10] != 0).unwrap_or(false);
            return Some(Battery { pct, charging, approx: false });
        }
    }
    None
}

// ---------------------------------------------------------------- LampArray (Dynamic Lighting)

const PAGE_LIGHTING: u16 = 0x59;
const U_LAMP_ARRAY: u16 = 0x01;
const U_LAMP_COUNT: u16 = 0x03;
const U_RED: u16 = 0x51;
const U_GREEN: u16 = 0x52;
const U_BLUE: u16 = 0x53;
const U_INTENSITY: u16 = 0x54;
const U_UPDATE_FLAGS: u16 = 0x55;
const U_LAMP_ID_START: u16 = 0x61;
const U_LAMP_ID_END: u16 = 0x62;
const U_AUTONOMOUS: u16 = 0x71;

fn lamp_count(dev: &HidDev) -> Option<u32> {
    let pp = dev.preparsed()?;
    let cap = pp.value_cap(FEATURE, PAGE_LIGHTING, U_LAMP_COUNT)?;
    let mut buf = vec![0u8; pp.caps.FeatureReportByteLength as usize];
    buf[0] = cap.report_id;
    dev.get_feature(&mut buf).then_some(())?;
    pp.get(FEATURE, PAGE_LIGHTING, &cap, U_LAMP_COUNT, &buf)
}

/// Pone todas las luces del dispositivo en un color (desactiva antes el modo autónomo).
pub fn lamparray_apply(path: &[u16], rgb: (u8, u8, u8)) -> Result<(), String> {
    let dev = HidDev::open_feature(path).ok_or("No se pudo abrir el dispositivo")?;
    let pp = dev.preparsed().ok_or("Descriptor no válido")?;
    let count = lamp_count(&dev).unwrap_or(1).max(1);
    let flen = pp.caps.FeatureReportByteLength as usize;
    // 1. Control: modo autónomo desactivado (la app manda)
    if let Some(cap) = pp.value_cap(FEATURE, PAGE_LIGHTING, U_AUTONOMOUS) {
        let mut buf = vec![0u8; flen];
        buf[0] = cap.report_id;
        pp.set(FEATURE, PAGE_LIGHTING, cap.link, U_AUTONOMOUS, 0, &mut buf);
        dev.set_feature(&buf);
    }
    // 2. Actualizar el rango completo de lámparas
    let cap = pp.value_cap(FEATURE, PAGE_LIGHTING, U_LAMP_ID_START).ok_or("El dispositivo no admite actualizar luces por rango")?;
    let mut buf = vec![0u8; flen];
    buf[0] = cap.report_id;
    let l = cap.link;
    let scale = |c: u8, u: u16| -> u32 {
        let max = pp.value_cap(FEATURE, PAGE_LIGHTING, u).map(|c| c.max).unwrap_or(255).max(1) as u32;
        c as u32 * max / 255
    };
    pp.set(FEATURE, PAGE_LIGHTING, l, U_UPDATE_FLAGS, 1, &mut buf);
    pp.set(FEATURE, PAGE_LIGHTING, l, U_LAMP_ID_START, 0, &mut buf);
    pp.set(FEATURE, PAGE_LIGHTING, l, U_LAMP_ID_END, count - 1, &mut buf);
    pp.set(FEATURE, PAGE_LIGHTING, l, U_RED, scale(rgb.0, U_RED), &mut buf);
    pp.set(FEATURE, PAGE_LIGHTING, l, U_GREEN, scale(rgb.1, U_GREEN), &mut buf);
    pp.set(FEATURE, PAGE_LIGHTING, l, U_BLUE, scale(rgb.2, U_BLUE), &mut buf);
    let on = if rgb == (0, 0, 0) { 0 } else { 255 };
    pp.set(FEATURE, PAGE_LIGHTING, l, U_INTENSITY, scale(on, U_INTENSITY), &mut buf);
    if dev.set_feature(&buf) { Ok(()) } else { Err("El dispositivo no aceptó el color".into()) }
}

// ---------------------------------------------------------------- búsqueda

pub fn scan(all: &[HidInfo], handled: &mut HashSet<(u16, u16)>) -> Vec<Device> {
    let mut out: Vec<Device> = Vec::new();
    // Luces Dynamic Lighting
    for i in all.iter().filter(|i| i.usage_page == PAGE_LIGHTING && i.usage == U_LAMP_ARRAY) {
        if handled.contains(&(i.vid, i.pid)) || out.iter().any(|d| d.key == format!("lamp-{:04x}-{:04x}", i.vid, i.pid)) {
            continue;
        }
        let name = product_name(i, all).unwrap_or_else(|| format!("Dispositivo RGB {:04X}:{:04X}", i.vid, i.pid));
        out.push(Device {
            key: format!("lamp-{:04x}-{:04x}", i.vid, i.pid),
            name,
            kind: kind_of(i, all),
            conn: "USB · Dynamic Lighting",
            battery: None,
            battery_note: "—",
            led: Some(LedCaps { modes: vec![("Estático", 1), ("Apagado", MODE_OFF)], speed: false, multicolor: false, volatile: true, zones: vec![], pad_extras: false }),
            driver: Driver::LampArray { path: i.path.clone() },
        });
    }
    // Razer: batería por su protocolo (interfaz 0 con informes de 90 bytes)
    let mut razer_done: HashSet<u16> = HashSet::new();
    for i in all.iter().filter(|i| i.vid == RAZER_VID && i.feat_len == 91) {
        if handled.contains(&(i.vid, i.pid)) || !razer_done.insert(i.pid) {
            continue;
        }
        if let Some(b) = razer_battery(i) {
            let name = product_name(i, all).unwrap_or_else(|| "Razer".into());
            let key = format!("lamp-{:04x}-{:04x}", i.vid, i.pid);
            if let Some(d) = out.iter_mut().find(|d| d.key == key) {
                d.battery = Some(b); // mismo dispositivo con Dynamic Lighting
            } else {
                out.push(Device {
                    key: format!("razer-{:04x}", i.pid),
                    name,
                    kind: kind_of(i, all),
                    conn: "Razer",
                    battery: Some(b),
                    battery_note: "",
                    led: None,
                    driver: Driver::None,
                });
            }
        }
    }
    // Batería HID estándar en cualquier otro dispositivo
    let mut seen: HashSet<(u16, u16)> = HashSet::new();
    for i in all.iter() {
        let id = (i.vid, i.pid);
        if handled.contains(&id) || seen.contains(&id) || i.vid == 0 || i.vid == RAZER_VID {
            continue;
        }
        if let Some(pct) = hid_battery(i) {
            seen.insert(id);
            let key = format!("lamp-{:04x}-{:04x}", i.vid, i.pid);
            let battery = Some(Battery { pct, charging: false, approx: false });
            if let Some(d) = out.iter_mut().find(|d| d.key == key) {
                d.battery = battery;
                continue;
            }
            out.push(Device {
                key: format!("hidbat-{:04x}-{:04x}", i.vid, i.pid),
                name: product_name(i, all).unwrap_or_else(|| format!("Dispositivo {:04X}:{:04X}", i.vid, i.pid)),
                kind: kind_of(i, all),
                conn: "USB / inalámbrico",
                battery,
                battery_note: "",
                led: None,
                driver: Driver::None,
            });
        }
    }
    // Lo encontrado aquí no se repite en la lista informativa de "otros dispositivos".
    for d in &out {
        if let Some(rest) = d.key.strip_prefix("lamp-").or_else(|| d.key.strip_prefix("hidbat-")) {
            if let (Ok(v), Ok(p)) = (u16::from_str_radix(&rest[..4], 16), u16::from_str_radix(&rest[5..9], 16)) {
                handled.insert((v, p));
            }
        } else if let Some(p) = d.key.strip_prefix("razer-").and_then(|p| u16::from_str_radix(p, 16).ok()) {
            handled.insert((RAZER_VID, p));
        }
    }
    out
}
