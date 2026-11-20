use std::path::{Path, PathBuf};

fn main() {
    // Controles con el estilo visual de Windows 11 (Common Controls v6), sin necesidad de rc.exe.
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bins=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
    );
    println!("cargo:rerun-if-changed=build.rs");

    // Icono de la app: se dibuja aquí y se incrusta como recurso si está el compilador de recursos.
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let ico = out.join("centroled.ico");
    std::fs::write(&ico, make_ico(&[16, 20, 24, 32, 40, 48, 64, 128])).unwrap();
    let Some(rc) = find_rc() else {
        println!("cargo:warning=No se encontró rc.exe: el ejecutable no tendrá icono incrustado");
        return;
    };
    let rc_file = out.join("app.rc");
    std::fs::write(&rc_file, format!("1 ICON \"{}\"\n", ico.display().to_string().replace('\\', "\\\\"))).unwrap();
    let res = out.join("app.res");
    let ok = std::process::Command::new(rc).arg("/nologo").arg("/fo").arg(&res).arg(&rc_file).status().map(|s| s.success()).unwrap_or(false);
    if ok {
        println!("cargo:rustc-link-arg-bins={}", res.display());
    }
}

fn find_rc() -> Option<PathBuf> {
    let base = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(base).ok()?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    versions.sort();
    versions.into_iter().rev().map(|v| v.join("x64").join("rc.exe")).find(|p| p.exists())
}

/// Píxel del icono (RGBA) en coordenadas normalizadas 0..1: cuadrado violeta redondeado
/// con un LED blanco y su halo.
fn shade(x: f32, y: f32) -> [f32; 4] {
    let mut c = [0.0f32; 4];
    let mut over = |col: [f32; 3], a: f32| {
        let a = a.clamp(0.0, 1.0);
        for i in 0..3 {
            c[i] = c[i] * (1.0 - a) + col[i] * a;
        }
        c[3] = c[3] + a * (1.0 - c[3]);
    };
    // Fondo: rectángulo redondeado con degradado
    let r = 0.22;
    let (qx, qy) = ((x - 0.5).abs() - (0.5 - r), (y - 0.5).abs() - (0.5 - r));
    let d = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r;
    let top = [0.63, 0.48, 1.0];
    let bottom = [0.43, 0.25, 0.88];
    let g: [f32; 3] = [0, 1, 2].map(|i| top[i] + (bottom[i] - top[i]) * y);
    over(g, if d < 0.0 { 1.0 } else { 0.0 });
    let (cx, cy) = (0.5, 0.46);
    let dist = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    // Halo y anillo
    over([1.0, 1.0, 1.0], (0.30 - dist).max(0.0) / 0.30 * 0.35);
    over([1.0, 1.0, 1.0], if (dist - 0.27).abs() < 0.022 { 0.55 } else { 0.0 });
    // LED
    over([1.0, 1.0, 1.0], if dist < 0.16 { 1.0 } else { 0.0 });
    // Base del LED
    let base = (x - 0.5).abs() < 0.085 && (0.66..0.77).contains(&y);
    over([1.0, 1.0, 1.0], if base { 0.95 } else { 0.0 });
    c
}

fn render(size: u32) -> Vec<u8> {
    // Supermuestreo 4x4 para bordes suaves. Filas de abajo a arriba, formato BGRA.
    let ss = 4;
    let mut px = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let mut acc = [0.0f32; 4];
            for sy in 0..ss {
                for sx in 0..ss {
                    let fx = (x as f32 + (sx as f32 + 0.5) / ss as f32) / size as f32;
                    let fy = (y as f32 + (sy as f32 + 0.5) / ss as f32) / size as f32;
                    let c = shade(fx, fy);
                    for i in 0..3 {
                        acc[i] += c[i] * c[3];
                    }
                    acc[3] += c[3];
                }
            }
            let n = (ss * ss) as f32;
            let a = acc[3] / n;
            let col: [f32; 3] = if acc[3] > 0.0 { [acc[0] / acc[3], acc[1] / acc[3], acc[2] / acc[3]] } else { [0.0; 3] };
            let row = size - 1 - y;
            let o = ((row * size + x) * 4) as usize;
            px[o] = (col[2] * 255.0) as u8;
            px[o + 1] = (col[1] * 255.0) as u8;
            px[o + 2] = (col[0] * 255.0) as u8;
            px[o + 3] = (a * 255.0) as u8;
        }
    }
    px
}

fn make_ico(sizes: &[u32]) -> Vec<u8> {
    let mut images = Vec::new();
    for &s in sizes {
        let mut img = Vec::new();
        img.extend_from_slice(&40u32.to_le_bytes());
        img.extend_from_slice(&(s as i32).to_le_bytes());
        img.extend_from_slice(&((s * 2) as i32).to_le_bytes());
        img.extend_from_slice(&1u16.to_le_bytes());
        img.extend_from_slice(&32u16.to_le_bytes());
        img.extend_from_slice(&[0u8; 24]);
        img.extend_from_slice(&render(s));
        let mask_row = ((s + 31) / 32 * 4) as usize;
        img.extend(std::iter::repeat(0u8).take(mask_row * s as usize));
        images.push((s, img));
    }
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (s, img) in &images {
        out.push(if *s >= 256 { 0 } else { *s as u8 });
        out.push(if *s >= 256 { 0 } else { *s as u8 });
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(img.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for (_, img) in images {
        out.extend_from_slice(&img);
    }
    out
}
