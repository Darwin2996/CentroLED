//! Estado de luces de cada dispositivo como una sola unidad (para perfiles, "aplicar a todos",
//! apagado automático y reconexiones), más atajos de teclado e inicio rápido.

use super::*;
use devices::Driver;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyNameTextW, GetKeyState, MapVirtualKeyW, RegisterHotKey, UnregisterHotKey};

/// Todo lo que define las luces de un dispositivo.
#[derive(Clone, PartialEq, Default, Debug)]
pub struct DevState {
    pub led: Option<LedSetting>,
    /// Borde del teclado.
    pub rim: Option<LedSetting>,
    /// Zonas (cascos con varias luces).
    pub zones: Vec<LedSetting>,
    /// Color de cada tecla.
    pub keys: Option<Box<[[u8; 3]; KB_LEDS]>>,
    /// Colores del ciclo suave.
    pub cycle: Vec<[u8; 3]>,
    /// LEDs de jugador / micrófono / botón Home.
    pub pad: Option<PadExtra>,
}

#[derive(Clone)]
pub struct Profile {
    pub name: String,
    pub devs: Vec<(String, DevState)>,
}

#[derive(Clone)]
pub struct Hotkey {
    /// "toggle" (apagar/encender todo) o "p:<perfil>".
    pub action: String,
    pub mods: u32,
    pub vk: u32,
}

// ------------------------------------------------------------------ texto

fn led_str(s: &LedSetting) -> String {
    format!("{},{:02x}{:02x}{:02x},{},{},{}", s.mode, s.rgb.0, s.rgb.1, s.rgb.2, s.bright, s.speed, s.multi as u8)
}

fn parse_led(t: &str) -> Option<LedSetting> {
    let f: Vec<&str> = t.split(',').collect();
    if f.len() != 5 {
        return None;
    }
    let c = parse_rgb(f[1])?;
    Some(LedSetting { mode: f[0].parse().ok()?, rgb: (c[0], c[1], c[2]), bright: f[2].parse().ok()?, speed: f[3].parse().ok()?, multi: f[4] == "1" })
}

pub fn colors_hex(list: &[[u8; 3]]) -> String {
    list.iter().map(|c| format!("{:02x}{:02x}{:02x}", c[0], c[1], c[2])).collect()
}

pub fn parse_colors(h: &str) -> Vec<[u8; 3]> {
    (0..h.len() / 6).filter_map(|i| parse_rgb(h.get(i * 6..i * 6 + 6)?)).collect()
}

impl DevState {
    pub fn to_text(&self) -> String {
        let mut parts = Vec::new();
        if let Some(l) = &self.led {
            parts.push(format!("l={}", led_str(l)));
        }
        if let Some(r) = &self.rim {
            parts.push(format!("r={}", led_str(r)));
        }
        if !self.zones.is_empty() {
            parts.push(format!("z={}", self.zones.iter().map(led_str).collect::<Vec<_>>().join("/")));
        }
        if let Some(k) = &self.keys {
            parts.push(format!("k={}", colors_hex(&k[..])));
        }
        if !self.cycle.is_empty() {
            parts.push(format!("c={}", colors_hex(&self.cycle)));
        }
        if let Some(p) = self.pad {
            parts.push(format!("p={},{},{}", p.player, p.player_bright, p.mic));
        }
        parts.join("|")
    }

    pub fn from_text(t: &str) -> DevState {
        let mut st = DevState::default();
        for part in t.split('|') {
            let Some((k, v)) = part.split_once('=') else { continue };
            match k {
                "l" => st.led = parse_led(v),
                "r" => st.rim = parse_led(v),
                "z" => st.zones = v.split('/').filter_map(parse_led).collect(),
                "k" => {
                    let list = parse_colors(v);
                    if list.len() == KB_LEDS {
                        let mut keys = Box::new([[0u8; 3]; KB_LEDS]);
                        keys.copy_from_slice(&list);
                        st.keys = Some(keys);
                    }
                }
                "c" => st.cycle = parse_colors(v),
                "p" => {
                    let n: Vec<u8> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                    if n.len() == 3 {
                        st.pad = Some(PadExtra { player: n[0], player_bright: n[1], mic: n[2] });
                    }
                }
                _ => {}
            }
        }
        st
    }

    fn lit(&self) -> bool {
        self.led.map(|l| l.mode != MODE_OFF).unwrap_or(false) || self.zones.iter().any(|z| z.mode != MODE_OFF)
    }
}

/// Nombre válido para un perfil (sin caracteres que rompan el archivo de configuración).
pub fn clean_name(s: &str) -> String {
    let s: String = s.chars().filter(|c| !matches!(c, '\t' | '\n' | '\r' | '|')).take(40).collect();
    s.trim().to_string()
}

// ------------------------------------------------------------------ configuración <-> estado

/// Estado guardado de un dispositivo (puede estar incompleto o vacío).
pub fn state_of(cfg: &Config, key: &str) -> DevState {
    let mut zones = Vec::new();
    while let Some(z) = cfg.led.get(&format!("{key}#z{}", zones.len())) {
        zones.push(*z);
    }
    DevState {
        led: cfg.led.get(key).copied(),
        rim: cfg.led.get(&format!("{key}#borde")).copied(),
        zones,
        keys: cfg.keys.get(key).cloned(),
        cycle: cfg.cycles.get(key).cloned().unwrap_or_default(),
        pad: cfg.pads.get(key).copied(),
    }
}

