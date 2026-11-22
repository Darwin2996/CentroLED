//! Acceso HID mínimo sobre la API nativa de Windows (hid.dll + setupapi).

use std::ptr::{null, null_mut};
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Devices::HumanInterfaceDevice::*;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows_sys::Win32::System::IO::{CancelIo, GetOverlappedResult, OVERLAPPED};

#[derive(Clone, Debug)]
pub struct HidInfo {
    pub path: Vec<u16>, // terminada en 0
    pub vid: u16,
    pub pid: u16,
    pub usage_page: u16,
    pub usage: u16,
    pub in_len: u16,
    pub out_len: u16,
    pub feat_len: u16,
    pub product: String,
    /// Número de interfaz USB ("&mi_XX" en la ruta), si existe.
    pub interface: Option<u8>,
}

fn wide_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end]).trim().to_string()
}

fn parse_interface(path: &str) -> Option<u8> {
    let i = path.find("&mi_")?;
    u8::from_str_radix(path.get(i + 4..i + 6)?, 16).ok()
}

/// Lista todas las colecciones HID presentes con sus capacidades.
pub fn enumerate() -> Vec<HidInfo> {
    let mut out = Vec::new();
    unsafe {
        let mut guid = std::mem::zeroed();
        HidD_GetHidGuid(&mut guid);
        let set = SetupDiGetClassDevsW(&guid, null(), null_mut(), DIGCF_PRESENT | DIGCF_DEVICEINTERFACE);
        if set as isize == -1 {
            return out;
        }
        let mut i = 0;
        loop {
            let mut did: SP_DEVICE_INTERFACE_DATA = std::mem::zeroed();
            did.cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
            if SetupDiEnumDeviceInterfaces(set, null(), &guid, i, &mut did) == 0 {
                break;
            }
            i += 1;
            let mut req = 0u32;
            SetupDiGetDeviceInterfaceDetailW(set, &did, null_mut(), 0, &mut req, null_mut());
            if req == 0 {
                continue;
            }
            let mut buf = vec![0u64; (req as usize + 7) / 8];
            let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(set, &did, detail, req, null_mut(), null_mut()) == 0 {
                continue;
            }
            let p = (*detail).DevicePath.as_ptr();
            let mut len = 0;
            while *p.add(len) != 0 {
                len += 1;
            }
            let mut path: Vec<u16> = std::slice::from_raw_parts(p, len).to_vec();
            path.push(0);
            if let Some(info) = query(&path) {
                out.push(info);
            }
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    out
}

unsafe fn query(path: &[u16]) -> Option<HidInfo> {
    let h = CreateFileW(path.as_ptr(), 0, FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING, 0, null_mut());
    if h == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut attr: HIDD_ATTRIBUTES = std::mem::zeroed();
    attr.Size = std::mem::size_of::<HIDD_ATTRIBUTES>() as u32;
    let mut caps: HIDP_CAPS = std::mem::zeroed();
    let mut ok = HidD_GetAttributes(h, &mut attr) != 0;
    let mut pp: PHIDP_PREPARSED_DATA = 0;
    if ok && HidD_GetPreparsedData(h, &mut pp) != 0 {
        ok = HidP_GetCaps(pp, &mut caps) == HIDP_STATUS_SUCCESS;
        HidD_FreePreparsedData(pp);
    } else {
        ok = false;
    }
    let mut name = [0u16; 128];
    HidD_GetProductString(h, name.as_mut_ptr() as _, (name.len() * 2) as u32);
    CloseHandle(h);
    if !ok {
        return None;
    }
    let path_str = String::from_utf16_lossy(&path[..path.len() - 1]).to_lowercase();
    Some(HidInfo {
        path: path.to_vec(),
        vid: attr.VendorID,
        pid: attr.ProductID,
        usage_page: caps.UsagePage,
        usage: caps.Usage,
        in_len: caps.InputReportByteLength,
        out_len: caps.OutputReportByteLength,
        feat_len: caps.FeatureReportByteLength,
        product: wide_to_string(&name),
        interface: parse_interface(&path_str),
    })
}

/// Dispositivo HID abierto con E/S solapada (permite tiempos de espera).
pub struct HidDev {
    h: HANDLE,
    ev: HANDLE,
}

unsafe impl Send for HidDev {}

impl HidDev {
    /// Abre para input/output reports (E/S solapada con tiempo de espera).
    pub fn open(path: &[u16]) -> Option<HidDev> {
        Self::open_with(path, FILE_FLAG_OVERLAPPED)
    }

    /// Abre sólo para feature reports (HidD_Get/SetFeature necesitan un handle síncrono).
    pub fn open_feature(path: &[u16]) -> Option<HidDev> {
        Self::open_with(path, 0)
    }

    fn open_with(path: &[u16], flags: u32) -> Option<HidDev> {
        unsafe {
            let share = FILE_SHARE_READ | FILE_SHARE_WRITE;
            let mut h = CreateFileW(path.as_ptr(), GENERIC_READ | GENERIC_WRITE, share, null(), OPEN_EXISTING, flags, null_mut());
            if h == INVALID_HANDLE_VALUE {
                // Teclados y ratones: Windows no deja leer/escribir, pero sí usar feature reports.
                h = CreateFileW(path.as_ptr(), 0, share, null(), OPEN_EXISTING, flags, null_mut());
                if h == INVALID_HANDLE_VALUE {
                    return None;
                }
            }
            let ev = CreateEventW(null(), 1, 0, null());
            Some(HidDev { h, ev })
        }
    }

    fn overlapped(&self) -> OVERLAPPED {
        let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
        ov.hEvent = self.ev;
        ov
    }

    pub fn write(&self, data: &[u8]) -> bool {
        unsafe {
            let mut ov = self.overlapped();
            let mut n = 0u32;
            if WriteFile(self.h, data.as_ptr(), data.len() as u32, &mut n, &mut ov) == 0 {
                if GetLastError() != ERROR_IO_PENDING {
                    return false;
                }
                if WaitForSingleObject(self.ev, 1000) != WAIT_OBJECT_0 {
                    CancelIo(self.h);
                    GetOverlappedResult(self.h, &ov, &mut n, 1);
                    return false;
                }
                if GetOverlappedResult(self.h, &ov, &mut n, 0) == 0 {
                    return false;
                }
            }
            true
        }
    }

    /// Lee un input report; devuelve los bytes leídos o None si vence el tiempo.
    pub fn read(&self, buf: &mut [u8], timeout_ms: u32) -> Option<usize> {
        unsafe {
            let mut ov = self.overlapped();
            let mut n = 0u32;
            if ReadFile(self.h, buf.as_mut_ptr(), buf.len() as u32, &mut n, &mut ov) == 0 {
                if GetLastError() != ERROR_IO_PENDING {
                    return None;
                }
                if WaitForSingleObject(self.ev, timeout_ms) != WAIT_OBJECT_0 {
                    CancelIo(self.h);
                    GetOverlappedResult(self.h, &ov, &mut n, 1);
                    return None;
                }
                if GetOverlappedResult(self.h, &ov, &mut n, 0) == 0 {
                    return None;
                }
            }
            Some(n as usize)
        }
    }

    pub fn set_feature(&self, data: &[u8]) -> bool {
        unsafe { HidD_SetFeature(self.h, data.as_ptr() as _, data.len() as u32) != 0 }
    }

    pub fn get_feature(&self, buf: &mut [u8]) -> bool {
        unsafe { HidD_GetFeature(self.h, buf.as_mut_ptr() as _, buf.len() as u32) != 0 }
    }

    pub fn get_input_report(&self, buf: &mut [u8]) -> bool {
        unsafe { HidD_GetInputReport(self.h, buf.as_mut_ptr() as _, buf.len() as u32) != 0 }
    }

    /// Descriptor del dispositivo ya interpretado por Windows (para leer/escribir campos por "usage").
    pub fn preparsed(&self) -> Option<Preparsed> {
        unsafe {
            let mut pp: PHIDP_PREPARSED_DATA = 0;
            if HidD_GetPreparsedData(self.h, &mut pp) == 0 {
                return None;
            }
            let mut caps: HIDP_CAPS = std::mem::zeroed();
            HidP_GetCaps(pp, &mut caps);
            Some(Preparsed { pp, caps })
        }
    }
}

pub const INPUT: i32 = 0;
pub const FEATURE: i32 = 2;

/// Descriptor HID interpretado: permite localizar un campo (página/uso) y leerlo o escribirlo
/// en un report sin conocer su posición exacta.
pub struct Preparsed {
    pp: PHIDP_PREPARSED_DATA,
    pub caps: HIDP_CAPS,
}

/// Campo de valor del descriptor: report en que está, colección y rango lógico.
#[derive(Clone, Copy, Debug)]
pub struct ValueCap {
    pub report_id: u8,
    pub link: u16,
    pub min: i32,
    pub max: i32,
}

impl Preparsed {
    pub fn value_cap(&self, kind: i32, page: u16, usage: u16) -> Option<ValueCap> {
        unsafe {
            let mut caps: [HIDP_VALUE_CAPS; 8] = std::mem::zeroed();
            let mut n: u16 = caps.len() as u16;
            if HidP_GetSpecificValueCaps(kind, page, 0, usage, caps.as_mut_ptr(), &mut n, self.pp) != HIDP_STATUS_SUCCESS || n == 0 {
                return None;
            }
            let c = &caps[0];
            Some(ValueCap { report_id: c.ReportID, link: c.LinkCollection, min: c.LogicalMin, max: c.LogicalMax })
        }
    }

    /// Todos los campos con ese uso (p. ej. el mismo uso en varios reports).
    pub fn value_caps(&self, kind: i32, page: u16, usage: u16) -> Vec<ValueCap> {
        unsafe {
            let mut caps: [HIDP_VALUE_CAPS; 8] = std::mem::zeroed();
            let mut n: u16 = caps.len() as u16;
            if HidP_GetSpecificValueCaps(kind, page, 0, usage, caps.as_mut_ptr(), &mut n, self.pp) != HIDP_STATUS_SUCCESS {
                return Vec::new();
            }
            caps[..n as usize].iter().map(|c| ValueCap { report_id: c.ReportID, link: c.LinkCollection, min: c.LogicalMin, max: c.LogicalMax }).collect()
        }
    }

    pub fn get(&self, kind: i32, page: u16, cap: &ValueCap, usage: u16, report: &[u8]) -> Option<u32> {
        unsafe {
            let mut v: u32 = 0;
            (HidP_GetUsageValue(kind, page, cap.link, usage, &mut v, self.pp, report.as_ptr() as _, report.len() as u32) == HIDP_STATUS_SUCCESS).then_some(v)
        }
    }

    pub fn set(&self, kind: i32, page: u16, link: u16, usage: u16, value: u32, report: &mut [u8]) -> bool {
        unsafe { HidP_SetUsageValue(kind, page, link, usage, value, self.pp, report.as_mut_ptr() as _, report.len() as u32) == HIDP_STATUS_SUCCESS }
    }
}

impl Drop for Preparsed {
    fn drop(&mut self) {
        unsafe {
            HidD_FreePreparsedData(self.pp);
        }
    }
}

impl Drop for HidDev {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.h);
            CloseHandle(self.ev);
        }
    }
}
