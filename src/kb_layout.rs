// Distribución del Tempest Diablo (del programa oficial). Índice de LED = fila * 18 + columna.
// (índice, etiqueta, x, y, ancho, alto) en unidades del dibujo original.
pub const KEYS: &[(usize, &str, i32, i32, i32, i32)] = &[
    (1, "Esc", 0, 0, 24, 18),
    (2, "F1", 58, 0, 24, 18),
    (3, "F2", 87, 0, 24, 18),
    (4, "F3", 116, 0, 24, 18),
    (5, "F4", 145, 0, 24, 18),
    (6, "F5", 188, 0, 24, 18),
    (7, "F6", 217, 0, 24, 18),
    (8, "F7", 246, 0, 24, 18),
    (9, "F8", 275, 0, 24, 18),
    (10, "F9", 319, 0, 24, 18),
    (11, "F10", 348, 0, 24, 18),
    (12, "F11", 377, 0, 24, 18),
    (13, "F12", 406, 0, 24, 18),
    (19, "º", 0, 30, 24, 22),
    (20, "1", 29, 30, 24, 22),
    (21, "2", 58, 30, 24, 22),
    (22, "3", 87, 30, 24, 22),
    (23, "4", 116, 30, 24, 22),
    (24, "5", 145, 30, 24, 22),
    (25, "6", 174, 30, 24, 22),
    (26, "7", 203, 30, 24, 22),
    (27, "8", 232, 30, 24, 22),
    (28, "9", 261, 30, 24, 22),
    (29, "0", 290, 30, 24, 22),
    (30, "'", 319, 30, 24, 22),
    (31, "¡", 348, 30, 24, 22),
    (32, "Bloq\nNum", 538, 30, 24, 22),
    (33, "/", 567, 30, 24, 22),
    (34, "*", 596, 30, 24, 22),
    (37, "Tab", 0, 58, 35, 22),
    (38, "Q", 44, 58, 24, 22),
    (39, "W", 73, 58, 24, 22),
    (40, "E", 102, 58, 24, 22),
    (41, "R", 131, 58, 24, 22),
    (42, "T", 160, 58, 24, 22),
    (43, "Y", 189, 58, 24, 22),
    (44, "U", 218, 58, 24, 22),
    (45, "I", 247, 58, 24, 22),
    (46, "O", 276, 58, 24, 22),
    (47, "P", 305, 58, 24, 22),
    (48, "`", 334, 58, 24, 22),
    (49, "+", 363, 58, 24, 22),
    (50, "7", 538, 58, 24, 22),
    (51, "8", 567, 58, 24, 22),
    (52, "9", 596, 58, 24, 22),
    (55, "Bloq\nMayús", 0, 88, 43, 22),
    (56, "A", 51, 88, 24, 22),
    (57, "S", 80, 88, 24, 22),
    (58, "D", 109, 88, 24, 22),
    (59, "F", 138, 88, 24, 22),
    (60, "G", 167, 88, 24, 22),
    (61, "H", 196, 88, 24, 22),
    (62, "J", 225, 88, 24, 22),
    (63, "K", 254, 88, 24, 22),
    (64, "L", 283, 88, 24, 22),
    (65, "Ñ", 312, 88, 24, 22),
    (66, "´", 341, 88, 24, 22),
    (67, "Ç", 392, 58, 37, 22),
    (68, "4", 538, 88, 24, 22),
    (69, "5", 567, 88, 24, 22),
    (70, "6", 596, 88, 24, 22),
    (73, "Shift", 0, 118, 58, 22),
    (74, "Z", 66, 118, 24, 22),
    (75, "X", 95, 118, 24, 22),
    (76, "C", 124, 118, 24, 22),
    (77, "V", 153, 118, 24, 22),
    (78, "B", 182, 118, 24, 22),
    (79, "N", 211, 118, 24, 22),
    (80, "M", 240, 118, 24, 22),
    (81, ",", 269, 118, 24, 22),
    (82, ".", 298, 118, 24, 22),
    (83, "-", 327, 118, 24, 22),
    (84, "Shift", 356, 118, 75, 22),
    (85, "Intro", 370, 88, 59, 22),
    (86, "1", 538, 118, 24, 22),
    (87, "2", 567, 118, 24, 22),
    (88, "3", 596, 118, 24, 22),
    (91, "Ctrl", 0, 145, 31, 22),
    (92, "Win", 36, 145, 31, 22),
    (93, "Alt", 72, 145, 31, 22),
    (94, "", 108, 145, 175, 22),
    (95, "Alt", 291, 145, 31, 22),
    (96, "FN", 327, 145, 31, 22),
    (97, "Menú", 363, 145, 31, 22),
    (98, "Ctrl", 399, 145, 31, 22),
    (99, "←", 443, 145, 24, 22),
    (100, "↓", 472, 145, 24, 22),
    (101, "↑", 472, 118, 24, 22),
    (102, "→", 501, 145, 24, 22),
    (103, "⌫", 375, 30, 56, 22),
    (104, "0", 538, 145, 52, 22),
    (105, ".", 596, 145, 24, 22),
    (106, "Intro", 625, 118, 24, 51),
    (112, "Impr", 441, 0, 26, 18),
    (113, "Bloq\nDesp", 472, 0, 24, 18),
    (115, "Pausa", 501, 0, 24, 18),
    (116, "Insert", 443, 30, 24, 22),
    (117, "Inicio", 472, 30, 24, 22),
    (118, "RePág", 501, 30, 24, 22),
    (119, "Supr", 443, 58, 24, 22),
    (120, "Fin", 472, 58, 24, 22),
    (121, "AvPág", 501, 58, 24, 22),
    (122, "-", 625, 30, 24, 22),
    (123, "+", 625, 58, 24, 51),
];