pub fn store_state(cfg: &mut Config, key: &str, st: &DevState) {
    let put = |cfg: &mut Config, k: String, v: Option<LedSetting>| match v {
        Some(v) => {
            cfg.led.insert(k, v);
        }
        None => {
            cfg.led.remove(&k);
        }
    };
    put(cfg, key.to_string(), st.led);
    put(cfg, format!("{key}#borde"), st.rim);
    let mut i = 0;
    while cfg.led.remove(&format!("{key}#z{i}")).is_some() {
        i += 1;
    }
    for (i, z) in st.zones.iter().enumerate() {
        cfg.led.insert(format!("{key}#z{i}"), *z);
    }
    match &st.keys {
        Some(k) => cfg.keys.insert(key.to_string(), k.clone()),
        None => cfg.keys.remove(key),
    };
    if st.cycle.len() >= 2 {
        cfg.cycles.insert(key.to_string(), st.cycle.clone());
    } else {
        cfg.cycles.remove(key);
    }
    match st.pad {
        Some(p) => cfg.pads.insert(key.to_string(), p),
        None => cfg.pads.remove(key),
    };
}

pub fn is_switch(d: &Device) -> bool {
    matches!(d.driver, Driver::Switch { .. })
}

pub fn default_pad(d: &Device) -> PadExtra {
    if is_switch(d) {
        PadExtra { player: 0b0001, player_bright: 2, mic: 0 }
    } else {
        PadExtra::default()
    }
}

/// Estado completo de un dispositivo presente: lo guardado más los valores por defecto.
pub fn full_state(d: &Device) -> DevState {
    let mut st = state_of(&app().config, &d.key);
    let base = *st.led.get_or_insert_with(|| default_setting(d));
    let caps = d.led.as_ref();
    st.zones.resize(caps.map(|c| c.zones.len()).unwrap_or(0), base);
    if is_keyboard(d) {
        st.rim.get_or_insert(LedSetting { mode: 24, rgb: base.rgb, bright: 100, speed: 60, multi: false });
        if st.keys.is_none() {
            st.keys = Some(Box::new([[base.rgb.0, base.rgb.1, base.rgb.2]; KB_LEDS]));
        }
    } else {
        st.rim = None;
        st.keys = None;
    }
    if caps.map(|c| c.pad_extras).unwrap_or(false) {
        st.pad.get_or_insert(default_pad(d));
    } else {
        st.pad = None;
    }
    st
}

/// Lo que se está editando ahora mismo en la página del dispositivo.
pub fn editor_state(d: &Device) -> DevState {
    let a = app();
    let kb = is_keyboard(d);
    let led = if a.zones.is_empty() { a.cur } else { a.zones[0] };
    DevState {
        led: Some(led),
        rim: kb.then_some(a.rim),
        zones: a.zones.clone(),
        keys: kb.then(|| a.keys.clone()),
        cycle: if led.mode == MODE_CYCLE { a.cycle.clone() } else { Vec::new() },
        pad: d.led.as_ref().filter(|c| c.pad_extras).map(|_| a.pad),
    }
}

/// Carga un estado en el editor del dispositivo seleccionado.
pub fn load_editor(st: &DevState) {
    let a = app();
    if let Some(l) = st.led {
        a.cur = l;
    }
    if let Some(r) = st.rim {
        a.rim = r;
    }
    if !a.zones.is_empty() && st.zones.len() == a.zones.len() {
        a.zones = st.zones.clone();
    }
    if let Some(k) = &st.keys {
        a.keys = k.clone();
    }
    if st.cycle.len() >= 2 {
        a.cycle = st.cycle.clone();
    }
    if let Some(p) = st.pad {
        a.pad = p;
    }
}

/// Mismo estado con las luces apagadas (el teclado apaga también el borde; los mandos de Switch
/// sólo la luz del botón Home, sus LEDs de jugador indican el número de mando).
pub fn off_state(d: &Device, st: &DevState) -> DevState {
    off_state_for(is_switch(d), st)
}

fn off_state_for(switch: bool, st: &DevState) -> DevState {
    let mut o = st.clone();
    if let Some(l) = o.led.as_mut() {
        l.mode = MODE_OFF;
    }
    if let Some(r) = o.rim.as_mut() {
        r.mode = 25;
    }
    for z in &mut o.zones {
        z.mode = MODE_OFF;
    }
    if let Some(p) = o.pad.as_mut() {
        if switch {
            p.player_bright = 0;
        } else {
            p.player = 0;
            p.mic = 0;
        }
    }
    o
}

pub fn extra_of(d: &Device, st: &DevState) -> Option<Extra> {
    let led = st.led?;
    if is_keyboard(d) {
        return Some(Extra::Keyboard(KbExtra {
            rim: st.rim.unwrap_or(LedSetting { mode: 24, rgb: led.rgb, bright: 100, speed: 60, multi: false }),
            keys: st.keys.clone().unwrap_or_else(|| Box::new([[led.rgb.0, led.rgb.1, led.rgb.2]; KB_LEDS])),
        }));
    }
    let caps = d.led.as_ref()?;
    if !caps.zones.is_empty() {
        return Some(Extra::Zones((0..caps.zones.len()).map(|i| st.zones.get(i).copied().unwrap_or(led)).collect()));
    }
    if caps.pad_extras {
        return st.pad.map(Extra::Pad);
    }
    None
}

