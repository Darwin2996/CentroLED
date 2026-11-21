//! Historial de batería (30 días, un .csv por dispositivo) y estimación del tiempo restante.

use super::*;
use std::io::Write as _;

#[derive(Clone, Copy)]
pub struct Rec {
    pub t: u64,
    pub pct: u8,
    pub charging: bool,
}

const KEEP: u64 = 30 * 86_400;
/// Sin cambios, se guarda una lectura cada 10 minutos.
const EVERY: u64 = 600;
/// Más de 45 minutos sin lecturas = el dispositivo estaba apagado o dormido (se corta la línea).
const GAP: u64 = 45 * 60;

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn file(key: &str) -> std::path::PathBuf {
    let name: String = key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c.to_ascii_lowercase() } else { '_' }).collect();
    config_path().with_file_name("bateria").join(format!("{name}.csv"))
}

/// Historial de un dispositivo (se lee del disco la primera vez y se recorta a 30 días).
pub fn get(key: &str) -> &'static Vec<Rec> {
    let a = app();
    if !a.hist.contains_key(key) {
        let path = file(key);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let limit = now().saturating_sub(KEEP);
        let mut old = false;
        let list: Vec<Rec> = text
            .lines()
            .filter_map(|l| {
                let mut f = l.split(',');
                let r = Rec { t: f.next()?.parse().ok()?, pct: f.next()?.parse().ok()?, charging: f.next()? == "1" };
                if r.t < limit {
                    old = true;
                    return None;
                }
                Some(r)
            })
            .collect();
        if old {
            let body: String = list.iter().map(|r| format!("{},{},{}\n", r.t, r.pct, r.charging as u8)).collect();
            let _ = std::fs::write(&path, body);
        }
        a.hist.insert(key.to_string(), list);
    }
    &a.hist[key]
}

/// Apunta una lectura si cambió o si pasaron 10 minutos desde la última.
pub fn record(key: &str, pct: u8, charging: bool) {
    let t = now();
    let last = get(key).last().copied();
    if let Some(l) = last {
        if l.pct == pct && l.charging == charging && t < l.t + EVERY {
            return;
        }
    }
    let r = Rec { t, pct, charging };
    app().hist.get_mut(key).unwrap().push(r);
    let path = file(key);
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{},{},{}", r.t, r.pct, r.charging as u8);
    }
}

/// Tiempo estimado (segundos) hasta vaciarse, o hasta llenarse si está cargando.
/// Necesita al menos 30 min de datos y una variación del 3 % en el tramo actual.
pub fn estimate(d: &Device) -> Option<(bool, u64)> {
    let b = d.battery?;
    if b.approx {
        return None;
    }
    let list = get(&d.key);
    let last = list.last()?;
    if last.charging != b.charging || now() > last.t + GAP {
        return None;
    }
    // Tramo actual: lecturas seguidas con el mismo estado de carga (hasta 3 días atrás).
    let since = last.t.saturating_sub(3 * 86_400);
    let first = list.iter().rev().take_while(|r| r.charging == b.charging && r.t >= since).last()?;
    let span = last.t.saturating_sub(first.t);
    let delta = if b.charging { last.pct as i32 - first.pct as i32 } else { first.pct as i32 - last.pct as i32 };
    if span < 1800 || delta < 3 {
        return None;
    }
    let rate = delta as f64 / span as f64; // % por segundo
    let left = if b.charging { 100 - b.pct.min(100) } else { b.pct } as f64;
    if b.charging && left <= 0.0 {
        return None;
    }
    Some((b.charging, (left / rate) as u64))
}

pub fn fmt_dur(secs: u64) -> String {
    let min = (secs + 59) / 60;
    if min < 60 {
        format!("{min} min")
    } else if min < 48 * 60 {
        let (h, m) = (min / 60, min % 60 / 10 * 10);
        if m == 0 || h >= 10 { format!("{h} h") } else { format!("{h} h {m} min") }
    } else {
        format!("{} días", (min + 12 * 60) / (24 * 60))
    }
}

/// Texto corto para las tarjetas: "≈ 6 h" o "⚡ llena en 40 min".
pub fn estimate_text(d: &Device) -> Option<String> {
    let (charging, secs) = estimate(d)?;
    Some(if charging { format!("⚡ llena en {}", fmt_dur(secs)) } else { format!("≈ {}", fmt_dur(secs)) })
}