// ---------------------------------------------------------------- teclas de otros teclados
//
// Para los teclados genéricos (OpenRGB y Dynamic Lighting) cada LED se identifica por la tecla
// que ilumina (código de uso HID del teclado). Esta tabla lleva cada código a su tecla del
// dibujo, de modo que el mismo editor tecla a tecla sirve para cualquier teclado.

/// (índice de LED del dibujo, código de uso HID de la página de teclado 0x07).
const USAGES: &[(u8, u8)] = &[
    (1, 0x29), (2, 0x3A), (3, 0x3B), (4, 0x3C), (5, 0x3D), (6, 0x3E), (7, 0x3F), (8, 0x40),
    (9, 0x41), (10, 0x42), (11, 0x43), (12, 0x44), (13, 0x45),
    (19, 0x35), (20, 0x1E), (21, 0x1F), (22, 0x20), (23, 0x21), (24, 0x22), (25, 0x23), (26, 0x24),
    (27, 0x25), (28, 0x26), (29, 0x27), (30, 0x2D), (31, 0x2E), (32, 0x53), (33, 0x54), (34, 0x55),
    (37, 0x2B), (38, 0x14), (39, 0x1A), (40, 0x08), (41, 0x15), (42, 0x17), (43, 0x1C), (44, 0x18),
    (45, 0x0C), (46, 0x12), (47, 0x13), (48, 0x2F), (49, 0x30), (50, 0x5F), (51, 0x60), (52, 0x61),
    (55, 0x39), (56, 0x04), (57, 0x16), (58, 0x07), (59, 0x09), (60, 0x0A), (61, 0x0B), (62, 0x0D),
    (63, 0x0E), (64, 0x0F), (65, 0x33), (66, 0x34), (67, 0x32), (67, 0x31), (68, 0x5C), (69, 0x5D),
    (70, 0x5E), (73, 0xE1), (74, 0x1D), (75, 0x1B), (76, 0x06), (77, 0x19), (78, 0x05), (79, 0x11),
    (80, 0x10), (81, 0x36), (82, 0x37), (83, 0x38), (84, 0xE5), (85, 0x28), (86, 0x59), (87, 0x5A),
    (88, 0x5B), (91, 0xE0), (92, 0xE3), (92, 0xE7), (93, 0xE2), (94, 0x2C), (95, 0xE6), (97, 0x65),
    (98, 0xE4), (99, 0x50), (100, 0x51), (101, 0x52), (102, 0x4F), (103, 0x2A), (104, 0x62),
    (105, 0x63), (106, 0x58), (112, 0x46), (113, 0x47), (115, 0x48), (116, 0x49), (117, 0x4A),
    (118, 0x4B), (119, 0x4C), (120, 0x4D), (121, 0x4E), (122, 0x56), (123, 0x57),
];

/// Tecla del dibujo para un código de uso HID de teclado.
pub fn index_of_usage(usage: u32) -> Option<u8> {
    USAGES.iter().find(|u| u.1 as u32 == usage).map(|u| u.0)
}