/// Envía un estado al dispositivo (en segundo plano). Los efectos animados por la app arrancan su hilo.
pub fn send(d: &Device, st: &DevState, silent: bool) {
    let (Some(caps), Some(led)) = (d.led.as_ref(), st.led) else { return };
    if is_app_animated(led.mode) {
        let pad = if caps.pad_extras { st.pad } else { None };
        start_cycle(d.clone(), st.cycle.clone(), led, pad);
        return;
    }
    stop_cycle_wait(&d.key);
    start_apply(d.clone(), led, extra_of(d, st), silent);
}

/// Igual que `send` pero espera a que termine (al suspender o apagar el equipo no hay tiempo
/// para hilos).
fn send_sync(d: &Device, st: &DevState) {
    stop_cycle_wait(&d.key);
    let Some(led) = st.led else { return };
    let _g = HID_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = devices::apply(d, &led, extra_of(d, st).as_ref());
}

/// Para el ciclo de un mando y espera a que su hilo deje de escribir.
fn stop_cycle_wait(key: &str) {
    if let Some(c) = app().cyclers.remove(key) {
        c.stop.store(true, Ordering::Relaxed);
        for _ in 0..20 {
            if !c.alive.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

/// Guarda varios estados y los aplica a los dispositivos presentes; los ausentes se aplican
/// cuando se conectan. El teclado sólo se reescribe si su estado cambia (su memoria se desgasta).
fn apply_states(states: Vec<(String, DevState)>) {
    let a = app();
    for (key, st) in states {
        let dev = a.devices.iter().find(|d| d.key == key && d.led.is_some()).cloned();
        let before = dev.as_ref().map(full_state);
        store_state(&mut a.config, &key, &st);
        match dev {
            Some(d) => {
                let after = full_state(&d);
                if is_keyboard(&d) && before.as_ref() == Some(&after) {
                    continue;
                }
                if a.auto_off != 0 && auto_included(&d) {
                    continue; // se aplicará al volver
                }
                send(&d, &after, true);
            }
            None => {
                a.pending.insert(key);
            }
        }
    }
    save_config(&a.config);
    refresh_editor();
}

/// Vuelve a cargar el editor del dispositivo abierto con lo guardado.
pub fn refresh_editor() {
    let a = app();
    if let Some(i) = a.sel.as_ref().and_then(|k| a.devices.iter().position(|d| &d.key == k)) {
        let (tab, hist) = (a.tab, a.show_hist);
        select_device(i);
        a.tab = tab;
        a.show_hist = hist;
    }
    redraw();
}

// ------------------------------------------------------------------ perfiles globales

/// Estado actual de los dispositivos cuyas luces se han configurado en la app (estén o no
/// conectados). Los que nunca se tocaron no entran: un perfil no les cambia nada.
fn snapshot_all() -> Vec<(String, DevState)> {
    let a = app();
    let mut keys: Vec<String> = a.config.led.keys().filter(|k| !k.contains('#')).cloned().collect();
    keys.sort();
    keys.into_iter()
        .map(|k| {
            let st = match a.devices.iter().find(|d| d.key == k && d.led.is_some()) {
                Some(d) => full_state(d),
                None => state_of(&a.config, &k),
            };
            (k, st)
        })
        .filter(|(_, st)| st.led.is_some())
        .collect()
}

/// Crea los perfiles de ejemplo la primera vez que hay dispositivos con luces.
pub fn seed_profiles() {
    if seed_profiles_mem() {
        save_config(&app().config);
    }
}

/// Crea los perfiles de ejemplo sin guardar (también lo usan las capturas de prueba).
pub fn seed_profiles_mem() -> bool {
    let a = app();
    // Esperar a que haya al menos un dispositivo con sus luces guardadas.
    if a.config.seeded || !a.config.led.keys().any(|k| !k.contains('#')) {
        return false;
    }
    a.config.seeded = true;
    if a.config.profiles.is_empty() {
        let base = snapshot_all();
        let dim = |s: &mut LedSetting| s.bright = s.bright.min(20);
        let night: Vec<_> = base
            .iter()
            .map(|(k, st)| {
                let mut s = st.clone();
                s.led.as_mut().map(dim);
                s.rim.as_mut().map(dim);
                s.zones.iter_mut().for_each(dim);
                (k.clone(), s)
            })
            .collect();
        // "Apagado" deja el teclado como esté (sus claves empiezan por "tempest").
        let off: Vec<_> = base.iter().filter(|(k, _)| !k.starts_with("tempest")).map(|(k, st)| (k.clone(), off_state_for(k.starts_with("ns-"), st))).collect();
        a.config.profiles.push(Profile { name: "Juego".into(), devs: base });
        a.config.profiles.push(Profile { name: "Noche".into(), devs: night });
        if !off.is_empty() {
            a.config.profiles.push(Profile { name: "Apagado".into(), devs: off });
        }
    }
    true
}

fn unique_name(name: &str, taken: &[String]) -> String {
    let mut n = name.to_string();
    let mut i = 2;
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&n)) {
        n = format!("{name} {i}");
        i += 1;
    }
    n
}

pub fn apply_profile(i: usize) {
    let a = app();
    let Some(p) = a.config.profiles.get(i).cloned() else { return };
    a.config.off_backup.clear();
    a.config.active_profile = p.name.clone();
    apply_states(p.devs);
    set_status(&format!("✓ Perfil «{}» aplicado", p.name));
}

pub fn apply_profile_named(name: &str) {
    if let Some(i) = app().config.profiles.iter().position(|p| p.name == name) {
        apply_profile(i);
    }
}

pub fn save_new_profile() {
    let a = app();
    let taken: Vec<String> = a.config.profiles.iter().map(|p| p.name.clone()).collect();
    let suggestion = unique_name("Mi perfil", &taken);
    let Some(name) = ask_name("Guardar perfil con las luces actuales", &suggestion) else { return };
    let name = unique_name(&name, &taken);
    a.config.profiles.push(Profile { name: name.clone(), devs: snapshot_all() });
    a.config.active_profile = name.clone();
    save_config(&a.config);
    set_status(&format!("✓ Perfil «{name}» guardado con las luces actuales"));
}

/// Menú de un perfil (clic derecho en su botón).
pub fn profile_menu(i: usize) {
    let a = app();
    let Some(name) = a.config.profiles.get(i).map(|p| p.name.clone()) else { return };
    let cmd = popup(&[(1, "Aplicar", false), (2, "Guardar aquí las luces actuales", false), (0, "", false), (3, "Cambiar nombre…", false), (4, "Borrar", false)]);
    match cmd {
        1 => apply_profile(i),
        2 => {
            a.config.profiles[i].devs = snapshot_all();
            save_config(&a.config);
            set_status(&format!("✓ Perfil «{name}» actualizado"));
        }
        3 => {
            let Some(new) = ask_name("Nuevo nombre del perfil", &name) else { return };
            let taken: Vec<String> = a.config.profiles.iter().filter(|p| p.name != name).map(|p| p.name.clone()).collect();
            let new = unique_name(&new, &taken);
            a.config.profiles[i].name = new.clone();
            for h in &mut a.config.hotkeys {
                if h.action == format!("p:{name}") {
                    h.action = format!("p:{new}");
                }
            }
            if a.config.active_profile == name {
                a.config.active_profile = new;
            }
            save_config(&a.config);
            register_hotkeys();
            redraw();
        }
        4 => {
            a.config.profiles.remove(i);
            a.config.hotkeys.retain(|h| h.action != format!("p:{name}"));
            if a.config.active_profile == name {
                a.config.active_profile.clear();
            }
            save_config(&a.config);
            register_hotkeys();
            set_status(&format!("Perfil «{name}» borrado"));
        }
        _ => {}
    }
}

/// Perfiles que no caben en la barra de inicio.
pub fn more_profiles_menu(from: usize) {
    let a = app();
    let mut items: Vec<(usize, String, bool)> = a.config.profiles.iter().enumerate().skip(from).map(|(i, p)| (10 + i, p.name.clone(), p.name == a.config.active_profile)).collect();
    items.push((0, String::new(), false));
    items.push((1, "Guardar las luces actuales como perfil…".into(), false));
    let refs: Vec<(usize, &str, bool)> = items.iter().map(|(i, n, c)| (*i, n.as_str(), *c)).collect();
    match popup(&refs) {
        1 => save_new_profile(),
        n if n >= 10 => apply_profile(n - 10),
        _ => {}
    }
}

// ------------------------------------------------------------------ perfiles de un dispositivo

pub fn device_profiles_menu(d: &Device) {
    let a = app();
    let list = a.config.dprofiles.get(&d.key).cloned().unwrap_or_default();
    let now = editor_state(d);
    let unsafe_menu = unsafe { CreatePopupMenu() };
    let add = |m: HMENU, id: usize, text: &str, flags: u32| unsafe {
        AppendMenuW(m, flags, id, wide(text).as_ptr());
    };
    if list.is_empty() {
        add(unsafe_menu, 0, "Sin perfiles guardados", MF_STRING | MF_GRAYED);
    }
    for (i, (name, st)) in list.iter().enumerate() {
        add(unsafe_menu, 1000 + i, name, MF_STRING | if *st == now { MF_CHECKED } else { 0 });
    }
    unsafe { AppendMenuW(unsafe_menu, MF_SEPARATOR, 0, null()) };
    add(unsafe_menu, 1, "Guardar estos ajustes como perfil…", MF_STRING);
    if !list.is_empty() {
        let (upd, ren, del) = unsafe { (CreatePopupMenu(), CreatePopupMenu(), CreatePopupMenu()) };
        for (i, (name, _)) in list.iter().enumerate() {
            add(upd, 2000 + i, name, MF_STRING);
            add(ren, 3000 + i, name, MF_STRING);
            add(del, 4000 + i, name, MF_STRING);
        }
        add(unsafe_menu, upd as usize, "Guardar estos ajustes en…", MF_STRING | MF_POPUP);
        add(unsafe_menu, ren as usize, "Cambiar nombre", MF_STRING | MF_POPUP);
        add(unsafe_menu, del as usize, "Borrar", MF_STRING | MF_POPUP);
    }
    let cmd = track(unsafe_menu);
    let names: Vec<String> = list.iter().map(|(n, _)| n.clone()).collect();
    match cmd {
        1 => {
            let Some(name) = ask_name(&format!("Perfil de {}", d.name), &unique_name("Perfil", &names)) else { return };
            let name = unique_name(&name, &names);
            a.config.dprofiles.entry(d.key.clone()).or_default().push((name.clone(), now));
            save_config(&a.config);
            set_status(&format!("✓ Perfil «{name}» guardado"));
        }
        1000..=1999 => {
            if let Some((name, st)) = list.get(cmd - 1000).cloned() {
                load_editor(&st);
                on_apply();
                set_status(&format!("✓ Perfil «{name}» aplicado a {}", d.name));
            }
        }
        2000..=2999 => {
            if let Some(e) = a.config.dprofiles.get_mut(&d.key).and_then(|l| l.get_mut(cmd - 2000)) {
                e.1 = now;
                save_config(&a.config);
                set_status("✓ Perfil actualizado");
            }
        }
        3000..=3999 => {
            let i = cmd - 3000;
            let Some(old) = names.get(i).cloned() else { return };
            let Some(new) = ask_name("Nuevo nombre del perfil", &old) else { return };
            let others: Vec<String> = names.iter().filter(|n| **n != old).cloned().collect();
            a.config.dprofiles.get_mut(&d.key).unwrap()[i].0 = unique_name(&new, &others);
            save_config(&a.config);
        }
        4000..=4999 => {
            if let Some(l) = a.config.dprofiles.get_mut(&d.key).filter(|l| cmd - 4000 < l.len()) {
                l.remove(cmd - 4000);
                save_config(&a.config);
            }
        }
        _ => {}
    }
    redraw();
}

// ------------------------------------------------------------------ todos los dispositivos

fn lighting() -> Vec<Device> {
    app().devices.iter().filter(|d| d.led.is_some()).cloned().collect()
}

pub fn all_lights_off() -> bool {
    !app().config.off_backup.is_empty()
}

/// Pone el mismo color a todas las luces (los efectos sin color pasan a color fijo).
pub fn all_color(c: [u8; 3]) {
    let a = app();
    let mut out = Vec::new();
    for d in lighting() {
        let caps = d.led.clone().unwrap();
        let kb = is_keyboard(&d);
        let solid = caps.modes.iter().find(|m| m.1 != MODE_OFF).map(|m| m.1).unwrap_or(1);
        let fix = |s: &mut LedSetting| {
            s.rgb = (c[0], c[1], c[2]);
            s.multi = false;
            if is_app_animated(s.mode) || s.mode == MODE_OFF || (kb && s.mode == MODE_CUSTOM) || (!kb && matches!(s.mode, 0x03 | 0x0E)) {
                s.mode = solid;
            }
        };
        let mut st = full_state(&d);
        st.led.as_mut().map(fix);
        st.zones.iter_mut().for_each(fix);
        if let Some(r) = st.rim.as_mut() {
            r.rgb = (c[0], c[1], c[2]);
            r.multi = false;
            if matches!(r.mode, 21 | 22 | 25) {
                r.mode = 24;
            }
        }
        out.push((d.key.clone(), st));
    }
    a.config.off_backup.clear();
    a.config.active_profile.clear();
    apply_states(out);
    set_status("✓ Color aplicado a todas las luces");
}

pub fn all_off() {
    let a = app();
    let mut backup = Vec::new();
    let mut out = Vec::new();
    for d in lighting() {
        let st = full_state(&d);
        if st.lit() || st.rim.map(|r| r.mode != 25).unwrap_or(false) {
            out.push((d.key.clone(), off_state(&d, &st)));
            backup.push((d.key.clone(), st));
        }
    }
    if backup.is_empty() {
        set_status("Las luces ya están apagadas");
        return;
    }
    a.config.off_backup = backup;
    a.config.active_profile.clear();
    apply_states(out);
    set_status("Luces apagadas · «Encender todo» las deja como estaban");
}

pub fn all_on() {
    let a = app();
    let backup = std::mem::take(&mut a.config.off_backup);
    if backup.is_empty() {
        return;
    }
    apply_states(backup);
    set_status("✓ Luces encendidas como estaban");
}

pub fn toggle_all() {
    if all_lights_off() {
        all_on();
    } else {
        all_off();
    }
}

// ------------------------------------------------------------------ apagado automático

pub const AUTO_LOCK: u8 = 1;
pub const AUTO_DISPLAY: u8 = 2;
pub const AUTO_SLEEP: u8 = 4;

/// Participa en el apagado automático: tiene luces configuradas, no es el teclado y el usuario
/// no lo ha quitado.
pub fn auto_included(d: &Device) -> bool {
    let a = app();
    d.led.is_some() && !is_keyboard(d) && a.config.led.contains_key(&d.key) && !a.config.auto_exclude.contains(&d.key)
}

/// Activa o desactiva un motivo de apagado (bloqueo, pantalla, suspensión). Las luces vuelven
/// cuando no queda ninguno. No cambia la configuración guardada.
pub fn auto_set(bit: u8, on: bool, sync: bool) {
    let a = app();
    let before = a.auto_off;
    if on {
        a.auto_off |= bit;
    } else {
        a.auto_off &= !bit;
    }
    if before == 0 && a.auto_off != 0 {
        auto_lights_off(sync);
    } else if before != 0 && a.auto_off == 0 {
        for d in lighting().iter().filter(|d| auto_included(d)) {
            send(d, &full_state(d), true);
        }
    }
}

pub fn auto_lights_off(sync: bool) {
    for d in lighting().iter().filter(|d| auto_included(d)) {
        let off = off_state(d, &full_state(d));
        if sync {
            send_sync(d, &off);
        } else {
            send(d, &off, true);
        }
    }
}

/// Qué hacer con un dispositivo que se acaba de conectar mientras las luces están apagadas solas.
pub fn reconnect_state(d: &Device) -> DevState {
    let st = full_state(d);
    if app().auto_off != 0 && auto_included(d) {
        off_state(d, &st)
    } else {
        st
    }
}

// ------------------------------------------------------------------ atajos de teclado

const MOD_ALT: u32 = 1;
const MOD_CONTROL: u32 = 2;
const MOD_SHIFT: u32 = 4;
const MOD_WIN: u32 = 8;
const MOD_NOREPEAT: u32 = 0x4000;
const HK_BASE: i32 = 0x100;

/// Acciones a las que se puede asignar un atajo: (acción, descripción).
pub fn hk_actions() -> Vec<(String, String)> {
    let mut v = vec![("toggle".to_string(), "Apagar / encender todas las luces".to_string())];
    for p in &app().config.profiles {
        v.push((format!("p:{}", p.name), format!("Perfil «{}»", p.name)));
    }
    v
}

fn unregister_hotkeys() {
    let a = app();
    for id in a.hk_ids.drain(..) {
        unsafe { UnregisterHotKey(a.hwnd, id) };
    }
}

/// Empieza (o cancela) la grabación del atajo de una acción. Mientras se graba, los atajos de la
/// app se desactivan para poder volver a grabar uno que ya existe.
pub fn toggle_recording(action: &str) {
    let a = app();
    if a.recording.as_deref() == Some(action) {
        a.recording = None;
        register_hotkeys();
    } else {
        a.recording = Some(action.to_string());
        unregister_hotkeys();
    }
    redraw();
}

/// Registra en Windows todos los atajos guardados; apunta los que otro programa ya usa.
pub fn register_hotkeys() {
    let a = app();
    unregister_hotkeys();
    a.hk_failed.clear();
    for (i, h) in a.config.hotkeys.iter().enumerate() {
        let id = HK_BASE + i as i32;
        if unsafe { RegisterHotKey(a.hwnd, id, h.mods | MOD_NOREPEAT, h.vk) } != 0 {
            a.hk_ids.push(id);
        } else {
            a.hk_failed.insert(h.action.clone());
        }
    }
}

pub fn on_hotkey(id: i32) {
    let Some(action) = app().config.hotkeys.get((id - HK_BASE) as usize).map(|h| h.action.clone()) else { return };
    if action == "toggle" {
        toggle_all();
    } else if let Some(name) = action.strip_prefix("p:") {
        apply_profile_named(name);
    }
}

pub fn hotkey_of(action: &str) -> Option<&'static Hotkey> {
    app().config.hotkeys.iter().find(|h| h.action == action)
}

