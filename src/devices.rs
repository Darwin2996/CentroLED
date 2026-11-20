//! Detección de dispositivos, lectura de batería y control de luces.

use crate::hid::{self, HidDev, HidInfo};
use crate::kb_template;
use std::collections::HashSet;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub struct Battery {
    pub pct: u8,
    pub charging: bool,
    /// true cuando el dispositivo sólo da niveles aproximados (bajo/medio/lleno).
    pub approx: bool,
}

/// Modo especial "Apagado" común a todos los dispositivos.
pub const MODE_OFF: u8 = 0xFF;
/// Modo "ciclo suave": la app va fundiendo los colores elegidos (sólo mandos).
pub const MODE_CYCLE: u8 = 0xF0;
/// Respiración y arcoíris animados por la app (mandos).
pub const MODE_SW_BREATH: u8 = 0xF1;
pub const MODE_SW_RAINBOW: u8 = 0xF2;

/// ¿El modo lo anima la app (en vez del propio dispositivo)?
pub fn is_app_animated(mode: u8) -> bool {
    matches!(mode, MODE_CYCLE | MODE_SW_BREATH | MODE_SW_RAINBOW)
}

#[derive(Clone, Debug)]
pub struct LedCaps {
    pub modes: Vec<(&'static str, u8)>,
    pub speed: bool,
    pub multicolor: bool,
    /// Las luces se pierden al desconectar: se reaplican automáticamente.
    pub volatile: bool,
    /// Zonas con luz independiente (vacío = una sola zona).
    pub zones: Vec<String>,
    /// LEDs de jugador y de micrófono (DualSense).
    pub pad_extras: bool,
}

/// Luces secundarias del DualSense.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PadExtra {
    /// Bits 0..4: los 5 LEDs blancos de jugador (de izquierda a derecha).
    pub player: u8,
    /// Brillo de los LEDs de jugador: 0 alto, 1 medio, 2 bajo.
    pub player_bright: u8,
    /// LED del micrófono: 0 apagado, 1 encendido, 2 parpadeando.
    pub mic: u8,
}

impl Default for PadExtra {
    fn default() -> Self {
        PadExtra { player: 0b00100, player_bright: 1, mic: 0 }
    }
}

/// Ajustes adicionales según el tipo de dispositivo.
#[derive(Clone)]
pub enum Extra {
    Keyboard(KbExtra),
    Zones(Vec<LedSetting>),
    Pad(PadExtra),
}

