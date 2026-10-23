//! Per-user Windows language preference for MSI installations.
//! HKCU\Software\lhtPDF\Language: REG_DWORD 0 = English, 1 = Vietnamese.
//! Portable mode does not access this preference.

#[cfg(target_os = "windows")]
pub fn read() -> Option<&'static str> {
    if crate::portable::data_dir().is_some() {
        return None;
    }

    use winsafe::{HKEY, RegistryValue, co::RRF};

    match HKEY::CURRENT_USER.RegGetValue(Some(r"Software\lhtPDF"), Some("Language"), RRF::RT_REG_DWORD).ok()? {
        RegistryValue::Dword(0) => Some("en"),
        RegistryValue::Dword(1) => Some("vi"),
        _ => None,
    }
}

#[cfg(not(target_os = "windows"))]
pub fn read() -> Option<&'static str> {
    None
}

#[cfg(target_os = "windows")]
pub fn write(language: &str) -> Result<(), String> {
    if crate::portable::data_dir().is_some() {
        return Ok(());
    }

    let value = match language {
        "en" => 0u32,
        "vi" => 1u32,
        _ => return Err(format!("Unsupported registry language: {language}")),
    };

    use winsafe::{
        HKEY, RegistryValue,
        co::{KEY, REG_OPTION},
    };

    let (key, _) = HKEY::CURRENT_USER
        .RegCreateKeyEx(r"Software\lhtPDF", None, REG_OPTION::NON_VOLATILE, KEY::SET_VALUE, None)
        .map_err(|error| format!("Cannot create language registry key: {error}"))?;

    key.RegSetValueEx(Some("Language"), RegistryValue::Dword(value)).map_err(|error| format!("Cannot save language to registry: {error}"))
}

#[cfg(not(target_os = "windows"))]
pub fn write(_language: &str) -> Result<(), String> {
    Ok(())
}