pub fn clear_hotkey(action: &str) {
    let a = app();
    a.config.hotkeys.retain(|h| h.action != action);
    save_config(&a.config);
    register_hotkeys();
    redraw();
}

/// Tecla pulsada mientras se graba un atajo. Devuelve true si la ha usado.
pub fn record_key(vk: u32) -> bool {
    let a = app();
    let Some(action) = a.recording.clone() else { return false };
    if matches!(vk, 0x10 | 0x11 | 0x12 | 0x5B | 0x5C | 0xA0..=0xA5) {
        return true; // sólo modificadores: esperar a la tecla
    }
    if vk == 0x1B {
        a.recording = None;
        register_hotkeys();
        redraw();
        return true;
    }
    let down = |k: i32| unsafe { GetKeyState(k) } < 0;
    let mods = if down(0x12) { MOD_ALT } else { 0 } | if down(0x11) { MOD_CONTROL } else { 0 } | if down(0x10) { MOD_SHIFT } else { 0 } | if down(0x5B) || down(0x5C) { MOD_WIN } else { 0 };
    if mods == 0 && !(0x70..=0x87).contains(&vk) {
        set_status("Combina la tecla con Ctrl, Alt, Mayús o Windows (o usa una tecla F)");
        return true;
    }
    a.recording = None;
    let mut msg = String::new();
    if let Some(other) = a.config.hotkeys.iter().find(|h| h.mods == mods && h.vk == vk && h.action != action) {
        let label = hk_actions().into_iter().find(|(x, _)| *x == other.action).map(|(_, l)| l).unwrap_or_default();
        msg = format!(" (se ha quitado de «{label}»)");
    }
    a.config.hotkeys.retain(|h| h.action != action && !(h.mods == mods && h.vk == vk));
    a.config.hotkeys.push(Hotkey { action: action.clone(), mods, vk });
    save_config(&a.config);
    register_hotkeys();
    if a.hk_failed.contains(&action) {
        set_status(&format!("Windows no deja usar {}: otro programa ya la tiene. Prueba otra.", combo_name(mods, vk)));
    } else {
        set_status(&format!("✓ Atajo {} guardado{msg}", combo_name(mods, vk)));
    }
    true
}

