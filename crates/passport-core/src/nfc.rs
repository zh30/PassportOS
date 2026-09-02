//! NTAG213 on this board is a **passive** Type 2 tag: phones talk RF, the MCU
//! has no wire. Apps still get a real NDEF encoder so a phone can write the
//! bytes this module produces.

use crate::api::ApiError;

/// Official chip: 36 user pages × 4 bytes, starting at page 4.
pub const NTAG213_USER_BYTES: u16 = 144;
pub const NTAG213_NAME: &str = "NTAG213";

/// Default URI a phone can store; matches the product site.
pub const DEFAULT_URI: &str = "https://ai-passport.folotoy.cn";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NfcTag {
    pub name: &'static str,
    pub user_bytes: u16,
    /// False on this PCB: no I2C/SPI/GPIO to the die.
    pub mcu_wired: bool,
}

pub const NTAG213: NfcTag = NfcTag {
    name: NTAG213_NAME,
    user_bytes: NTAG213_USER_BYTES,
    mcu_wired: false,
};

/// MCU has no bus to the tag. Callers must not pretend a read succeeded.
pub fn mcu_read(_page: u8, _dst: &mut [u8]) -> Result<usize, ApiError> {
    Err(ApiError::NoBus)
}

pub fn mcu_write(_page: u8, _src: &[u8]) -> Result<(), ApiError> {
    Err(ApiError::NoBus)
}

/// NFC Forum URI identifier codes (RTD-URI).
fn uri_prefix(url: &str) -> (u8, &str) {
    if let Some(rest) = url.strip_prefix("https://www.") {
        (0x02, rest)
    } else if let Some(rest) = url.strip_prefix("http://www.") {
        (0x01, rest)
    } else if let Some(rest) = url.strip_prefix("https://") {
        (0x04, rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (0x03, rest)
    } else {
        (0x00, url)
    }
}

/// Type 2 TLV: `03 <len> D1 01 <plen> 55 <code> <uri...> FE`.
pub fn encode_uri_tlv(url: &str, out: &mut [u8]) -> Result<usize, ApiError> {
    if url.is_empty() {
        return Err(ApiError::BadArg);
    }
    let (code, rest) = uri_prefix(url);
    let payload_len = 1 + rest.len();
    if payload_len > 255 {
        return Err(ApiError::BadArg);
    }
    let ndef_len = 4 + payload_len; // hdr + type_len + plen + 'U' + payload
    let total = 2 + ndef_len + 1; // TLV type+len + ndef + terminator
    if ndef_len > usize::from(NTAG213_USER_BYTES) {
        return Err(ApiError::Full);
    }
    if out.len() < total {
        return Err(ApiError::Full);
    }
    out[0] = 0x03;
    out[1] = ndef_len as u8;
    out[2] = 0xD1; // MB+ME+SR, TNF=well-known
    out[3] = 0x01;
    out[4] = payload_len as u8;
    out[5] = b'U';
    out[6] = code;
    out[7..7 + rest.len()].copy_from_slice(rest.as_bytes());
    out[7 + rest.len()] = 0xFE;
    Ok(total)
}

/// Inverse of [`encode_uri_tlv`] for host tests.
pub fn decode_uri_tlv(buf: &[u8]) -> Result<heapless::String<80>, ApiError> {
    if buf.len() < 8 || buf[0] != 0x03 || buf[2] != 0xD1 || buf[5] != b'U' {
        return Err(ApiError::BadArg);
    }
    let ndef_len = buf[1] as usize;
    let payload_len = buf[4] as usize;
    if payload_len == 0 || 4 + payload_len != ndef_len {
        return Err(ApiError::BadArg);
    }
    let rest_len = payload_len - 1;
    let start = 7;
    let end = start + rest_len;
    if buf.len() < end + 1 || buf[end] != 0xFE {
        return Err(ApiError::BadArg);
    }
    let rest = core::str::from_utf8(&buf[start..end]).map_err(|_| ApiError::BadArg)?;
    let prefix = match buf[6] {
        0x01 => "http://www.",
        0x02 => "https://www.",
        0x03 => "http://",
        0x04 => "https://",
        0x00 => "",
        _ => return Err(ApiError::BadArg),
    };
    let mut s = heapless::String::new();
    s.push_str(prefix).map_err(|_| ApiError::Full)?;
    s.push_str(rest).map_err(|_| ApiError::Full)?;
    Ok(s)
}

/// One-line console / status description.
pub fn describe() -> heapless::String<96> {
    let mut s = heapless::String::new();
    let _ = core::fmt::Write::write_fmt(
        &mut s,
        format_args!(
            "nfc {name} user={bytes}B mcu=none uri={uri}",
            name = NTAG213.name,
            bytes = NTAG213.user_bytes,
            uri = DEFAULT_URI
        ),
    );
    s
}