/// Código de uso HID a partir del nombre de LED de OpenRGB ("Key: A", "Key: Left Shift"…).
pub fn usage_of_name(name: &str) -> Option<u8> {
    let n = name.strip_prefix("Key:").unwrap_or(name).trim().to_lowercase();
    let n = n.as_str();
    let b = n.as_bytes();
    if b.len() == 1 {
        return match b[0] {
            c @ b'a'..=b'z' => Some(0x04 + c - b'a'),
            b'0' => Some(0x27),
            c @ b'1'..=b'9' => Some(0x1E + c - b'1'),
            b'`' => Some(0x35),
            b'-' => Some(0x2D),
            b'=' => Some(0x2E),
            b'[' => Some(0x2F),
            b']' => Some(0x30),
            b';' => Some(0x33),
            b'\'' => Some(0x34),
            b'#' => Some(0x32),
            b',' => Some(0x36),
            b'.' => Some(0x37),
            b'/' => Some(0x38),
            _ => None,
        };
    }
    if let Some(f) = n.strip_prefix('f').and_then(|f| f.parse::<u8>().ok()) {
        return (1..=12).contains(&f).then(|| 0x3A + f - 1);
    }
    for p in ["number pad ", "numpad ", "num pad ", "keypad ", "kp "] {
        if let Some(k) = n.strip_prefix(p) {
            return match k {
                "/" => Some(0x54),
                "*" => Some(0x55),
                "-" => Some(0x56),
                "+" => Some(0x57),
                "enter" => Some(0x58),
                "0" => Some(0x62),
                "." | "del" | "delete" => Some(0x63),
                d if d.len() == 1 && (b'1'..=b'9').contains(&d.as_bytes()[0]) => Some(0x59 + d.as_bytes()[0] - b'1'),
                _ => None,
            };
        }
    }
    Some(match n {
        "escape" | "esc" => 0x29,
        "print screen" | "prtsc" | "print" => 0x46,
        "scroll lock" => 0x47,
        "pause/break" | "pause" | "break" => 0x48,
        "backspace" => 0x2A,
        "insert" | "ins" => 0x49,
        "home" => 0x4A,
        "page up" | "pgup" => 0x4B,
        "delete" | "del" => 0x4C,
        "end" => 0x4D,
        "page down" | "pgdn" => 0x4E,
        "num lock" | "numlock" => 0x53,
        "tab" => 0x2B,
        "caps lock" | "capslock" => 0x39,
        "enter" | "return" => 0x28,
        "\\ (ansi)" | "\\" => 0x31,
        "\\ (iso)" => 0x64,
        "left shift" | "lshift" => 0xE1,
        "right shift" | "rshift" => 0xE5,
        "left control" | "left ctrl" | "lctrl" => 0xE0,
        "right control" | "right ctrl" | "rctrl" => 0xE4,
        "left windows" | "left win" | "lwin" | "windows" | "left super" => 0xE3,
        "right windows" | "right win" | "rwin" => 0xE7,
        "left alt" | "lalt" => 0xE2,
        "right alt" | "ralt" | "alt gr" | "altgr" => 0xE6,
        "space" | "spacebar" => 0x2C,
        "menu" | "context menu" | "application" => 0x65,
        "up arrow" | "up" => 0x52,
        "down arrow" | "down" => 0x51,
        "left arrow" | "left" => 0x50,
        "right arrow" | "right" => 0x4F,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nombres de tecla de OpenRGB de un teclado completo (ANSI + ISO).
    const OPENRGB: &[&str] = &[
        "Escape", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "Print Screen", "Scroll Lock", "Pause/Break",
        "`", "1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "-", "=", "Backspace", "Insert", "Home", "Page Up",
        "Num Lock", "Number Pad /", "Number Pad *", "Number Pad -", "Tab", "Q", "W", "E", "R", "T", "Y", "U", "I", "O", "P",
        "[", "]", "\\ (ANSI)", "Delete", "End", "Page Down", "Number Pad 7", "Number Pad 8", "Number Pad 9", "Number Pad +",
        "Caps Lock", "A", "S", "D", "F", "G", "H", "J", "K", "L", ";", "'", "#", "Enter", "Number Pad 4", "Number Pad 5",
        "Number Pad 6", "Left Shift", "Z", "X", "C", "V", "B", "N", "M", ",", ".", "/", "Right Shift", "Up Arrow",
        "Number Pad 1", "Number Pad 2", "Number Pad 3", "Number Pad Enter", "Left Control", "Left Windows", "Left Alt",
        "Space", "Right Alt", "Menu", "Right Control", "Left Arrow", "Down Arrow", "Right Arrow", "Number Pad 0", "Number Pad .",
    ];

    #[test]
    fn openrgb_names_cover_the_drawing() {
        let mut lit = std::collections::HashSet::new();
        for n in OPENRGB {
            let u = usage_of_name(&format!("Key: {n}")).unwrap_or_else(|| panic!("sin código: {n}"));
            let i = index_of_usage(u as u32).unwrap_or_else(|| panic!("sin tecla en el dibujo: {n}"));
            assert!(KEYS.iter().any(|k| k.0 == i as usize), "{n}");
            lit.insert(i);
        }
        // Todas las teclas del dibujo salvo Fn reciben luz
        let missing: Vec<&str> = KEYS.iter().filter(|k| !lit.contains(&(k.0 as u8))).map(|k| k.1).collect();
        assert_eq!(missing, vec!["FN"]);
    }

    #[test]
    fn every_usage_points_to_a_drawn_key() {
        for &(i, _) in USAGES {
            assert!(KEYS.iter().any(|k| k.0 == i as usize), "LED {i}");
        }
    }
}
