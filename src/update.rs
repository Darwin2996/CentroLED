//! Actualización automática desde las releases de GitHub (WinHTTP, sin dependencias).

use std::ptr::{null, null_mut};
use windows_sys::Win32::Networking::WinHttp::*;

pub const REPO: &str = "Darwin2996/CentroLED";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const EXE_NAME: &str = "CentroLED.exe";

pub struct Release {
    pub version: String,
    pub url: String,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// GET por HTTPS. Sigue las redirecciones (GitHub manda las descargas a otro servidor).
fn https_get(url: &str) -> Result<Vec<u8>, String> {
    let rest = url.strip_prefix("https://").ok_or("URL no válida")?;
    let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let path = if path.is_empty() { "/" } else { path };
    unsafe {
        let session = WinHttpOpen(wide("CentroLED-updater").as_ptr(), 4 /* proxy automático */, null(), null(), 0);
        if session.is_null() {
            return Err("No se pudo iniciar la conexión".into());
        }
        let close = |h: *mut core::ffi::c_void| {
            if !h.is_null() {
                WinHttpCloseHandle(h);
            }
        };
        let conn = WinHttpConnect(session, wide(host).as_ptr(), 443, 0);
        let req = if conn.is_null() {
            null_mut()
        } else {
            WinHttpOpenRequest(conn, wide("GET").as_ptr(), wide(path).as_ptr(), null(), null(), null(), 0x0080_0000 /* SECURE */)
        };
        let result = (|| {
            if req.is_null() {
                return Err("Sin conexión con GitHub".to_string());
            }
            let headers = wide("Accept: application/vnd.github+json\r\n");
            if WinHttpSendRequest(req, headers.as_ptr(), u32::MAX, null(), 0, 0, 0) == 0 || WinHttpReceiveResponse(req, null_mut()) == 0 {
                return Err("Sin conexión con GitHub".to_string());
            }
            let mut status: u32 = 0;
            let mut len = 4u32;
            WinHttpQueryHeaders(req, 19 | 0x2000_0000 /* STATUS_CODE | FLAG_NUMBER */, null(), &mut status as *mut u32 as _, &mut len, null_mut());
            if status != 200 {
                return Err(format!("GitHub respondió {status}"));
            }
            let mut out = Vec::new();
            loop {
                let mut avail = 0u32;
                if WinHttpQueryDataAvailable(req, &mut avail) == 0 {
                    return Err("La descarga se cortó".to_string());
                }
                if avail == 0 {
                    break;
                }
                let start = out.len();
                out.resize(start + avail as usize, 0);
                let mut read = 0u32;
                if WinHttpReadData(req, out[start..].as_mut_ptr() as _, avail, &mut read) == 0 {
                    return Err("La descarga se cortó".to_string());
                }
                out.truncate(start + read as usize);
            }
            Ok(out)
        })();
        close(req);
        close(conn);
        close(session);
        result
    }
}

fn parse_version(v: &str) -> (u32, u32, u32) {
    let mut it = v.trim_start_matches('v').split('.').map(|n| n.parse().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0), it.next().unwrap_or(0))
}

fn json_str<'a>(json: &'a str, key: &str, from: usize) -> Option<(&'a str, usize)> {
    let pat = format!("\"{key}\"");
    let k = from + json[from..].find(&pat)?;
    let colon = k + pat.len() + json[k + pat.len()..].find(':')?;
    let q1 = colon + json[colon..].find('"')? + 1;
    let q2 = q1 + json[q1..].find('"')?;
    Some((&json[q1..q2], q2))
}

/// Última versión publicada, si es más nueva que la que está en marcha.
pub fn check() -> Result<Option<Release>, String> {
    let body = https_get(&format!("https://api.github.com/repos/{REPO}/releases/latest"))?;
    let json = String::from_utf8_lossy(&body);
    let (tag, _) = json_str(&json, "tag_name", 0).ok_or("Respuesta de GitHub no válida")?;
    // Buscar el ejecutable entre los archivos de la release.
    let mut pos = 0;
    let mut url = None;
    while let Some((u, end)) = json_str(&json, "browser_download_url", pos) {
        if u.ends_with(EXE_NAME) {
            url = Some(u.to_string());
            break;
        }
        pos = end;
    }
    let url = url.ok_or("La release no incluye el ejecutable")?;
    if parse_version(tag) > parse_version(VERSION) {
        Ok(Some(Release { version: tag.trim_start_matches('v').to_string(), url }))
    } else {
        Ok(None)
    }
}

/// Descarga la versión nueva y la pone en lugar del ejecutable actual
/// (Windows permite renombrar un .exe en uso; el antiguo se borra en el siguiente arranque).
pub fn install(rel: &Release) -> Result<std::path::PathBuf, String> {
    let data = https_get(&rel.url)?;
    if data.len() < 50_000 || !data.starts_with(b"MZ") {
        return Err("El archivo descargado no es válido".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("Carpeta no válida")?;
    let new = dir.join("CentroLED.new.exe");
    let old = dir.join("CentroLED.old.exe");
    std::fs::write(&new, &data).map_err(|e| format!("No se pudo guardar la descarga: {e}"))?;
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("No se pudo reemplazar el programa: {e}"))?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe); // deshacer
        return Err(format!("No se pudo reemplazar el programa: {e}"));
    }
    Ok(exe)
}

/// Borra la copia antigua que deja una actualización.
pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let _ = std::fs::remove_file(dir.join("CentroLED.old.exe"));
            let _ = std::fs::remove_file(dir.join("CentroLED.new.exe"));
        }
    }
}