/// Efectos de una zona Logitech: (id de efecto, índice en la lista del dispositivo).
#[derive(Clone, Debug)]
pub struct LogiZone {
    pub effects: Vec<(u16, u8)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LedSetting {
    pub mode: u8,
    pub rgb: (u8, u8, u8),
    pub bright: u8, // 0..100
    pub speed: u8,  // 0..100
    pub multi: bool,
}

#[derive(Clone, Debug)]
pub enum Driver {
    Tempest { path: Vec<u16> },
    DualSense { path: Vec<u16>, bt: bool },
    DualShock4 { path: Vec<u16>, bt: bool },
    Logitech { path: Vec<u16>, idx: u8, led_feat: u8, zones: Vec<LogiZone> },
    Switch { path: Vec<u16>, out_len: u16, home: bool },
    OpenRgb { index: u32, leds: u16 },
    None,
}

#[derive(Clone, Debug)]
pub struct Device {
    pub key: String,
    pub name: String,
    pub kind: &'static str,
    pub conn: &'static str,
    pub battery: Option<Battery>,
    /// Texto alternativo cuando no hay batería (p. ej. "Dormido").
    pub battery_note: &'static str,
    pub led: Option<LedCaps>,
    pub driver: Driver,
}

// ---------------------------------------------------------------- búsqueda

pub fn scan() -> Vec<Device> {
    let all = hid::enumerate();
    let mut out = Vec::new();
    let mut handled: HashSet<(u16, u16)> = HashSet::new();

    // Teclado Tempest Diablo (Sonix 0C45:8006)
    if let Some(i) = all.iter().find(|i| i.vid == 0x0C45 && i.pid == 0x8006 && i.interface == Some(0) && i.feat_len == 65) {
        handled.insert((i.vid, i.pid));
        out.push(Device {
            key: "tempest-diablo".into(),
            name: "Teclado Tempest Diablo".into(),
            kind: "Teclado",
            conn: "USB",
            battery: None,
            battery_note: "Con cable",
            led: Some(LedCaps { modes: TEMPEST_MODES.to_vec(), speed: true, multicolor: true, volatile: false, zones: vec![], pad_extras: false }),
            driver: Driver::Tempest { path: i.path.clone() },
        });
    }

    // Mandos PlayStation
    for i in all.iter().filter(|i| i.vid == 0x054C && i.usage_page == 1 && i.usage == 5) {
        let ds5 = matches!(i.pid, 0x0CE6 | 0x0DF2);
        let ds4 = matches!(i.pid, 0x05C4 | 0x09CC | 0x0BA0);
        if !ds5 && !ds4 {
            continue;
        }
        handled.insert((i.vid, i.pid));
        let bt = i.in_len != 64;
        let (battery, driver, name) = if ds5 {
            (dualsense_battery(i, bt), Driver::DualSense { path: i.path.clone(), bt }, if i.pid == 0x0DF2 { "Mando DualSense Edge" } else { "Mando DualSense (PS5)" })
        } else {
            (ds4_battery(i, bt), Driver::DualShock4 { path: i.path.clone(), bt }, "Mando DualShock 4 (PS4)")
        };
        let n = out.iter().filter(|d: &&Device| d.key.starts_with("ps-")).count();
        out.push(Device {
            key: format!("ps-{:04x}-{}", i.pid, n),
            name: name.into(),
            kind: "Mando",
            conn: if bt { "Bluetooth" } else { "USB" },
            battery,
            battery_note: "Sin datos",
            led: Some(LedCaps {
                modes: vec![
                    ("Estático", 1),
                    ("Respiración", MODE_SW_BREATH),
                    ("Ciclo suave", MODE_CYCLE),
                    ("Arcoíris", MODE_SW_RAINBOW),
                    ("Apagado", MODE_OFF),
                ],
                speed: true,
                multicolor: false,
                volatile: true,
                zones: vec![],
                pad_extras: ds5,
            }),
            driver,
        });
    }

    // Logitech HID++ (cascos, ratones y teclados inalámbricos)
    for i in all.iter().filter(|i| i.vid == 0x046D && (i.usage_page == 0xFF43 || i.usage_page == 0xFF00) && i.out_len == 20) {
        let found = logitech_probe(i);
        if !found.is_empty() {
            handled.insert((i.vid, i.pid));
        }
        out.extend(found);
    }

    // Ratones VXE / ATK (receptor "COMPX")
    for i in all.iter().filter(|i| i.vid == 0x3554 && i.usage_page == 0xFF02 && i.out_len == 17) {
        handled.insert((i.vid, i.pid));
        let (battery, note) = match compx_battery(i) {
            Ok(b) => (Some(b), ""),
            Err(e) => (None, e),
        };
        let name = if i.product.to_lowercase().contains("vxe") { "Ratón VXE" } else { "Ratón ATK" };
        out.push(Device {
            key: format!("compx-{:04x}", i.pid),
            name: name.into(),
            kind: "Ratón",
            conn: "Inalámbrico 2.4 GHz",
            battery,
            battery_note: note,
            led: None,
            driver: Driver::None,
        });
    }

    // Cascos de otras marcas y mandos de Nintendo
    out.extend(crate::headsets::scan(&all, &mut handled));
    out.extend(crate::nintendo::scan(&all, &mut handled));

    // Mandos Xbox (XInput) y dispositivos Bluetooth. Si un mando Xbox también publica su
    // batería por Bluetooth (valor exacto), se usa esa y no se muestra dos veces.
    let mut xbox = xinput_devices();
    let mut bt = bluetooth_batteries();
    for x in xbox.iter_mut().filter(|x| x.kind == "Mando" && x.battery.map(|b| b.approx).unwrap_or(true)) {
        if let Some(pos) = bt.iter().position(|b| b.kind == "Mando" && b.name.to_lowercase().contains("xbox")) {
            let b = bt.remove(pos);
            x.battery = b.battery;
            x.conn = "Bluetooth";
        }
    }
    out.extend(xbox);
    out.extend(bt);

    // Todo lo que controla OpenRGB (placas base, RAM, gráficas, ventiladores…), si está abierto.
    let native = ["dualsense", "dualshock", "g733", "tempest", "diablo"];
    let orgb = crate::openrgb::scan();
    if orgb.iter().any(|o| o.kind == "Placa base") {
        // La placa base ya la controla OpenRGB: no mostrar también la entrada informativa.
        handled.extend(all.iter().filter(|i| i.vid == 0x1462).map(|i| (i.vid, i.pid)));
    }
    for o in orgb {
        let l = o.name.to_lowercase();
        if native.iter().any(|n| l.contains(n)) {
            continue;
        }
        out.push(Device {
            key: format!("orgb-{}", o.name),
            name: o.name.clone(),
            kind: o.kind,
            conn: "OpenRGB",
            battery: None,
            battery_note: "—",
            led: Some(LedCaps { modes: vec![("Estático", 1), ("Apagado", MODE_OFF)], speed: false, multicolor: false, volatile: true, zones: vec![], pad_extras: false }),
            driver: Driver::OpenRgb { index: o.index, leds: o.leds },
        });
    }

    // Resto de dispositivos USB con nombre (sólo informativo)
    let mut seen: HashSet<(u16, u16)> = HashSet::new();
    for i in &all {
        if handled.contains(&(i.vid, i.pid)) || !seen.insert((i.vid, i.pid)) || i.vid == 0 {
            continue;
        }
        let (name, kind) = describe_other(i, &all);
        if name.starts_with("Dispositivo USB") {
            continue; // dispositivos virtuales sin nombre (p. ej. los de G HUB)
        }
        out.push(Device {
            key: format!("usb-{:04x}-{:04x}", i.vid, i.pid),
            name,
            kind,
            conn: "USB",
            battery: None,
            battery_note: "—",
            led: None,
            driver: Driver::None,
        });
    }
    out
}

fn describe_other(i: &HidInfo, all: &[HidInfo]) -> (String, &'static str) {
    let siblings: Vec<&HidInfo> = all.iter().filter(|o| o.vid == i.vid && o.pid == i.pid).collect();
    let has = |page: u16, usage: u16| siblings.iter().any(|o| o.usage_page == page && o.usage == usage);
    let kind = if i.vid == 0x1462 && i.product.to_uppercase().contains("MYSTIC") {
        "Placa base"
    } else if has(1, 6) {
        "Teclado"
    } else if has(1, 2) {
        "Ratón"
    } else if has(1, 4) || has(1, 5) {
        "Mando"
    } else {
        "Otro"
    };
    let product = siblings.iter().map(|o| o.product.as_str()).find(|p| !p.is_empty() && !p.eq_ignore_ascii_case("USB DEVICE")).unwrap_or("");
    let name = if i.vid == 0x1462 && kind == "Placa base" {
        "Placa base MSI (Mystic Light)".to_string()
    } else if product.is_empty() {
        format!("Dispositivo USB {:04X}:{:04X}", i.vid, i.pid)
    } else {
        product.to_string()
    };
    (name, kind)
}

// ---------------------------------------------------------------- aplicar luces

/// Datos extra del teclado Tempest: efecto del borde y color de cada tecla.
#[derive(Clone)]
pub struct KbExtra {
    pub rim: LedSetting,
    pub keys: Box<[[u8; 3]; KB_LEDS]>,
}

pub fn apply(dev: &Device, s: &LedSetting, extra: Option<&Extra>) -> Result<(), String> {
    match &dev.driver {
        Driver::Tempest { path } => match extra {
            // Dos escrituras: la primera fija el modo de las teclas y la segunda el del borde.
            Some(Extra::Keyboard(x)) => tempest_apply_full(path, s, Some(&x.keys)).and_then(|_| tempest_apply_full(path, &x.rim, Some(&x.keys))),
            _ => tempest_apply_full(path, s, None),
        },
        Driver::DualSense { .. } | Driver::DualShock4 { .. } => {
            let lb = Lightbar::open(dev)?;
            if let Some(Extra::Pad(px)) = extra {
                lb.set_extras(px);
            }
            if lb.set(scaled(s)) { Ok(()) } else { Err("El mando no aceptó el color".into()) }
        }
        Driver::Logitech { path, idx, led_feat, zones } => {
            let per_zone: Vec<LedSetting> = match extra {
                Some(Extra::Zones(z)) => (0..zones.len()).map(|i| z.get(i).copied().unwrap_or(*s)).collect(),
                _ => vec![*s; zones.len()],
            };
            logitech_apply(path, *idx, *led_feat, zones, &per_zone)
        }
        Driver::Switch { path, out_len, home } => match extra {
            Some(Extra::Pad(px)) => crate::nintendo::apply(path, *out_len, *home, px),
            _ => Ok(()),
        },
        Driver::OpenRgb { index, leds } => crate::openrgb::set_color(*index, *leds, scaled(s)),
        Driver::None => Err("Este dispositivo no tiene luces controlables".into()),
    }
}

fn scaled(s: &LedSetting) -> (u8, u8, u8) {
    if s.mode == MODE_OFF {
        return (0, 0, 0);
    }
    let f = |c: u8| ((c as u32 * s.bright as u32) / 100) as u8;
    (f(s.rgb.0), f(s.rgb.1), f(s.rgb.2))
}

// ---------------------------------------------------------------- Tempest Diablo

pub const TEMPEST_MODES: &[(&str, u8)] = &[
    ("Estático", 1),
    ("Personalizado", MODE_CUSTOM),
    ("Respiración", 7),
    ("Espectro", 8),
    ("Ola", 11),
    ("Ola (centro a lados)", 9),
    ("Colorido", 6),
    ("Reactivo", 2),
    ("Reactivo inverso", 3),
    ("Estrellas", 4),
    ("Lluvia", 5),
    ("Desplazamiento", 10),
    ("Rotación", 12),
    ("Onda expansiva", 13),
    ("Explosión de ondas", 14),
    ("Ondas", 15),
    ("Flujo", 16),
    ("Pulsación", 17),
    ("Inclinación", 18),
    ("Haces alternos", 19),
    ("Apagado", MODE_OFF),
];

/// Modos del borde (tienen su propio modo activo, independiente de las teclas).
pub const RIM_MODES: &[(&str, u8)] = &[
    ("Estático", 24),
    ("Respiración", 23),
    ("Ola", 20),
    ("Aurora", 21),
    ("Espectro", 22),
    ("Apagado", 25),
];

fn kb_set(dev: &HidDev, data: &[u8]) -> bool {
    dev.set_feature(data)
}

fn kb_get(dev: &HidDev, buf: &mut [u8]) -> bool {
    dev.get_feature(buf)
}

/// Envía un comando y, si `ack`, espera la confirmación "04 cmd 00 01" (el teclado
/// contesta "00" mientras procesa; el software oficial reintenta igual).
fn kb_cmd(dev: &HidDev, cmd: &[u8], ack: bool) -> Result<(), String> {
    kb_cmd_resp(dev, cmd, ack).map(|_| ())
}

/// Igual que kb_cmd pero devuelve la respuesta; espera hasta ~1,5 s antes de reenviar.
fn kb_cmd_resp(dev: &HidDev, cmd: &[u8], ack: bool) -> Result<[u8; 65], String> {
    let mut out = [0u8; 65];
    out[1..1 + cmd.len()].copy_from_slice(cmd);
    for _attempt in 0..3 {
        if !kb_set(dev, &out) {
            return Err(format!("El teclado rechazó el comando {:02X} {:02X}", cmd[0], cmd[1]));
        }
        if !ack {
            return Ok([0u8; 65]);
        }
        for _ in 0..150 {
            std::thread::sleep(Duration::from_millis(10));
            let mut inp = [0u8; 65];
            if kb_get(dev, &mut inp) && inp[1] == cmd[0] && inp[2] == cmd[1] && inp[4] == 1 {
                return Ok(inp);
            }
        }
    }
    Err(format!("El teclado no confirmó el comando {:02X} {:02X}", cmd[0], cmd[1]))
}

/// Posiciones de LED del teclado: 8 filas × 18 columnas (índice = fila * 18 + columna).
pub const KB_LEDS: usize = 144;
/// Modo "personalizado": cada LED con su propio color.
pub const MODE_CUSTOM: u8 = 0x80;

/// Rellena la entrada del modo en la tabla (7 primeros paquetes, 16 bytes por modo) y la devuelve.
fn tempest_entry(packets: &mut [[u8; 64]; 18], s: &LedSetting) -> Result<[u8; 16], String> {
    let (mode, bright) = if s.mode == MODE_OFF { (1u8, 0u8) } else { (s.mode, ((s.bright as u32 * 16 + 50) / 100) as u8) };
    let speed = ((s.speed as u32 * 16 + 50) / 100) as u8;
    for p in 0..7 {
        for e in 0..4 {
            let off = e * 16;
            if packets[p][off] == mode {
                let ent = &mut packets[p][off..off + 16];
                if (20..=25).contains(&mode) {
                    // El borde guarda el color en orden R, B, G.
                    ent[1] = s.rgb.0;
                    ent[2] = s.rgb.2;
                    ent[3] = s.rgb.1;
                } else if mode != MODE_CUSTOM {
                    ent[1] = s.rgb.0;
                    ent[2] = s.rgb.1;
                    ent[3] = s.rgb.2;
                }
                if mode != MODE_CUSTOM {
                    ent[8] = s.multi as u8;
                    ent[10] = speed;
                }
                ent[9] = bright;
                let mut entry = [0u8; 16];
                entry.copy_from_slice(ent);
                return Ok(entry);
            }
        }
    }
    Err("Modo no válido para el teclado".into())
}

/// Escribe el perfil completo con `s` como modo activo (de teclas o de borde) y los colores por tecla.
pub fn tempest_apply_full(path: &[u16], s: &LedSetting, keys: Option<&[[u8; 3]; KB_LEDS]>) -> Result<(), String> {
    let dev = HidDev::open_feature(path).ok_or("No se pudo abrir el teclado")?;
    let mut packets = kb_template::PACKETS;
    if let Some(keys) = keys {
        // Paquetes 9..17: 4 bytes por LED (0x80 + R G B).
        for (i, c) in keys.iter().enumerate() {
            let p = &mut packets[8 + i / 16][(i % 16) * 4..(i % 16) * 4 + 4];
            p.copy_from_slice(&[0x80, c[0], c[1], c[2]]);
        }
    }
    // El último paquete indica el modo que se activa.
    let entry = tempest_entry(&mut packets, s)?;
    packets[17] = [0u8; 64];
    packets[17][..16].copy_from_slice(&entry);

    // Si el teclado pierde algún paquete, la suma no coincide: repetir la escritura completa.
    let mut last = Err(String::new());
    for _ in 0..3 {
        last = tempest_write(&dev, &packets);
        if last.is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    last
}

fn tempest_write(dev: &HidDev, packets: &[[u8; 64]; 18]) -> Result<(), String> {
    // Saludo (el teclado responde a un desafío; no hace falta validarlo).
    kb_cmd(&dev, &[0x04, 0xAB], true)?;
    let mut challenge = [0u8; 65];
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let mut x = (now.as_nanos() as u32 ^ 0x9E37_79B9 ^ std::process::id()) | 1;
    for b in challenge[1..].iter_mut() {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *b = x as u8;
    }
    kb_set(dev, &challenge);
    kb_cmd(dev, &[0x04, 0x02], true)?;
    // Inicio de escritura: 0x12 = 18 paquetes. El teclado borra su memoria tras confirmarlo.
    kb_cmd(dev, &[0x04, 0x13, 0, 0, 0, 0, 0, 0, 0x12], true)?;
    std::thread::sleep(Duration::from_millis(50));
    for (n, p) in packets.iter().enumerate() {
        let mut out = [0u8; 65];
        out[1..].copy_from_slice(p);
        // El teclado graba cada paquete en su memoria: darle tiempo y reintentar si está ocupado.
        let mut sent = false;
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(12));
            if kb_set(dev, &out) {
                sent = true;
                break;
            }
        }
        if !sent {
            return Err(format!("Error enviando el paquete {} de 18 al teclado", n + 1));
        }
    }
    // Dejar que termine de grabar el último paquete antes de pedir la verificación.
    std::thread::sleep(Duration::from_millis(60));
    // El teclado responde con la suma de todos los bytes recibidos y 01 si la escritura es válida.
    let expected: u32 = packets.iter().flatten().map(|&b| b as u32).sum::<u32>() & 0xFF_FFFF;
    let r = kb_cmd_resp(dev, &[0x04, 0x02], true)?;
    let got = u32::from_le_bytes([r[5], r[6], r[7], 0]);
    if got != expected {
        return Err(format!("El teclado rechazó los datos (suma {:06X}, esperada {:06X})", got, expected));
    }
    kb_cmd(dev, &[0x04, 0xF0], false)?;
    Ok(())
}

