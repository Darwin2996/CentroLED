//! Puente con OpenRGB (servidor SDK en localhost:6742): permite controlar placas base, RAM,
//! gráficas, ventiladores, tiras LED y miles de periféricos que OpenRGB ya soporta.
//! Si OpenRGB no está abierto con su servidor activado, simplemente no aparece nada.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

const PORT: u16 = 6742;
const REQUEST_CONTROLLER_COUNT: u32 = 0;
const REQUEST_CONTROLLER_DATA: u32 = 1;
const SET_CLIENT_NAME: u32 = 50;
const UPDATE_LEDS: u32 = 1050;
const SET_CUSTOM_MODE: u32 = 1100;
const UPDATE_MODE: u32 = 1101;

// Banderas y modos de color de un modo de OpenRGB.
const HAS_SPEED: u32 = 1 << 0;
const HAS_PER_LED_COLOR: u32 = 1 << 5;
const HAS_MODE_SPECIFIC_COLOR: u32 = 1 << 6;
const HAS_RANDOM_COLOR: u32 = 1 << 7;
const COLOR_PER_LED: u32 = 1;
const COLOR_SPECIFIC: u32 = 2;
const COLOR_RANDOM: u32 = 3;

/// Efecto de hardware tal como lo describe OpenRGB (se reenvía igual cambiando color y velocidad).
#[derive(Clone, Debug)]
pub struct OrgbMode {
    pub name: String,
    value: i32,
    pub flags: u32,
    speed_min: u32,
    speed_max: u32,
    colors_min: u32,
    colors_max: u32,
    direction: u32,
    color_mode: u32,
    n_colors: u16,
}

impl OrgbMode {
    pub fn has_speed(&self) -> bool {
        self.flags & HAS_SPEED != 0 && self.speed_min != self.speed_max
    }
    pub fn has_random(&self) -> bool {
        self.flags & HAS_RANDOM_COLOR != 0
    }
    /// Modo de control directo (cada LED con su color, no se graba en el dispositivo).
    pub fn is_direct(&self) -> bool {
        matches!(self.name.to_lowercase().as_str(), "direct" | "custom")
    }
}

#[derive(Clone, Debug)]
pub struct OrgbDevice {
    pub index: u32,
    pub name: String,
    pub kind: &'static str,
    pub leds: u16,
    pub modes: Vec<OrgbMode>,
    pub led_names: Vec<String>,
}

fn connect() -> Option<TcpStream> {
    let addr = SocketAddr::from(([127, 0, 0, 1], PORT));
    let s = TcpStream::connect_timeout(&addr, Duration::from_millis(150)).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(1500))).ok()?;
    s.set_write_timeout(Some(Duration::from_millis(1500))).ok()?;
    Some(s)
}

fn send(s: &mut TcpStream, dev: u32, id: u32, data: &[u8]) -> Option<()> {
    let mut pkt = Vec::with_capacity(16 + data.len());
    pkt.extend_from_slice(b"ORGB");
    pkt.extend_from_slice(&dev.to_le_bytes());
    pkt.extend_from_slice(&id.to_le_bytes());
    pkt.extend_from_slice(&(data.len() as u32).to_le_bytes());
    pkt.extend_from_slice(data);
    s.write_all(&pkt).ok()
}

fn recv(s: &mut TcpStream, id: u32) -> Option<Vec<u8>> {
    loop {
        let mut head = [0u8; 16];
        s.read_exact(&mut head).ok()?;
        if &head[..4] != b"ORGB" {
            return None;
        }
        let pid = u32::from_le_bytes(head[8..12].try_into().unwrap());
        let len = u32::from_le_bytes(head[12..16].try_into().unwrap()) as usize;
        if len > 16 * 1024 * 1024 {
            return None;
        }
        let mut data = vec![0u8; len];
        s.read_exact(&mut data).ok()?;
        if pid == id {
            return Some(data);
        }
    }
}

struct Cur<'a> {
    d: &'a [u8],
    p: usize,
}

impl Cur<'_> {
    fn u16(&mut self) -> Option<u16> {
        let v = u16::from_le_bytes(self.d.get(self.p..self.p + 2)?.try_into().ok()?);
        self.p += 2;
        Some(v)
    }
    fn u32(&mut self) -> Option<u32> {
        let v = u32::from_le_bytes(self.d.get(self.p..self.p + 4)?.try_into().ok()?);
        self.p += 4;
        Some(v)
    }
    fn string(&mut self) -> Option<String> {
        let n = self.u16()? as usize;
        let b = self.d.get(self.p..self.p + n)?;
        self.p += n;
        Some(String::from_utf8_lossy(b).trim_end_matches('\0').to_string())
    }
    fn skip(&mut self, n: usize) -> Option<()> {
        (self.p + n <= self.d.len()).then(|| self.p += n)
    }
}

