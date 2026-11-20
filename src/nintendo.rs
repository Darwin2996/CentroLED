//! Mandos de Nintendo Switch (Pro Controller y Joy-Con): batería, LEDs de jugador y luz Home.
//! Protocolo según el controlador hid-nintendo de Linux.

use crate::devices::{Battery, Device, Driver, LedCaps, PadExtra};
use crate::hid::{HidDev, HidInfo};
use std::collections::HashSet;
use std::time::{Duration, Instant};

const VID: u16 = 0x057E;
const SUBCMD_DEVICE_INFO: u8 = 0x02;
const SUBCMD_PLAYER_LEDS: u8 = 0x30;
const SUBCMD_HOME_LIGHT: u8 = 0x38;

fn model(pid: u16) -> Option<(&'static str, bool)> {
    // (nombre, tiene luz Home)
    match pid {
        0x2009 => Some(("Mando Pro de Switch", true)),
        0x2006 => Some(("Joy-Con (I)", false)),
        0x2007 => Some(("Joy-Con (D)", true)),
        0x2017 => Some(("Mando SNES de Switch", false)),
        _ => None,
    }
}

/// Envía un subcomando (report 0x01) y espera su respuesta (report 0x21).
fn subcmd(dev: &HidDev, out_len: usize, id: u8, data: &[u8]) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; out_len.max(49)];
    buf[0] = 0x01;
    static COUNTER: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
    buf[1] = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed) & 0x0F;
    // Vibración neutra
    buf[2..10].copy_from_slice(&[0x00, 0x01, 0x40, 0x40, 0x00, 0x01, 0x40, 0x40]);
    buf[10] = id;
    buf[11..11 + data.len()].copy_from_slice(data);
    if !dev.write(&buf) {
        return None;
    }
    let deadline = Instant::now() + Duration::from_millis(600);
    let mut r = vec![0u8; 64];
    while Instant::now() < deadline {
        if let Some(n) = dev.read(&mut r, 250) {
            if n > 14 && r[0] == 0x21 && r[14] == id {
                return Some(r[..n].to_vec());
            }
        }
    }
    None
}

/// Batería del byte 2 de la respuesta: nivel en los 3 bits altos (0..4) y bit de carga.
fn battery(reply: &[u8]) -> Battery {
    let b = reply[2];
    let level = (b >> 5).min(4);
    Battery { pct: [5, 25, 50, 75, 100][level as usize], charging: b & 0x10 != 0, approx: true }
}

pub fn scan(all: &[HidInfo], handled: &mut HashSet<(u16, u16)>) -> Vec<Device> {
    let mut out = Vec::new();
    for i in all.iter().filter(|i| i.vid == VID && i.out_len > 0) {
        let Some((name, _)) = model(i.pid) else { continue };
        if !handled.insert((i.vid, i.pid)) {
            continue;
        }
        let battery = HidDev::open(&i.path).and_then(|d| subcmd(&d, i.out_len as usize, SUBCMD_DEVICE_INFO, &[])).map(|r| battery(&r));
        out.push(Device {
            key: format!("ns-{:04x}", i.pid),
            name: name.into(),
            kind: "Mando",
            conn: if i.out_len >= 49 { "Bluetooth" } else { "USB" },
            battery,
            battery_note: "Sin datos (conéctalo por Bluetooth)",
            led: Some(LedCaps { modes: vec![("LEDs", 1)], speed: false, multicolor: false, volatile: true, zones: vec![], pad_extras: true }),
            driver: Driver::Switch { path: i.path.clone(), out_len: i.out_len, home: model(i.pid).map(|m| m.1).unwrap_or(false) },
        });
    }
    out
}

/// LEDs de jugador (bits 0..3) y brillo de la luz Home (0 apagada .. 3 máxima, en player_bright).
pub fn apply(path: &[u16], out_len: u16, home: bool, x: &PadExtra) -> Result<(), String> {
    let dev = HidDev::open(path).ok_or("No se pudo abrir el mando")?;
    subcmd(&dev, out_len as usize, SUBCMD_PLAYER_LEDS, &[x.player & 0x0F]).ok_or("El mando no respondió")?;
    if home {
        let b = [0u8, 5, 10, 15][x.player_bright.min(3) as usize];
        subcmd(&dev, out_len as usize, SUBCMD_HOME_LIGHT, &[0x01, b << 4, b | (b << 4), 0x11, 0x11]);
    }
    Ok(())
}
