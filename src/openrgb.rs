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

#[derive(Clone, Debug)]
pub struct OrgbDevice {
    pub index: u32,
    pub name: String,
    pub kind: &'static str,
    pub leds: u16,
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
    let modes = c.u16()?;
    c.u32()?; // modo activo
    for _ in 0..modes {
        c.string()?;
        c.skip(4 * 8)?; // valor, flags, velocidad mín/máx, colores mín/máx, velocidad, dirección
        c.skip(4)?; // modo de color
        let n = c.u16()? as usize;
        c.skip(n * 4)?;
    }
    let zones = c.u16()?;
    for _ in 0..zones {
        c.string()?;
        c.skip(4 * 4)?; // tipo, leds mín/máx/cantidad
        let mlen = c.u16()? as usize;
        c.skip(mlen)?;
    }
    let leds = c.u16()?;
    Some(OrgbDevice { index, name, kind, leds })
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
    let mut s = connect().ok_or("OpenRGB no está abierto (activa su servidor SDK)")?;
    send(&mut s, index, SET_CUSTOM_MODE, &[]).ok_or("OpenRGB no respondió")?;
    let mut data = Vec::with_capacity(6 + leds as usize * 4);
    data.extend_from_slice(&(6 + leds as u32 * 4).to_le_bytes());
    data.extend_from_slice(&leds.to_le_bytes());
    for _ in 0..leds {
        data.extend_from_slice(&[rgb.0, rgb.1, rgb.2, 0]);
    }
    send(&mut s, index, UPDATE_LEDS, &data).ok_or("OpenRGB no respondió")?;
    Ok(())
}