fn kind_name(t: u32) -> &'static str {
    match t {
        0 => "Placa base",
        1 => "Memoria RAM",
        2 => "Gráfica",
        3 => "Refrigeración",
        4 => "Tira LED",
        5 => "Teclado",
        6 => "Ratón",
        7 => "Alfombrilla",
        8 => "Cascos",
        9 => "Soporte",
        10 => "Mando",
        11 => "Luz",
        12 => "Altavoz",
        14 => "Almacenamiento",
        15 => "Caja",
        16 => "Micrófono",
        _ => "RGB",
    }
}

/// Datos del controlador en el formato del protocolo 0 (el más simple y compatible).
fn parse(index: u32, d: &[u8]) -> Option<OrgbDevice> {
    let mut c = Cur { d, p: 0 };
    c.u32()?; // tamaño
    let kind = kind_name(c.u32()?);
    let name = c.string()?;
    for _ in 0..4 {
        c.string()?; // descripción, versión, serie, ubicación
    }
    let n_modes = c.u16()?;
    c.u32()?; // modo activo
    let mut modes = Vec::new();
    for _ in 0..n_modes {
        let name = c.string()?;
        let value = c.u32()? as i32;
        let flags = c.u32()?;
        let (speed_min, speed_max, colors_min, colors_max) = (c.u32()?, c.u32()?, c.u32()?, c.u32()?);
        c.u32()?; // velocidad actual
        let (direction, color_mode) = (c.u32()?, c.u32()?);
        let n_colors = c.u16()?;
        c.skip(n_colors as usize * 4)?;
        modes.push(OrgbMode { name, value, flags, speed_min, speed_max, colors_min, colors_max, direction, color_mode, n_colors });
    }
    let zones = c.u16()?;
    for _ in 0..zones {
        c.string()?;
        c.skip(4 * 4)?; // tipo, leds mín/máx/cantidad
        let mlen = c.u16()? as usize;
        c.skip(mlen)?;
    }
    let leds = c.u16()?;
    let mut led_names = Vec::with_capacity(leds as usize);
    for _ in 0..leds {
        led_names.push(c.string().unwrap_or_default());
        if c.u32().is_none() {
            break;
        }
    }
    Some(OrgbDevice { index, name, kind, leds, modes, led_names })
}

/// Lista los dispositivos de OpenRGB (vacío si el servidor no está en marcha).
pub fn scan() -> Vec<OrgbDevice> {
    let Some(mut s) = connect() else { return Vec::new() };
    let mut out = Vec::new();
    let _ = send(&mut s, 0, SET_CLIENT_NAME, b"Centro LED\0");
    if send(&mut s, 0, REQUEST_CONTROLLER_COUNT, &[]).is_none() {
        return out;
    }
    let Some(count) = recv(&mut s, REQUEST_CONTROLLER_COUNT).and_then(|d| Some(u32::from_le_bytes(d.get(..4)?.try_into().ok()?))) else {
        return out;
    };
    for i in 0..count.min(64) {
        if send(&mut s, i, REQUEST_CONTROLLER_DATA, &0u32.to_le_bytes()).is_none() {
            break;
        }
        match recv(&mut s, REQUEST_CONTROLLER_DATA).and_then(|d| parse(i, &d)) {
            Some(dev) if dev.leds > 0 => out.push(dev),
            _ => {}
        }
    }
    out
}

/// Pone todos los LEDs de un dispositivo en un color (modo "directo"/personalizado de OpenRGB).
pub fn set_color(index: u32, leds: u16, rgb: (u8, u8, u8)) -> Result<(), String> {
    set_leds(index, &vec![[rgb.0, rgb.1, rgb.2]; leds as usize])
}

/// Da a cada LED su propio color en el modo directo (no se graba en la memoria del dispositivo).
pub fn set_leds(index: u32, colors: &[[u8; 3]]) -> Result<(), String> {
    let mut s = connect().ok_or("OpenRGB no está abierto (activa su servidor SDK)")?;
    send(&mut s, index, SET_CUSTOM_MODE, &[]).ok_or("OpenRGB no respondió")?;
    send(&mut s, index, UPDATE_LEDS, &leds_packet(colors)).ok_or("OpenRGB no respondió")?;
    Ok(())
}

fn leds_packet(colors: &[[u8; 3]]) -> Vec<u8> {
    let n = colors.len().min(u16::MAX as usize);
    let mut data = Vec::with_capacity(6 + n * 4);
    data.extend_from_slice(&(6 + n as u32 * 4).to_le_bytes());
    data.extend_from_slice(&(n as u16).to_le_bytes());
    for c in &colors[..n] {
        data.extend_from_slice(&[c[0], c[1], c[2], 0]);
    }
    data
}

