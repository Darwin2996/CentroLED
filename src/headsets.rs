//! Batería de cascos inalámbricos de otras marcas (SteelSeries, HyperX, Corsair).
//! Protocolos tomados del proyecto abierto HeadsetControl.

use crate::devices::{Battery, Device, Driver};
use crate::hid::{HidDev, HidInfo};
use std::collections::HashSet;
use std::time::{Duration, Instant};

enum Proto {
    /// SteelSeries con petición directa: (petición, byte de batería, (byte de estado, valor desconectado, valor cargando), mín, máx)
    SsDirect(&'static [u8], usize, Option<(usize, u8, u8)>, u8, u8),
    /// SteelSeries de la familia 7+/Nova (petición 0x00 0xB0): variante de interpretación
    SsStatus(SsKind),
    /// SteelSeries Nova Pro Wireless (petición 0x06 0xB0)
    SsNovaPro,
    /// Corsair Void y similares (petición 0xC9 0x64)
    Corsair,
    /// HyperX Cloud Alpha Wireless (tres peticiones 0x21 0xBB)
    HyperxAlpha,
    /// HyperX Cloud Flight (tensión de la batería)
    HyperxFlight,
    /// HyperX Cloud II Wireless (0x06 0xFF 0xBB)
    HyperxCloud2,
}

#[derive(Clone, Copy)]
enum SsKind {
    Plus,          // Arctis 7+: [1]=1 desconectado, [2]=nivel 0..4, [3]=1 cargando
    Nova5,         // Nova 5: [1]=2 desconectado, [3]=% , [4]=1 cargando
    Nova7Discrete, // Nova 7 antiguos: [3]=0 desconectado, 1/2 cargando, [2]=nivel 0..4
    Nova7Percent,  // Nova 7 nuevos: igual pero [2] ya es porcentaje
}

struct Spec {
    name: &'static str,
    vid: u16,
    pids: &'static [u16],
    iface: Option<u8>,
    proto: Proto,
}

const SPECS: &[Spec] = &[
    Spec { name: "SteelSeries Arctis 1 / 7X", vid: 0x1038, pids: &[0x12b3, 0x12b6, 0x12d7], iface: Some(3), proto: Proto::SsDirect(&[0x06, 0x12], 3, Some((2, 0x01, 0xFF)), 0, 100) },
    Spec { name: "SteelSeries Arctis 7 / Pro", vid: 0x1038, pids: &[0x1260, 0x12ad, 0x1252], iface: Some(5), proto: Proto::SsDirect(&[0x06, 0x18], 2, None, 0, 100) },
    Spec { name: "SteelSeries Arctis 9", vid: 0x1038, pids: &[0x12c2], iface: Some(0), proto: Proto::SsDirect(&[0x00, 0x20], 3, Some((4, 0xFE, 0x01)), 0x64, 0x9A) },
    Spec { name: "SteelSeries Arctis 7+", vid: 0x1038, pids: &[0x220e, 0x2212, 0x2216, 0x2236], iface: Some(3), proto: Proto::SsStatus(SsKind::Plus) },
    Spec { name: "SteelSeries Arctis Nova 5", vid: 0x1038, pids: &[0x2232, 0x2253], iface: Some(3), proto: Proto::SsStatus(SsKind::Nova5) },
    Spec { name: "SteelSeries Arctis Nova 7", vid: 0x1038, pids: &[0x2202, 0x2206, 0x220a, 0x223a, 0x227a, 0x22a4], iface: Some(3), proto: Proto::SsStatus(SsKind::Nova7Discrete) },
    Spec { name: "SteelSeries Arctis Nova 7", vid: 0x1038, pids: &[0x22a1, 0x227e, 0x2258, 0x229e, 0x22ad, 0x22a9, 0x22a5], iface: Some(3), proto: Proto::SsStatus(SsKind::Nova7Percent) },
    Spec { name: "SteelSeries Arctis Nova Pro Wireless", vid: 0x1038, pids: &[0x12e0, 0x12e5], iface: Some(4), proto: Proto::SsNovaPro },
    Spec {
        name: "Corsair Void",
        vid: 0x1b1c,
        pids: &[0x1b1c, 0x1b27, 0x0a14, 0x0a16, 0x0a17, 0x0a1d, 0x0a1a, 0x1b2a, 0x1b23, 0x1b29, 0x0a55, 0x0a51, 0x0a52, 0x0a38, 0x0a4f, 0x0a2b, 0x0a75, 0x0a56],
        iface: Some(3),
        proto: Proto::Corsair,
    },
    Spec { name: "HyperX Cloud Alpha Wireless", vid: 0x03f0, pids: &[0x098d], iface: None, proto: Proto::HyperxAlpha },
    Spec { name: "HyperX Cloud Flight", vid: 0x0951, pids: &[0x16c4, 0x1723], iface: None, proto: Proto::HyperxFlight },
    Spec { name: "HyperX Cloud II Wireless", vid: 0x03f0, pids: &[0x0696, 0x018b], iface: Some(0), proto: Proto::HyperxCloud2 },
];

enum Query {
    Ok(Battery),
    Offline,
    Fail,
}

/// Envía una petición (rellenada al tamaño del report) y lee respuestas durante un tiempo.
/// Devuelve los datos "como los da hidapi": sin el byte de report ID cuando este es 0.
fn transact(dev: &HidDev, info: &HidInfo, req: &[u8], accept: impl Fn(&[u8]) -> bool) -> Option<Vec<u8>> {
    let mut out = vec![0u8; (info.out_len as usize).max(req.len())];
    out[..req.len()].copy_from_slice(req);
    if !dev.write(&out) {
        return None;
    }
    let deadline = Instant::now() + Duration::from_millis(800);
    let mut buf = vec![0u8; (info.in_len as usize).max(8)];
    while Instant::now() < deadline {
        if let Some(n) = dev.read(&mut buf, 300) {
            let data = if buf[0] == 0 { &buf[1..n] } else { &buf[..n] };
            if accept(data) {
                return Some(data.to_vec());
            }
        }
    }
    None
}

fn map(v: i32, a: i32, b: i32) -> u8 {
    ((v - a) * 100 / (b - a).max(1)).clamp(0, 100) as u8
}

fn query(dev: &HidDev, info: &HidInfo, proto: &Proto) -> Query {
    let bat = |pct: u8, charging: bool, approx: bool| Query::Ok(Battery { pct, charging, approx });
    match proto {
        Proto::SsDirect(req, bidx, status, min, max) => {
            let Some(r) = transact(dev, info, req, |d| d.len() > *bidx) else { return Query::Fail };
            let mut charging = false;
            if let Some((sidx, offline, chg)) = status {
                let s = r.get(*sidx).copied().unwrap_or(0);
                if s == *offline {
                    return Query::Offline;
                }
                charging = s == *chg;
            }
            bat(map(r[*bidx] as i32, *min as i32, *max as i32), charging, false)
        }
        Proto::SsStatus(kind) => {
            let Some(r) = transact(dev, info, &[0x00, 0xb0], |d| d.len() > 5 && d[0] == 0xb0) else { return Query::Fail };
            match kind {
                SsKind::Plus => {
                    if r[1] == 0x01 {
                        return Query::Offline;
                    }
                    bat(map(r[2] as i32, 0, 4), r[3] == 0x01, true)
                }
                SsKind::Nova5 => {
                    if r[1] == 0x02 {
                        return Query::Offline;
                    }
                    bat(r[3].min(100), r[4] == 0x01, false)
                }
                SsKind::Nova7Discrete | SsKind::Nova7Percent => {
                    if r[3] == 0x00 {
                        return Query::Offline;
                    }
                    let charging = r[3] == 0x01 || r[3] == 0x02;
                    match kind {
                        SsKind::Nova7Discrete => bat(map(r[2] as i32, 0, 4), charging, true),
                        _ => bat(r[2].min(100), charging, false),
                    }
                }
            }
        }
        Proto::SsNovaPro => {
            let Some(r) = transact(dev, info, &[0x06, 0xb0], |d| d.len() >= 16) else { return Query::Fail };
            if r[15] == 0x01 {
                return Query::Offline;
            }
            bat(map(r[6] as i32, 0, 8), r[15] == 0x02, true)
        }
        Proto::Corsair => {
            let Some(r) = transact(dev, info, &[0xC9, 0x64], |d| d.len() >= 5) else { return Query::Fail };
            match r[4] {
                0 => Query::Offline,
                s @ 1..=5 => bat((r[2] & 0x7F).min(100), s == 4 || s == 5, false),
                _ => Query::Fail,
            }
        }
        Proto::HyperxAlpha => {
            let req = |c: u8| [0x21, 0xbb, c];
            let ok = |c: u8| move |d: &[u8]| d.len() > 3 && d[0] == 0x21 && d[1] == 0xbb && d[2] == c;
            let Some(conn) = transact(dev, info, &req(0x03), ok(0x03)) else { return Query::Fail };
            if conn[3] == 0x01 {
                return Query::Offline;
            }
            let charging = transact(dev, info, &req(0x0c), ok(0x0c)).map(|r| r[3] == 0x01).unwrap_or(false);
            match transact(dev, info, &req(0x0b), ok(0x0b)) {
                Some(r) => bat(r[3].min(100), charging, false),
                None => Query::Fail,
            }
        }
        Proto::HyperxFlight => {
            let Some(r) = transact(dev, info, &[0x21, 0xff, 0x05], |d| d.len() >= 5 && d[0] == 0x21) else { return Query::Fail };
            let mv = ((r[3] as u32) << 8) | r[4] as u32;
            if mv > 0x100B {
                return bat(100, true, true);
            }
            let v = mv as f64;
            let pct = if mv <= 3648 {
                0.00125 * v
            } else if mv > 3975 {
                100.0
            } else {
                0.00000002547505 * v.powi(4) - 0.0003900299 * v.powi(3) + 2.238321 * v.powi(2) - 5706.256 * v + 5452299.0
            };
            bat(pct.round().clamp(0.0, 100.0) as u8, false, false)
        }
        Proto::HyperxCloud2 => {
            let req = |c: u8| [0x06, 0xff, 0xbb, c];
            let ok = |c: u8| move |d: &[u8]| d.len() >= 8 && d[0] == 0x06 && d[1] == 0xff && d[2] == 0xbb && d[3] == c;
            let Some(level) = transact(dev, info, &req(0x02), ok(0x02)) else { return Query::Fail };
            let charging = transact(dev, info, &req(0x03), ok(0x03)).map(|r| r[4] == 1).unwrap_or(false);
            bat(level[7].min(100), charging, false)
        }
    }
}

/// Busca cascos conocidos entre las colecciones HID y lee su batería.
pub fn scan(all: &[HidInfo], handled: &mut HashSet<(u16, u16)>) -> Vec<Device> {
    let mut out = Vec::new();
    for spec in SPECS {
        for &pid in spec.pids {
            let cands: Vec<&HidInfo> = all
                .iter()
                .filter(|i| i.vid == spec.vid && i.pid == pid && i.out_len > 0 && spec.iface.map(|n| i.interface == Some(n)).unwrap_or(true))
                .collect();
            if cands.is_empty() {
                continue;
            }
            handled.insert((spec.vid, pid));
            let mut result = Query::Fail;
            for info in cands {
                if let Some(dev) = HidDev::open(&info.path) {
                    result = query(&dev, info, &spec.proto);
                    if !matches!(result, Query::Fail) {
                        break;
                    }
                }
            }
            let (battery, note) = match result {
                Query::Ok(b) => (Some(b), ""),
                Query::Offline => (None, "Apagado / desconectado"),
                Query::Fail => (None, "Sin datos"),
            };
            out.push(Device {
                key: format!("hs-{:04x}-{:04x}", spec.vid, pid),
                name: spec.name.to_string(),
                kind: "Cascos",
                conn: "Inalámbrico",
                battery,
                battery_note: note,
                led: None,
                driver: Driver::None,
            });
        }
    }
    out
}