pub fn combo_name(mods: u32, vk: u32) -> String {
    let mut s = String::new();
    for (m, n) in [(MOD_CONTROL, "Ctrl"), (MOD_ALT, "Alt"), (MOD_SHIFT, "Mayús"), (MOD_WIN, "Win")] {
        if mods & m != 0 {
            s += n;
            s += " + ";
        }
    }
    s + &key_name(vk)
}

fn key_name(vk: u32) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from(vk as u8).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x60..=0x69 => format!("Num {}", vk - 0x60),
        0x20 => "Espacio".into(),
        _ => unsafe {
            let sc = MapVirtualKeyW(vk, 0);
            let ext = matches!(vk, 0x21..=0x28 | 0x2D | 0x2E | 0x6F | 0x90);
            let mut buf = [0u16; 64];
            let n = GetKeyNameTextW(((sc << 16) | if ext { 1 << 24 } else { 0 }) as i32, buf.as_mut_ptr(), 64);
            if n > 0 {
                String::from_utf16_lossy(&buf[..n as usize])
            } else {
                format!("Tecla {vk}")
            }
        },
    }
}

// ------------------------------------------------------------------ inicio rápido

const TASK: &str = "CentroLED";

fn schtasks(args: &[&str]) -> Option<std::process::Output> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("schtasks").args(args).creation_flags(0x0800_0000).output().ok()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Tarea programada al iniciar sesión, sin el retraso que Windows aplica a los programas de
/// inicio y con prioridad por encima de lo normal. No necesita permisos de administrador.
pub fn fast_start_set(on: bool) -> Result<(), String> {
    if !on {
        schtasks(&["/delete", "/tn", TASK, "/f"]);
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let user = format!("{}\\{}", std::env::var("USERDOMAIN").unwrap_or_default(), std::env::var("USERNAME").unwrap_or_default());
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>Inicia Centro LED nada más iniciar sesión</Description></RegistrationInfo>
  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <IdleSettings><StopOnIdleEnd>false</StopOnIdleEnd><RestartOnIdle>false</RestartOnIdle></IdleSettings>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>2</Priority>
  </Settings>
  <Actions Context="Author"><Exec><Command>{exe}</Command><Arguments>--tray</Arguments></Exec></Actions>
</Task>
"#,
        user = xml_escape(&user),
        exe = xml_escape(&exe.display().to_string()),
    );
    let mut bytes = vec![0xFF, 0xFE];
    for u in xml.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    let path = std::env::temp_dir().join("CentroLED-tarea.xml");
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let out = schtasks(&["/create", "/tn", TASK, "/xml", &path.display().to_string(), "/f"]);
    let _ = std::fs::remove_file(&path);
    match out {
        Some(o) if o.status.success() => Ok(()),
        _ => Err("Windows no dejó crear la tarea de inicio".into()),
    }
}