/// Gráfica del historial: últimas 24 h (`days` = 1) o 7 días.
pub fn paint_graph(p: &Painter, key: &str, x: i32, y: i32, w: i32, h: i32, days: u64) {
    let a = app();
    let list = get(key);
    let range = days * 86_400;
    let end = now();
    let start = end - range;
    let gx = x + 44;
    let gw = w - 52;
    // Rejilla 0 / 50 / 100 %
    for (pct, label) in [(100, "100 %"), (50, "50 %"), (0, "0 %")] {
        let ly = y + h - h * pct / 100;
        p.fill(&p.rect(gx, ly, gw, 1), LINE, 0);
        p.text(label, &p.rect(x, ly - 9, 40, 18), SUB, a.fonts.small, DT_SINGLELINE | DT_VCENTER | DT_RIGHT);
    }
    let labels: Vec<(f32, String)> = if days == 1 {
        vec![(0.0, "hace 24 h".into()), (0.5, "hace 12 h".into()), (1.0, "ahora".into())]
    } else {
        (0..=7).map(|i| (i as f32 / 7.0, if i == 7 { "hoy".to_string() } else { format!("-{} d", 7 - i) })).collect()
    };
    for (f, label) in labels {
        let lx = gx + (gw as f32 * f) as i32;
        p.text(&label, &p.rect(lx - 40, y + h + 6, 80, 18), SUB, a.fonts.small, DT_SINGLELINE | DT_CENTER);
    }
    let pts: Vec<&Rec> = list.iter().filter(|r| r.t + GAP >= start).collect();
    if pts.is_empty() {
        p.text(
            "Todavía no hay datos: la batería se va apuntando mientras Centro LED está abierto (también en segundo plano).",
            &p.rect(gx + 20, y + h / 2 - 30, gw - 40, 60),
            SUB,
            a.fonts.body,
            DT_CENTER | DT_WORDBREAK,
        );
        return;
    }
    let px = |t: u64| s(gx) + ((t.max(start) - start) as f64 / range as f64 * s(gw) as f64) as i32;
    let py = |pct: u8| s(y + h) - (s(h) as f64 * pct.min(100) as f64 / 100.0) as i32;
    unsafe {
        let normal = CreatePen(PS_SOLID, s(2).max(1), ACCENT);
        let charge = CreatePen(PS_SOLID, s(2).max(1), AMBER);
        let old = SelectObject(p.dc, normal);
        for wnd in pts.windows(2) {
            let (r0, r1) = (wnd[0], wnd[1]);
            if r1.t > r0.t + GAP {
                continue;
            }
            SelectObject(p.dc, if r1.charging { charge } else { normal });
            MoveToEx(p.dc, px(r0.t), py(r0.pct), null_mut());
            LineTo(p.dc, px(r1.t), py(r1.pct));
        }
        // Punto de la última lectura
        let l = pts[pts.len() - 1];
        SelectObject(p.dc, old);
        DeleteObject(normal);
        DeleteObject(charge);
        let (lx, ly) = (px(l.t), py(l.pct));
        let r = s(4);
        let dot = RECT { left: lx - r, top: ly - r, right: lx + r, bottom: ly + r };
        p.circle(&dot, if l.charging { AMBER } else { ACCENT }, None);
    }
    p.text("━ usando   ", &p.rect(x + w - 190, y - 26, 100, 18), ACCENT, a.fonts.small, DT_SINGLELINE | DT_RIGHT);
    p.text("━ cargando", &p.rect(x + w - 90, y - 26, 90, 18), AMBER, a.fonts.small, DT_SINGLELINE | DT_RIGHT);
}

/// Historial de ejemplo en memoria para las capturas de prueba (`--render ... --hist <clave>`).
pub fn demo(key: &str) {
    let end = now();
    let mut list = Vec::new();
    let mut pct = 100.0f32;
    let mut charging = false;
    let mut t = end - 7 * 86_400;
    while t <= end {
        if !charging && pct <= 12.0 {
            charging = true;
        } else if charging && pct >= 100.0 {
            charging = false;
        }
        pct = if charging { (pct + 2.5).min(100.0) } else { (pct - 0.18).max(0.0) };
        // Por la noche el dispositivo duerme: sin lecturas.
        let hour = (t / 3600) % 24;
        if !(2..8).contains(&hour) {
            list.push(Rec { t, pct: pct as u8, charging });
        }
        t += 600;
    }
    app().hist.insert(key.to_string(), list);
}
