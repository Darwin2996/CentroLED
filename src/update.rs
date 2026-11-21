//! Actualización automática desde las releases de GitHub (WinHTTP, sin dependencias).

use std::ptr::{null, null_mut};
use windows_sys::Win32::Networking::WinHttp::*;
use windows_sys::Win32::Security::Cryptography::*;

pub const REPO: &str = "Darwin2996/CentroLED";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const EXE_NAME: &str = "CentroLED.exe";
const SUMS_NAME: &str = "SHA256SUMS.txt";

/// Clave pública ECDSA P-256 (X ‖ Y) con la que se firman las releases. La privada sólo está
/// en el PC del autor (tools/firmar-release.ps1); sin ella nadie puede publicar una versión
/// que estas copias acepten.
const PUBLIC_KEY: &str = "51c6224326fe98916d7b83fa94c89207b22bc59f33e71ecf13c2306e7165847162f78325bda5adf883400d559bccb18ee59c2d8f6fde72e8992d7c2d4d57407d";

pub struct Release {
    pub version: String,
    pub url: String,
    /// Lista de SHA-256 de los archivos de la release y su firma.
    pub sums_url: Option<String>,
    pub sig_url: Option<String>,
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
    let rel = latest()?;
    Ok((parse_version(&rel.version) > parse_version(VERSION)).then_some(rel))
}

/// Última versión publicada en GitHub (sea o no más nueva).
pub fn latest() -> Result<Release, String> {
    let body = https_get(&format!("https://api.github.com/repos/{REPO}/releases/latest"))?;
    let json = String::from_utf8_lossy(&body);
    let (tag, _) = json_str(&json, "tag_name", 0).ok_or("Respuesta de GitHub no válida")?;
    // Buscar el ejecutable, la lista de sumas y su firma entre los archivos de la release.
    let mut pos = 0;
    let (mut url, mut sums_url, mut sig_url) = (None, None, None);
    while let Some((u, end)) = json_str(&json, "browser_download_url", pos) {
        if u.ends_with(&format!("/{EXE_NAME}")) {
            url = Some(u.to_string());
        } else if u.ends_with(&format!("/{SUMS_NAME}")) {
            sums_url = Some(u.to_string());
        } else if u.ends_with(&format!("/{SUMS_NAME}.sig")) {
            sig_url = Some(u.to_string());
        }
        pos = end;
    }
    let url = url.ok_or("La release no incluye el ejecutable")?;
    Ok(Release { version: tag.trim_start_matches('v').to_string(), url, sums_url, sig_url })
}

fn hex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len() / 2).map(|i| u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()).collect()
}

fn ok(st: i32) -> bool {
    st >= 0
}

/// SHA-256 con la criptografía de Windows (CNG).
pub fn sha256(data: &[u8]) -> Option<[u8; 32]> {
    unsafe {
        let mut alg: BCRYPT_ALG_HANDLE = null_mut();
        if !ok(BCryptOpenAlgorithmProvider(&mut alg, wide("SHA256").as_ptr(), null(), 0)) {
            return None;
        }
        let mut out = [0u8; 32];
        let r = BCryptHash(alg, null(), 0, data.as_ptr(), data.len() as u32, out.as_mut_ptr(), 32);
        BCryptCloseAlgorithmProvider(alg, 0);
        ok(r).then_some(out)
    }
}

/// Comprueba la firma ECDSA P-256 (r ‖ s, 64 bytes) de `msg` con la clave pública de la app.
fn signature_ok(msg: &[u8], sig: &[u8]) -> bool {
    let (Some(key), Some(hash)) = (hex(PUBLIC_KEY), sha256(msg)) else { return false };
    if sig.len() != 64 || key.len() != 64 {
        return false;
    }
    // BCRYPT_ECCKEY_BLOB: magia "ECS1" (pública P-256) + longitud de cada coordenada.
    let mut blob = Vec::with_capacity(72);
    blob.extend_from_slice(&0x3153_4345u32.to_le_bytes());
    blob.extend_from_slice(&32u32.to_le_bytes());
    blob.extend_from_slice(&key);
    unsafe {
        let mut alg: BCRYPT_ALG_HANDLE = null_mut();
        if !ok(BCryptOpenAlgorithmProvider(&mut alg, wide("ECDSA_P256").as_ptr(), null(), 0)) {
            return false;
        }
        let mut k: BCRYPT_KEY_HANDLE = null_mut();
        let mut good = false;
        if ok(BCryptImportKeyPair(alg, null_mut(), wide("ECCPUBLICBLOB").as_ptr(), &mut k, blob.as_ptr(), blob.len() as u32, 0)) {
            good = ok(BCryptVerifySignature(k, null(), hash.as_ptr(), 32, sig.as_ptr(), 64, 0));
            BCryptDestroyKey(k);
        }
        BCryptCloseAlgorithmProvider(alg, 0);
        good
    }
}

/// Verifica que `data` es el archivo `name` publicado por el autor: la lista de sumas debe
/// estar firmada con su clave y contener el SHA-256 exacto de `data`.
pub fn verify(data: &[u8], name: &str, sums: &[u8], sig_hex: &[u8]) -> Result<(), String> {
    let sig = hex(&String::from_utf8_lossy(sig_hex)).ok_or("La firma de la versión no es válida")?;
    if !signature_ok(sums, &sig) {
        return Err("La firma de la versión no coincide: no se instala por seguridad".into());
    }
    let text = String::from_utf8_lossy(sums);
    let want = text
        .lines()
        .find_map(|l| {
            let (h, n) = l.split_once(char::is_whitespace)?;
            (n.trim().trim_start_matches('*') == name).then(|| h.to_lowercase())
        })
        .ok_or("La versión publicada no incluye la suma de este archivo")?;
    let got: String = sha256(data).ok_or("No se pudo calcular la suma")?.iter().map(|b| format!("{b:02x}")).collect();
    if got != want {
        return Err("El archivo descargado está alterado o incompleto: no se instala".into());
    }
    Ok(())
}

/// Descarga el ejecutable de la release y comprueba su firma antes de devolverlo.
pub fn fetch_verified(rel: &Release) -> Result<Vec<u8>, String> {
    let (Some(sums_url), Some(sig_url)) = (&rel.sums_url, &rel.sig_url) else {
        return Err("La versión publicada no está firmada: no se instala por seguridad".into());
    };
    let sums = https_get(sums_url)?;
    let sig = https_get(sig_url)?;
    let data = https_get(&rel.url)?;
    verify(&data, EXE_NAME, &sums, &sig)?;
    Ok(data)
}

/// Descarga la versión nueva y la pone en lugar del ejecutable actual
/// (Windows permite renombrar un .exe en uso; el antiguo se borra en el siguiente arranque).
pub fn install(rel: &Release) -> Result<std::path::PathBuf, String> {
    let data = fetch_verified(rel)?;
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
