use windows_sys::core::GUID;

/// Decodes a NUL-terminated UTF-16 buffer.
pub fn from_wide(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

pub fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Formats a GUID the way `GetAdaptersAddresses` reports `AdapterName`:
/// `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}` in upper case.
pub fn guid_string(guid: &GUID) -> String {
    let d4 = guid.data4;
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        guid.data1, guid.data2, guid.data3, d4[0], d4[1], d4[2], d4[3], d4[4], d4[5], d4[6], d4[7]
    )
}