/// Al arrancar: si la tarea falta o apunta a un .exe que ya no existe (se movió), rehacerla
/// (en segundo plano). Si apunta a otra copia que sí existe, se deja.
pub fn fast_start_check() {
    std::thread::spawn(move || {
        let ok = schtasks(&["/query", "/tn", TASK, "/xml"])
            .filter(|o| o.status.success())
            .map(|o| {
                let text = if o.stdout.starts_with(&[0xFF, 0xFE]) {
                    String::from_utf16_lossy(&o.stdout[2..].chunks(2).map(|c| u16::from_le_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect::<Vec<_>>())
                } else {
                    String::from_utf8_lossy(&o.stdout).into_owned()
                };
                let text = text.replace("&amp;", "&");
                text.split("<Command>").nth(1).and_then(|c| c.split("</Command>").next()).map(|c| std::path::Path::new(c.trim()).exists()).unwrap_or(false)
            })
            .unwrap_or(false);
        if !ok {
            let _ = fast_start_set(true);
        }
    });
}

// ------------------------------------------------------------------ menús y diálogo de nombre

fn track(menu: HMENU) -> usize {
    unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        SetForegroundWindow(app().hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, app().hwnd, null()) as usize;
        DestroyMenu(menu);
        cmd
    }
}

/// Menú emergente simple: (id, texto, marcado); id 0 = separador.
pub fn popup(items: &[(usize, &str, bool)]) -> usize {
    unsafe {
        let menu = CreatePopupMenu();
        for &(id, text, checked) in items {
            if id == 0 {
                AppendMenuW(menu, MF_SEPARATOR, 0, null());
            } else {
                AppendMenuW(menu, MF_STRING | if checked { MF_CHECKED } else { 0 }, id, wide(text).as_ptr());
            }
        }
        track(menu)
    }
}