// ---------------------------------------------------------------- PlayStation

fn crc32(seed: u8, data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in std::iter::once(&seed).chain(data.iter()) {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn read_report(dev: &HidDev, ids: &[u8], len: usize) -> Option<Vec<u8>> {
    let deadline = Instant::now() + Duration::from_millis(600);
    let mut buf = vec![0u8; len.max(64)];
    while Instant::now() < deadline {
        if let Some(n) = dev.read(&mut buf, 200) {
            if n > 0 && ids.contains(&buf[0]) {
                buf.truncate(n);
                return Some(buf);
            }
        }
    }
    None
}

fn dualsense_battery(i: &HidInfo, bt: bool) -> Option<Battery> {
    let dev = HidDev::open(&i.path)?;
    if bt {
        // Leer la calibración activa el modo de informes completos (0x31) por Bluetooth.
        let mut f = [0u8; 41];
        f[0] = 0x05;
        if let Some(fd) = HidDev::open_feature(&i.path) { fd.get_feature(&mut f); }
    }
    let r = read_report(&dev, if bt { &[0x31] } else { &[0x01] }, i.in_len as usize)?;
    let st = *r.get(if bt { 54 } else { 53 })?;
    let level = st & 0x0F;
    match st >> 4 {
        0 => Some(Battery { pct: (level * 10 + 5).min(100), charging: false, approx: false }),
        1 => Some(Battery { pct: (level * 10 + 5).min(100), charging: true, approx: false }),
        2 => Some(Battery { pct: 100, charging: false, approx: false }),
        _ => None,
    }
}

/// Barra de luz de un mando PlayStation abierta para cambiar el color al instante
/// (no se guarda en el mando, así que se puede animar sin desgaste).
pub struct Lightbar {
    dev: HidDev,
    bt: bool,
    ds5: bool,
}

impl Lightbar {
    pub fn open(d: &Device) -> Result<Lightbar, String> {
        let (path, bt, ds5) = match &d.driver {
            Driver::DualSense { path, bt } => (path, *bt, true),
            Driver::DualShock4 { path, bt } => (path, *bt, false),
            _ => return Err("No es un mando PlayStation".into()),
        };
        let dev = HidDev::open(path).ok_or("No se pudo abrir el mando (¿lo oculta HidHide/DS4Windows?)")?;
        let lb = Lightbar { dev, bt, ds5 };
        if ds5 {
            // Apagar la animación azul por defecto para poder poner nuestro color.
            if !lb.dev.write(&lb.ds5_report(None)) {
                return Err("El mando no aceptó el comando".into());
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        Ok(lb)
    }

    fn ds5_report(&self, rgb: Option<(u8, u8, u8)>) -> Vec<u8> {
        let mut buf = vec![0u8; if self.bt { 78 } else { 48 }];
        let c = if self.bt {
            buf[0] = 0x31;
            buf[2] = 0x10;
            3
        } else {
            buf[0] = 0x02;
            1
        };
        match rgb {
            None => {
                buf[c + 38] = 0x02; // valid_flag2: control de la configuración de la barra
                buf[c + 41] = 0x02; // apagar la animación por defecto
            }
            Some((r, g, b)) => {
                buf[c + 1] = 0x04; // valid_flag1: color de la barra de luz
                buf[c + 44] = r;
                buf[c + 45] = g;
                buf[c + 46] = b;
            }
        }
        if self.bt {
            let crc = crc32(0xA2, &buf[..74]);
            buf[74..78].copy_from_slice(&crc.to_le_bytes());
        }
        buf
    }

    /// LEDs de jugador, su brillo y el LED del micrófono (sólo DualSense).
    pub fn set_extras(&self, x: &PadExtra) -> bool {
        if !self.ds5 {
            return true;
        }
        let mut buf = vec![0u8; if self.bt { 78 } else { 48 }];
        let c = if self.bt {
            buf[0] = 0x31;
            buf[2] = 0x10;
            3
        } else {
            buf[0] = 0x02;
            1
        };
        buf[c + 1] = 0x01 | 0x10; // valid_flag1: LED de micrófono + LEDs de jugador
        buf[c + 38] = 0x01; // valid_flag2: brillo de los LEDs
        buf[c + 8] = x.mic.min(2);
        buf[c + 42] = x.player_bright.min(2);
        buf[c + 43] = x.player & 0x1F;
        if self.bt {
            let crc = crc32(0xA2, &buf[..74]);
            buf[74..78].copy_from_slice(&crc.to_le_bytes());
        }
        self.dev.write(&buf)
    }

    pub fn set(&self, (r, g, b): (u8, u8, u8)) -> bool {
        let buf = if self.ds5 {
            self.ds5_report(Some((r, g, b)))
        } else if self.bt {
            let mut buf = vec![0u8; 78];
            buf[0] = 0x11;
            buf[1] = 0xC0;
            buf[3] = 0x02;
            buf[8] = r;
            buf[9] = g;
            buf[10] = b;
            let crc = crc32(0xA2, &buf[..74]);
            buf[74..78].copy_from_slice(&crc.to_le_bytes());
            buf
        } else {
            let mut buf = vec![0u8; 32];
            buf[0] = 0x05;
            buf[1] = 0x02;
            buf[6] = r;
            buf[7] = g;
            buf[8] = b;
            buf
        };
        self.dev.write(&buf)
    }
}

fn ds4_battery(i: &HidInfo, bt: bool) -> Option<Battery> {
    let dev = HidDev::open(&i.path)?;
    if bt {
        let mut f = [0u8; 37];
        f[0] = 0x02;
        if let Some(fd) = HidDev::open_feature(&i.path) { fd.get_feature(&mut f); }
    }
    let r = read_report(&dev, if bt { &[0x11] } else { &[0x01] }, i.in_len as usize)?;
    let st = *r.get(if bt { 32 } else { 30 })?;
    let level = st & 0x0F;
    let cable = st & 0x10 != 0;
    Some(if cable && level >= 10 {
        Battery { pct: 100, charging: false, approx: false }
    } else {
        Battery { pct: (level * 10 + 5).min(100), charging: cable, approx: false }
    })
}

// ---------------------------------------------------------------- Logitech HID++ 2.0

const SW_ID: u8 = 0x0A;

fn hidpp(dev: &HidDev, idx: u8, feat: u8, func: u8, params: &[u8]) -> Option<[u8; 16]> {
    let mut req = [0u8; 20];
    req[0] = 0x11;
    req[1] = idx;
    req[2] = feat;
    req[3] = (func << 4) | SW_ID;
    req[4..4 + params.len()].copy_from_slice(params);
    if !dev.write(&req) {
        return None;
    }
    let deadline = Instant::now() + Duration::from_millis(500);
    let mut buf = [0u8; 20];
    while Instant::now() < deadline {
        if dev.read(&mut buf, 250).is_none() {
            continue;
        }
        if buf[0] != 0x11 || buf[1] != idx {
            continue;
        }
        if buf[2] == 0xFF && buf[3] == feat {
            return None; // error HID++ 2.0
        }
        if buf[2] == feat && buf[3] == req[3] {
            let mut p = [0u8; 16];
            p.copy_from_slice(&buf[4..20]);
            return Some(p);
        }
    }
    None
}

fn feature_index(dev: &HidDev, idx: u8, feature: u16) -> Option<u8> {
    let r = hidpp(dev, idx, 0x00, 0, &feature.to_be_bytes())?;
    (r[0] != 0).then_some(r[0])
}

fn logitech_name(dev: &HidDev, idx: u8) -> Option<(String, u8)> {
    let f = feature_index(dev, idx, 0x0005)?;
    let len = hidpp(dev, idx, f, 0, &[])?[0] as usize;
    let mut name = Vec::new();
    while name.len() < len {
        let chunk = hidpp(dev, idx, f, 1, &[name.len() as u8])?;
        let take = (len - name.len()).min(16);
        name.extend_from_slice(&chunk[..take]);
    }
    let kind = hidpp(dev, idx, f, 2, &[]).map(|r| r[0]).unwrap_or(0xFF);
    Some((String::from_utf8_lossy(&name).trim().to_string(), kind))
}

const VOLT_PCT: [(u16, u8); 8] = [(4100, 100), (3950, 80), (3850, 60), (3750, 40), (3650, 20), (3500, 10), (3300, 5), (3150, 0)];

fn volt_to_pct(mv: u16) -> u8 {
    if mv >= VOLT_PCT[0].0 {
        return 100;
    }
    for w in VOLT_PCT.windows(2) {
        let ((v1, p1), (v2, p2)) = (w[0], w[1]);
        if mv >= v2 {
            return p2 + ((mv - v2) as u32 * (p1 - p2) as u32 / (v1 - v2) as u32) as u8;
        }
    }
    0
}

fn logitech_battery(dev: &HidDev, idx: u8) -> Option<Battery> {
    if let Some(f) = feature_index(dev, idx, 0x1004) {
        let r = hidpp(dev, idx, f, 1, &[])?;
        return Some(Battery { pct: r[0].min(100), charging: matches!(r[2], 1 | 2), approx: false });
    }
    if let Some(f) = feature_index(dev, idx, 0x1000) {
        let r = hidpp(dev, idx, f, 0, &[])?;
        return Some(Battery { pct: r[0].min(100), charging: matches!(r[2], 1 | 2 | 4), approx: false });
    }
    if let Some(f) = feature_index(dev, idx, 0x1F20) {
        let r = hidpp(dev, idx, f, 0, &[])?;
        let mv = u16::from_be_bytes([r[0], r[1]]);
        if mv == 0 {
            return None;
        }
        return Some(Battery { pct: volt_to_pct(mv), charging: r[2] == 0x03, approx: false });
    }
    None
}

fn logitech_probe(i: &HidInfo) -> Vec<Device> {
    let mut out = Vec::new();
    let Some(dev) = HidDev::open(&i.path) else { return out };
    // 0xFF = conectado directamente (cascos); 1..6 = dispositivos detrás de un receptor.
    let indexes: &[u8] = if i.usage_page == 0xFF43 { &[0xFF] } else { &[0xFF, 1, 2, 3, 4, 5, 6] };
    for &idx in indexes {
        // Comprobar que responde HID++ 2.0 (función ping del feature raíz).
        if hidpp(&dev, idx, 0x00, 1, &[0, 0, 0xAA]).is_none() {
            continue;
        }
        let (mut name, dtype) = logitech_name(&dev, idx).unwrap_or_default();
        if name.is_empty() {
            name = i.product.clone();
        }
        if !name.to_lowercase().starts_with("logitech") {
            name = format!("Logitech {name}");
        }
        let lname = name.to_lowercase();
        let kind = if lname.contains("headset") || i.usage_page == 0xFF43 {
            "Cascos"
        } else {
            match dtype {
                0 => "Teclado",
                3 | 4 | 5 => "Ratón",
                _ => "Otro",
            }
        };
        let battery = logitech_battery(&dev, idx);
        let mut led = None;
        let mut driver = Driver::None;
        if let Some(f) = feature_index(&dev, idx, 0x8070) {
            let (zones, names) = logitech_zones(&dev, idx, f);
            // Efectos comunes a todas las zonas, en un orden fijo.
            let mut modes: Vec<(&'static str, u8)> = Vec::new();
            for &(id, name) in LOGI_EFFECTS {
                if zones.iter().all(|z| z.effects.iter().any(|e| e.0 == id)) {
                    modes.push((name, if id == 0 { MODE_OFF } else { id as u8 }));
                }
            }
            if !modes.iter().any(|m| m.1 == MODE_OFF) {
                modes.push(("Apagado", MODE_OFF));
            }
            led = Some(LedCaps {
                modes,
                speed: true,
                multicolor: false,
                volatile: true,
                zones: if zones.len() > 1 { names } else { vec![] },
                pad_extras: false,
            });
            driver = Driver::Logitech { path: i.path.clone(), idx, led_feat: f, zones };
        }
        out.push(Device {
            key: format!("logi-{:04x}-{:02x}", i.pid, idx),
            name,
            kind,
            conn: if i.usage_page == 0xFF43 { "Inalámbrico 2.4 GHz" } else { "Receptor Logitech" },
            battery,
            battery_note: "Apagado / sin datos",
            led,
            driver,
        });
    }
    out
}

/// Efectos de iluminación Logitech (feature 0x8070) que sabemos configurar.
const LOGI_EFFECTS: &[(u16, &str)] = &[
    (0x01, "Estático"),
    (0x0A, "Respiración"),
    (0x02, "Pulso"),
    (0x03, "Ciclo de color"),
    (0x0B, "Ondas"),
    (0x0E, "Descomposición"),
    (0x00, "Apagado"),
];

fn zone_name(location: u16, n: usize) -> String {
    match location {
        0x01 => "Principal".into(),
        0x02 => "Logo".into(),
        0x03 => "Lado izquierdo".into(),
        0x04 => "Lado derecho".into(),
        0x05 => "Combinada".into(),
        _ => format!("Zona {}", n + 1),
    }
}

/// Lee las zonas de luz y los efectos que admite cada una. Si el dispositivo no
/// responde a la consulta detallada, usa la distribución conocida (apagado/estático/respiración).
fn logitech_zones(dev: &HidDev, idx: u8, f: u8) -> (Vec<LogiZone>, Vec<String>) {
    let count = hidpp(dev, idx, f, 0, &[]).map(|r| r[0]).unwrap_or(1).clamp(1, 8);
    let mut zones = Vec::new();
    let mut names = Vec::new();
    for z in 0..count {
        let mut effects = Vec::new();
        let mut location = 0;
        if let Some(info) = hidpp(dev, idx, f, 1, &[z]) {
            location = u16::from_be_bytes([info[1], info[2]]);
            for e in 0..info[3].min(16) {
                if let Some(r) = hidpp(dev, idx, f, 2, &[z, e]) {
                    effects.push((u16::from_be_bytes([r[2], r[3]]), e));
                }
            }
        }
        if effects.is_empty() {
            effects = vec![(0x00, 0), (0x01, 1), (0x0A, 2)];
        }
        names.push(zone_name(location, z as usize));
        zones.push(LogiZone { effects });
    }
    // Nombres repetidos (p. ej. dos "Principal"): numerarlos.
    for i in 0..names.len() {
        if names.iter().filter(|n| **n == names[i]).count() > 1 {
            names[i] = format!("Zona {}", i + 1);
        }
    }
    (zones, names)
}

fn logitech_apply(path: &[u16], idx: u8, feat: u8, zones: &[LogiZone], per_zone: &[LedSetting]) -> Result<(), String> {
    let dev = HidDev::open(path).ok_or("No se pudo abrir el dispositivo Logitech")?;
    for (z, zone) in zones.iter().enumerate() {
        let s = per_zone[z];
        let id: u16 = if s.mode == MODE_OFF { 0 } else { s.mode as u16 };
        let Some(&(_, index)) = zone.effects.iter().find(|e| e.0 == id) else {
            return Err("Efecto no disponible en esta zona".into());
        };
        let full = scaled(&LedSetting { bright: 100, ..s });
        let dim = scaled(&s);
        let intensity = s.bright.max(1);
        // Periodo: de 8 s (lento) a 1,5 s (rápido)
        let period = ((8000 - s.speed as u32 * 65) as u16).to_be_bytes();
        let mut p = [0u8; 13];
        p[0] = z as u8;
        p[1] = index;
        let prm = &mut p[2..12];
        match id {
            0x01 => prm[..3].copy_from_slice(&[dim.0, dim.1, dim.2]),
            0x0A => {
                prm[..3].copy_from_slice(&[full.0, full.1, full.2]);
                prm[3..5].copy_from_slice(&period);
                prm[6] = intensity;
            }
            0x02 => {
                prm[..3].copy_from_slice(&[full.0, full.1, full.2]);
                prm[3] = (s.speed as u32 * 255 / 100) as u8;
            }
            0x03 => {
                prm[5..7].copy_from_slice(&period);
                prm[7] = intensity;
            }
            0x0B => {
                prm[..3].copy_from_slice(&[full.0, full.1, full.2]);
                prm[4..6].copy_from_slice(&period);
            }
            0x0E => {
                prm[6..8].copy_from_slice(&period);
                prm[8] = intensity;
            }
            _ => {}
        }
        if hidpp(&dev, idx, feat, 3, &p).is_none() {
            return Err("Los cascos no aceptaron el efecto (¿están encendidos?)".into());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- VXE / ATK (COMPX)

/// Mensaje del protocolo COMPX: report 8, [cmd, estado, dirección(2), longitud, datos(10), checksum].
fn compx_cmd(dev: &HidDev, cmd: u8, data: &[u8], wait_ms: u64) -> Option<[u8; 17]> {
    let mut req = [0u8; 17];
    req[0] = 0x08;
    req[1] = cmd;
    req[5] = data.len() as u8;
    req[6..6 + data.len()].copy_from_slice(data);
    let sum: u32 = req.iter().take(16).map(|&b| b as u32).sum();
    req[16] = 0x55u8.wrapping_sub(sum as u8);
    if !dev.write(&req) {
        return None;
    }
    let deadline = Instant::now() + Duration::from_millis(wait_ms);
    let mut buf = [0u8; 17];
    while Instant::now() < deadline {
        if dev.read(&mut buf, 250).is_some() && buf[0] == 0x08 && buf[1] == cmd {
            return (buf[2] == 0).then_some(buf);
        }
    }
    None
}

fn compx_battery(i: &HidInfo) -> Result<Battery, &'static str> {
    let dev = HidDev::open(&i.path).ok_or("Sin acceso")?;
    // ¿Hay un ratón enlazado y encendido?
    let online = compx_cmd(&dev, 0x03, &[], 500).ok_or("Sin respuesta")?;
    if online[6] == 0 {
        return Err("Apagado / dormido");
    }
    // Saludo que hace el software oficial antes de leer la batería.
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u32;
    compx_cmd(&dev, 0x01, &now.to_le_bytes(), 500);
    let r = compx_cmd(&dev, 0x04, &[], 1500).ok_or("Sin respuesta (mueve el ratón)")?;
    let pct = r[6];
    if pct == 0 || pct > 100 {
        return Err("Sin datos");
    }
    Ok(Battery { pct, charging: r[7] != 0, approx: false })
}

// ---------------------------------------------------------------- Xbox (XInput)

/// Modelo de un mando Xbox a partir de su identificador de producto.
fn xbox_model(vid: u16, pid: u16) -> &'static str {
    if vid != 0x045E {
        return "Mando compatible Xbox";
    }
    match pid {
        0x0B12 | 0x0B13 | 0x0B20 | 0x0B21 => "Mando Xbox Series X|S",
        0x0B00 | 0x0B05 | 0x0B22 => "Mando Xbox Elite Series 2",
        0x02E3 => "Mando Xbox Elite",
        0x02EA | 0x02FD | 0x0B0A => "Mando Xbox One S",
        0x02D1 | 0x02DD | 0x02E0 => "Mando Xbox One",
        0x0B0C => "Mando adaptativo Xbox",
        0x028E | 0x028F | 0x0719 => "Mando Xbox 360",
        _ => "Mando Xbox",
    }
}

/// XInputGetCapabilitiesEx (función no documentada, ordinal 108 de xinput1_4) para saber el modelo.
fn xinput_ids(n: u32) -> Option<(u16, u16)> {
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    #[repr(C)]
    struct CapsEx {
        caps: [u8; 20],
        vid: u16,
        pid: u16,
        rev: u16,
        _pad: u16,
        _id: u32,
    }
    type F = unsafe extern "system" fn(u32, u32, u32, *mut CapsEx) -> u32;
    unsafe {
        let name: Vec<u16> = "xinput1_4.dll\0".encode_utf16().collect();
        let lib = LoadLibraryW(name.as_ptr());
        if lib.is_null() {
            return None;
        }
        let f: F = std::mem::transmute(GetProcAddress(lib, 108 as _)?);
        let mut c: CapsEx = std::mem::zeroed();
        (f(1, n, 0, &mut c) == 0).then_some((c.vid, c.pid))
    }
}

fn xinput_devices() -> Vec<Device> {
    use windows_sys::Win32::UI::Input::XboxController::*;
    let mut out = Vec::new();
    for n in 0..4u32 {
        unsafe {
            let mut st: XINPUT_STATE = std::mem::zeroed();
            if XInputGetState(n, &mut st) != 0 {
                continue;
            }
            let mut bi: XINPUT_BATTERY_INFORMATION = std::mem::zeroed();
            XInputGetBatteryInformation(n, BATTERY_DEVTYPE_GAMEPAD, &mut bi);
            let wired = bi.BatteryType == BATTERY_TYPE_WIRED;
            let battery = if matches!(bi.BatteryType, BATTERY_TYPE_ALKALINE | BATTERY_TYPE_NIMH) {
                let pct = match bi.BatteryLevel {
                    BATTERY_LEVEL_EMPTY => 5,
                    BATTERY_LEVEL_LOW => 25,
                    BATTERY_LEVEL_MEDIUM => 60,
                    _ => 100,
                };
                Some(Battery { pct, charging: false, approx: true })
            } else {
                None
            };
            let model = xinput_ids(n).map(|(v, p)| xbox_model(v, p)).unwrap_or("Mando Xbox");
            // Cascos conectados al mando (inalámbricos Xbox)
            let mut hb: XINPUT_BATTERY_INFORMATION = std::mem::zeroed();
            if XInputGetBatteryInformation(n, BATTERY_DEVTYPE_HEADSET, &mut hb) == 0 && matches!(hb.BatteryType, BATTERY_TYPE_ALKALINE | BATTERY_TYPE_NIMH) {
                let pct = match hb.BatteryLevel {
                    BATTERY_LEVEL_EMPTY => 5,
                    BATTERY_LEVEL_LOW => 25,
                    BATTERY_LEVEL_MEDIUM => 60,
                    _ => 100,
                };
                out.push(Device {
                    key: format!("xinput-hs-{n}"),
                    name: "Cascos Xbox".into(),
                    kind: "Cascos",
                    conn: "Inalámbrico Xbox",
                    battery: Some(Battery { pct, charging: false, approx: true }),
                    battery_note: "",
                    led: None,
                    driver: Driver::None,
                });
            }
            out.push(Device {
                key: format!("xinput-{n}"),
                name: if n == 0 { model.to_string() } else { format!("{} ({})", model, n + 1) },
                kind: "Mando",
                conn: if wired { "USB" } else { "Inalámbrico" },
                battery,
                battery_note: if wired { "Con cable" } else { "Sin datos" },
                led: None,
                driver: Driver::None,
            });
        }
    }
    out
}

// ---------------------------------------------------------------- Bluetooth (batería que publica Windows)

fn bluetooth_batteries() -> Vec<Device> {
    use windows_sys::core::GUID;
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
    use windows_sys::Win32::Devices::Properties::DEVPROPKEY;
    let key = DEVPROPKEY { fmtid: GUID::from_u128(0x104EA319_6EE2_4701_BD47_8DDBF425BBE5), pid: 2 };
    let mut out: Vec<Device> = Vec::new();
    unsafe {
        let set = SetupDiGetClassDevsW(std::ptr::null(), std::ptr::null(), std::ptr::null_mut(), DIGCF_ALLCLASSES | DIGCF_PRESENT);
        if set as isize == -1 {
            return out;
        }
        let mut i = 0;
        loop {
            let mut data: SP_DEVINFO_DATA = std::mem::zeroed();
            data.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
            if SetupDiEnumDeviceInfo(set, i, &mut data) == 0 {
                break;
            }
            i += 1;
            let mut ty = 0u32;
            let mut val = 0u8;
            if SetupDiGetDevicePropertyW(set, &data, &key, &mut ty, &mut val, 1, std::ptr::null_mut(), 0) == 0 || val > 100 {
                continue;
            }
            let mut name = [0u16; 128];
            if SetupDiGetDeviceRegistryPropertyW(set, &data, SPDRP_FRIENDLYNAME, std::ptr::null_mut(), name.as_mut_ptr() as _, 256, std::ptr::null_mut()) == 0 {
                SetupDiGetDeviceRegistryPropertyW(set, &data, SPDRP_DEVICEDESC, std::ptr::null_mut(), name.as_mut_ptr() as _, 256, std::ptr::null_mut());
            }
            let end = name.iter().position(|&c| c == 0).unwrap_or(0);
            let mut n = String::from_utf16_lossy(&name[..end]);
            // Windows crea varias entradas por perfil (manos libres, estéreo...): unificar.
            for suffix in [" Hands-Free AG", " Hands-Free", " Avrcp Transport", " Stereo"] {
                if let Some(p) = n.find(suffix) {
                    n.truncate(p);
                }
            }
            if n.is_empty() || out.iter().any(|d| d.name == n) {
                continue;
            }
            // Tipo: primero la clase que asigna Windows al dispositivo, después el nombre.
            let mut class = [0u16; 64];
            SetupDiGetDeviceRegistryPropertyW(set, &data, SPDRP_CLASS, std::ptr::null_mut(), class.as_mut_ptr() as _, 128, std::ptr::null_mut());
            let cend = class.iter().position(|&c| c == 0).unwrap_or(0);
            let class = String::from_utf16_lossy(&class[..cend]).to_lowercase();
            let l = n.to_lowercase();
            let has = |words: &[&str]| words.iter().any(|k| l.contains(k));
            let kind = if class == "audioendpoint" || class == "media" || has(&["buds", "head", "auricular", "casco", "airpods", "wh-", "wf-", "earphone", "speaker", "altavoz"]) {
                "Cascos"
            } else if class == "mouse" || has(&["mouse", "ratón", "raton", "mx ", "trackball"]) {
                "Ratón"
            } else if class == "keyboard" || has(&["keyboard", "teclado", "keys"]) {
                "Teclado"
            } else if has(&["controller", "mando", "gamepad", "dualsense", "dualshock", "joy-con", "xbox", "8bitdo"]) {
                "Mando"
            } else if has(&["pen", "lápiz", "stylus"]) {
                "Lápiz"
            } else if has(&["watch", "reloj", "band"]) {
                "Reloj"
            } else if has(&["phone", "móvil", "galaxy", "iphone", "pixel"]) {
                "Móvil"
            } else {
                "Bluetooth"
            };
            out.push(Device {
                key: format!("bt-{n}"),
                name: n,
                kind,
                conn: "Bluetooth",
                battery: Some(Battery { pct: val, charging: false, approx: false }),
                battery_note: "",
                led: None,
                driver: Driver::None,
            });
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    out
}
