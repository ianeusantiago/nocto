//! Estado do aparelho para a página Início e os avisos da pílula: bateria do notebook,
//! câmera/microfone em uso (com o nome do app) e bateria do fone Bluetooth.

use windows::core::{GUID, PCWSTR, PWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW, SetupDiGetDeviceInstanceIdW,
    SetupDiGetDevicePropertyW, SetupDiGetDeviceRegistryPropertyW, DIGCF_ALLCLASSES, DIGCF_PRESENT, SPDRP_FRIENDLYNAME,
    SP_DEVINFO_DATA,
};
use windows::Win32::Devices::Properties::DEVPROPTYPE;
use windows::Win32::Foundation::{DEVPROPKEY, ERROR_SUCCESS};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_QWORD,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
}

/// Bateria do notebook. `None` em PC de mesa (sem bateria).
pub fn battery() -> Option<Battery> {
    let mut s = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut s).ok()? };
    // 128 = sem bateria; 255 = desconhecido.
    if s.BatteryFlag & 128 != 0 || s.BatteryLifePercent > 100 {
        return None;
    }
    Some(Battery { percent: s.BatteryLifePercent, charging: s.ACLineStatus == 1 })
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn subkeys(key: HKEY) -> Vec<String> {
    let mut out = Vec::new();
    for i in 0.. {
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let r = unsafe { RegEnumKeyExW(key, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None) };
        if r != ERROR_SUCCESS {
            break;
        }
        out.push(String::from_utf16_lossy(&buf[..len as usize]));
    }
    out
}

fn qword(key: HKEY, sub: &str, value: &str) -> u64 {
    let (s, v) = (wide(sub), wide(value));
    let mut data = 0u64;
    let mut size = 8u32;
    let r = unsafe {
        RegGetValueW(key, PCWSTR(s.as_ptr()), PCWSTR(v.as_ptr()), RRF_RT_REG_QWORD, None, Some(&mut data as *mut u64 as _), Some(&mut size))
    };
    if r == ERROR_SUCCESS { data } else { 0 }
}

/// Nome curto do app a partir da chave do registro:
/// "Microsoft.WindowsCamera_8wekyb3d8bbwe" → "Câmera"; "C:#…#Discord.exe" → "Discord".
pub fn app_name(key: &str) -> String {
    let base = if key.contains('#') {
        let file = key.rsplit('#').next().unwrap_or(key);
        file.trim_end_matches(".exe").trim_end_matches(".EXE").to_string()
    } else {
        let pkg = key.split('_').next().unwrap_or(key);
        pkg.rsplit('.').next().unwrap_or(pkg).to_string()
    };
    match base.to_lowercase().as_str() {
        "windowscamera" => "Câmera".into(),
        "msedge" => "Edge".into(),
        "msedgewebview2" => "Navegador".into(),
        "ms-teams" | "teams" | "msteams" => "Teams".into(),
        "soundrecorder" => "Gravador".into(),
        _ => {
            let mut c = base.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}

/// Apps usando a câmera ("webcam") ou o microfone ("microphone") agora.
pub fn in_use(capability: &str) -> Vec<String> {
    let root = format!("Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\{capability}");
    let mut apps = Vec::new();
    let scan = |path: &str, apps: &mut Vec<String>| {
        let w = wide(path);
        let mut key = HKEY::default();
        if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(w.as_ptr()), None, KEY_READ, &mut key) } != ERROR_SUCCESS {
            return Vec::new();
        }
        let names = subkeys(key);
        let mut nested = Vec::new();
        for n in &names {
            if n == "NonPackaged" {
                nested.push(format!("{path}\\NonPackaged"));
                continue;
            }
            let start = qword(key, n, "LastUsedTimeStart");
            let stop = qword(key, n, "LastUsedTimeStop");
            if start != 0 && stop == 0 {
                let name = app_name(n);
                if !apps.contains(&name) {
                    apps.push(name);
                }
            }
        }
        unsafe {
            let _ = RegCloseKey(key);
        }
        nested
    };
    for n in scan(&root, &mut apps) {
        scan(&n, &mut apps);
    }
    apps
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Headset {
    pub name: String,
    pub percent: u8,
}

/// Bateria dos aparelhos Bluetooth conectados que informam isso ao Windows.
pub fn bluetooth_batteries() -> Vec<Headset> {
    // DEVPKEY_Bluetooth_Battery (o mesmo que aparece em Configurações > Bluetooth).
    let key = DEVPROPKEY {
        fmtid: GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5),
        pid: 2,
    };
    let mut out: Vec<Headset> = Vec::new();
    unsafe {
        let Ok(set) = SetupDiGetClassDevsW(None, PCWSTR::null(), None, DIGCF_PRESENT | DIGCF_ALLCLASSES) else {
            return out;
        };
        for i in 0.. {
            let mut info = SP_DEVINFO_DATA { cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32, ..Default::default() };
            if SetupDiEnumDeviceInfo(set, i, &mut info).is_err() {
                break;
            }
            let mut id = [0u16; 512];
            if SetupDiGetDeviceInstanceIdW(set, &info, Some(&mut id), None).is_err() {
                continue;
            }
            let id = String::from_utf16_lossy(&id).to_uppercase();
            if !id.starts_with("BTHENUM") && !id.starts_with("BTHLE") && !id.starts_with("BTHHFENUM") {
                continue;
            }
            let mut ty = DEVPROPTYPE::default();
            let mut val = [0u8; 4];
            if SetupDiGetDevicePropertyW(set, &info, &key, &mut ty, Some(&mut val), None, 0).is_err() {
                continue;
            }
            let pct = val[0].min(100);
            let mut name = [0u8; 512];
            let name = if SetupDiGetDeviceRegistryPropertyW(set, &info, SPDRP_FRIENDLYNAME, None, Some(&mut name), None).is_ok() {
                let w: Vec<u16> = name.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
                clean_name(&String::from_utf16_lossy(&w))
            } else {
                "Fone".into()
            };
            if !out.iter().any(|h| h.name == name) {
                out.push(Headset { name, percent: pct });
            }
        }
        let _ = SetupDiDestroyDeviceInfoList(set);
    }
    out
}

/// "JBL Tune 510BT Hands-Free AG Audio" → "JBL Tune 510BT".
fn clean_name(s: &str) -> String {
    let mut n = s.trim().to_string();
    for tail in [" Hands-Free AG Audio", " Hands-Free AG", " Hands-Free", " Stereo", " Avrcp Transport", " AVRCP Transport"] {
        if let Some(i) = n.find(tail) {
            n.truncate(i);
        }
    }
    n.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(app_name("C:#Users#ian#AppData#Local#Discord#app-1.0#Discord.exe"), "Discord");
        assert_eq!(app_name("Microsoft.WindowsCamera_8wekyb3d8bbwe"), "Câmera");
        assert_eq!(app_name("C:#Program Files#Brave#brave.exe"), "Brave");
        assert_eq!(clean_name("JBL Tune 510BT Hands-Free AG Audio"), "JBL Tune 510BT");
    }

    #[test]
    #[ignore]
    fn live() {
        println!("bateria: {:?}", battery());
        println!("camera: {:?}", in_use("webcam"));
        println!("microfone: {:?}", in_use("microphone"));
        println!("bluetooth: {:?}", bluetooth_batteries());
    }
}