static NAME_RESULT: Mutex<String> = Mutex::new(String::new());

unsafe extern "system" fn name_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> isize {
    match msg {
        WM_INITDIALOG => {
            SetDlgItemTextW(h, 100, lp as *const u16);
            SendDlgItemMessageW(h, 100, 0x00B1 /* EM_SETSEL */, 0, -1);
            SetFocus(GetDlgItem(h, 100));
            let dark: i32 = 1;
            DwmSetWindowAttribute(h, 20, &dark as *const i32 as _, 4);
            0
        }
        WM_COMMAND => {
            match (wp & 0xFFFF) as i32 {
                1 => {
                    let mut buf = [0u16; 64];
                    let n = GetDlgItemTextW(h, 100, buf.as_mut_ptr(), 64) as usize;
                    *NAME_RESULT.lock().unwrap_or_else(|e| e.into_inner()) = String::from_utf16_lossy(&buf[..n]);
                    EndDialog(h, 1);
                }
                2 => {
                    EndDialog(h, 0);
                }
                _ => return 0,
            }
            1
        }
        _ => 0,
    }
}

/// Pequeño cuadro de diálogo nativo para escribir un nombre.
pub fn ask_name(title: &str, initial: &str) -> Option<String> {
    let t = name_template(title);
    let init = wide(initial);
    let r = unsafe { DialogBoxIndirectParamW(GetModuleHandleW(null()), t.as_ptr() as *const DLGTEMPLATE, app().hwnd, Some(name_proc), init.as_ptr() as LPARAM) };
    if r != 1 {
        return None;
    }
    let name = clean_name(&NAME_RESULT.lock().unwrap_or_else(|e| e.into_inner()));
    (!name.is_empty()).then_some(name)
}