/// Activa un efecto de hardware con el color y la velocidad elegidos (`speed` 0..100).
/// Sólo lo activa: no pide a OpenRGB que lo grabe en la memoria del dispositivo.
pub fn set_mode(index: u32, mode_idx: usize, m: &OrgbMode, leds: u16, rgb: (u8, u8, u8), speed: u8, random: bool) -> Result<(), String> {
    let (d, color_mode) = mode_packet(mode_idx, m, rgb, speed, random);
    let mut s = connect().ok_or("OpenRGB no está abierto (activa su servidor SDK)")?;
    send(&mut s, index, UPDATE_MODE, &d).ok_or("OpenRGB no respondió")?;
    if color_mode == COLOR_PER_LED {
        send(&mut s, index, UPDATE_LEDS, &leds_packet(&vec![[rgb.0, rgb.1, rgb.2]; leds as usize])).ok_or("OpenRGB no respondió")?;
    }
    Ok(())
}

/// Descripción del modo (protocolo 0) para UPDATE_MODE, y el modo de color elegido.
fn mode_packet(mode_idx: usize, m: &OrgbMode, rgb: (u8, u8, u8), speed: u8, random: bool) -> (Vec<u8>, u32) {
    let color_mode = if random && m.flags & HAS_RANDOM_COLOR != 0 {
        COLOR_RANDOM
    } else if m.flags & HAS_MODE_SPECIFIC_COLOR != 0 {
        COLOR_SPECIFIC
    } else if m.flags & HAS_PER_LED_COLOR != 0 {
        COLOR_PER_LED
    } else {
        m.color_mode
    };
    let n_colors = if color_mode == COLOR_SPECIFIC {
        let n = (m.n_colors as u32).max(m.colors_min).max(1);
        if m.colors_max > 0 { n.min(m.colors_max) } else { n }
    } else {
        m.n_colors as u32
    };
    let sp = if m.has_speed() {
        (m.speed_min as i64 + (m.speed_max as i64 - m.speed_min as i64) * speed as i64 / 100) as u32
    } else {
        m.speed_min
    };
    let mut d = Vec::new();
    d.extend_from_slice(&0u32.to_le_bytes()); // tamaño (se rellena al final)
    d.extend_from_slice(&(mode_idx as i32).to_le_bytes());
    let name = m.name.as_bytes();
    d.extend_from_slice(&(name.len() as u16 + 1).to_le_bytes());
    d.extend_from_slice(name);
    d.push(0);
    for v in [m.value as u32, m.flags, m.speed_min, m.speed_max, m.colors_min, m.colors_max, sp, m.direction, color_mode] {
        d.extend_from_slice(&v.to_le_bytes());
    }
    d.extend_from_slice(&(n_colors as u16).to_le_bytes());
    for _ in 0..n_colors {
        d.extend_from_slice(&[rgb.0, rgb.1, rgb.2, 0]);
    }
    let len = d.len() as u32;
    d[..4].copy_from_slice(&len.to_le_bytes());
    (d, color_mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lee el paquete como lo hace OpenRGB (RGBController::SetModeDescription, protocolo 0).
    #[test]
    fn mode_packet_matches_openrgb_layout() {
        let m = OrgbMode { name: "Breathing".into(), value: 7, flags: HAS_SPEED | HAS_MODE_SPECIFIC_COLOR | HAS_RANDOM_COLOR, speed_min: 0, speed_max: 4, colors_min: 1, colors_max: 2, direction: 0, color_mode: COLOR_RANDOM, n_colors: 0 };
        let (d, cm) = mode_packet(3, &m, (10, 20, 30), 50, false);
        assert_eq!(cm, COLOR_SPECIFIC);
        let mut c = Cur { d: &d, p: 0 };
        assert_eq!(c.u32().unwrap() as usize, d.len());
        assert_eq!(c.u32().unwrap(), 3);
        assert_eq!(c.string().unwrap(), "Breathing");
        let v: Vec<u32> = (0..9).map(|_| c.u32().unwrap()).collect();
        assert_eq!(v, vec![7, m.flags, 0, 4, 1, 2, 2, 0, COLOR_SPECIFIC]);
        assert_eq!(c.u16().unwrap(), 1);
        assert_eq!(&d[c.p..], &[10, 20, 30, 0]);
        // Arcoíris pedido y admitido: modo de color aleatorio, sin colores
        let (_, cm) = mode_packet(3, &m, (10, 20, 30), 100, true);
        assert_eq!(cm, COLOR_RANDOM);
        // Velocidad con rango invertido (algunos dispositivos van de lento=alto a rápido=bajo)
        let inv = OrgbMode { speed_min: 200, speed_max: 0, ..m };
        let (d, _) = mode_packet(0, &inv, (0, 0, 0), 25, false);
        let sp = u32::from_le_bytes(d[4 + 4 + 2 + 10 + 24..4 + 4 + 2 + 10 + 28].try_into().unwrap());
        assert_eq!(sp, 150);
    }
}