/// Prueba del diálogo sin mostrarlo en pantalla: lo crea fuera de la vista, lo dibuja en un .bmp
/// y comprueba que el cuadro de texto recibe el nombre inicial (`--test-dialog salida.bmp salida.txt`).
pub unsafe fn test_dialog(bmp: &str, txt: &str) {
    let t = name_template("Guardar perfil con las luces actuales");
    let init = wide("Mi perfil");
    let h = CreateDialogIndirectParamW(GetModuleHandleW(null()), t.as_ptr() as *const DLGTEMPLATE, null_mut(), Some(name_proc), init.as_ptr() as LPARAM);
    if h.is_null() {
        let _ = std::fs::write(txt, format!("ERROR {}", GetLastError()));
        return;
    }
    SetWindowPos(h, null_mut(), -4000, -4000, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW);
    let mut msg: MSG = std::mem::zeroed();
    while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
        DispatchMessageW(&msg);
    }
    let mut r: RECT = std::mem::zeroed();
    GetWindowRect(h, &mut r);
    render_to_file(r.right - r.left, r.bottom - r.top, bmp, |dc| {
        SendMessageW(h, WM_PRINT, dc as WPARAM, (PRF_CLIENT | PRF_NONCLIENT | PRF_CHILDREN | PRF_ERASEBKGND) as LPARAM);
    });
    let mut buf = [0u16; 64];
    let n = GetDlgItemTextW(h, 100, buf.as_mut_ptr(), 64) as usize;
    let _ = std::fs::write(txt, format!("OK {}x{} «{}»", r.right - r.left, r.bottom - r.top, String::from_utf16_lossy(&buf[..n])));
    DestroyWindow(h);
}

fn name_template(title: &str) -> Vec<u16> {
    let mut t: Vec<u16> = Vec::new();
    let dw = |t: &mut Vec<u16>, v: u32| {
        t.push(v as u16);
        t.push((v >> 16) as u16);
    };
    let s = |t: &mut Vec<u16>, text: &str| {
        t.extend(text.encode_utf16());
        t.push(0);
    };
    let align = |t: &mut Vec<u16>| {
        if t.len() % 2 == 1 {
            t.push(0);
        }
    };
    // DLGTEMPLATE: DS_SETFONT | DS_MODALFRAME | DS_CENTER | WS_POPUP | WS_CAPTION | WS_SYSMENU
    dw(&mut t, 0x40 | 0x80 | 0x800 | 0x8000_0000 | 0x00C0_0000 | 0x0008_0000);
    dw(&mut t, 0);
    t.extend_from_slice(&[3, 0, 0, 230, 58]); // 3 controles, x, y, ancho, alto (unidades de diálogo)
    t.extend_from_slice(&[0, 0]); // sin menú, clase por defecto
    s(&mut t, title);
    t.push(9);
    s(&mut t, "Segoe UI");
    let item = |t: &mut Vec<u16>, style: u32, x: u16, y: u16, w: u16, h: u16, id: u16, class: u16, text: &str| {
        align(t);
        dw(t, style);
        dw(t, 0);
        t.extend_from_slice(&[x, y, w, h, id, 0xFFFF, class]);
        s(t, text);
        t.push(0);
    };
    // Cuadro de texto (WS_CHILD|WS_VISIBLE|WS_BORDER|WS_TABSTOP|ES_AUTOHSCROLL) y botones
    item(&mut t, 0x5081_0080, 8, 9, 214, 14, 100, 0x0081, "");
    item(&mut t, 0x5001_0001, 118, 34, 50, 16, 1, 0x0080, "Aceptar");
    item(&mut t, 0x5001_0000, 172, 34, 50, 16, 2, 0x0080, "Cancelar");
    t
}
