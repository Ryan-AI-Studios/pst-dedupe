//! NDB-layer encryption and decryption (MS-PST section 5.1).
//!
//! PST data blocks (not pages, not internal blocks) may be encrypted with one of:
//! - `NDB_CRYPT_NONE` (0x00): no encryption
//! - `NDB_CRYPT_PERMUTE` (0x01): byte substitution cipher
//! - `NDB_CRYPT_CYCLIC` (0x02): XOR cipher keyed on block ID

use crate::error::{PstError, Result};

/// Encryption method used for data blocks in this PST file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CryptMethod {
    None,
    Permute,
    Cyclic,
}

impl CryptMethod {
    pub fn from_byte(b: u8) -> Result<Self> {
        match b {
            0x00 => Ok(CryptMethod::None),
            0x01 => Ok(CryptMethod::Permute),
            0x02 => Ok(CryptMethod::Cyclic),
            _ => Err(PstError::UnsupportedCryptMethod(b)),
        }
    }
}

/// Decrypt a data block payload in-place.
///
/// `bid` is required for cyclic decryption (used to derive the XOR key).
/// Only external data block payloads are encrypted; internal blocks
/// (XBLOCK, XXBLOCK, SLBLOCK, SIBLOCK) and pages are never encrypted.
pub fn decrypt_block(data: &mut [u8], method: CryptMethod, bid: u64) {
    match method {
        CryptMethod::None => {}
        CryptMethod::Permute => decrypt_permute(data),
        CryptMethod::Cyclic => decrypt_cyclic(data, bid),
    }
}

/// NDB_CRYPT_PERMUTE decryption: byte substitution using `mpbbI`.
///
/// MS-PST section 5.1 defines a 768-byte `mpbbCrypt` table. Encoding uses the
/// first 256 bytes (`mpbbR`); decoding uses the third 256 bytes (`mpbbI`).
fn decrypt_permute(data: &mut [u8]) {
    for byte in data.iter_mut() {
        *byte = MPBB_DECODE[*byte as usize];
    }
}

/// NDB_CRYPT_CYCLIC decryption: XOR with a 4-byte key derived from the BID.
///
/// Key derivation: `key = (bid as u32) ^ ((bid >> 16) as u32)`.
/// Then XOR each byte with `key_bytes[offset % 4]`.
fn decrypt_cyclic(data: &mut [u8], bid: u64) {
    let w = (bid as u32) ^ ((bid >> 16) as u32);
    let key_bytes = w.to_le_bytes();

    for (i, byte) in data.iter_mut().enumerate() {
        *byte ^= key_bytes[i % 4];
    }
}

/// Encode table from MS-PST section 5.1 (`mpbbR`, `mpbbCrypt[0..256]`).
#[rustfmt::skip]
const MPBB_ENCODE: [u8; 256] = [
     65,  54,  19,  98, 168,  33, 110, 187,
    244,  22, 204,   4, 127, 100, 232,  93,
     30, 242, 203,  42, 116, 197,  94,  53,
    210, 149,  71, 158, 150,  45, 154, 136,
     76, 125, 132,  63, 219, 172,  49, 182,
     72,  95, 246, 196, 216,  57, 139, 231,
     35,  59,  56, 142, 200, 193, 223,  37,
    177,  32, 165,  70,  96,  78, 156, 251,
    170, 211,  86,  81,  69, 124,  85,   0,
      7, 201,  43, 157, 133, 155,   9, 160,
    143, 173, 179,  15,  99, 171, 137,  75,
    215, 167,  21,  90, 113, 102,  66, 191,
     38,  74, 107, 152, 250, 234, 119,  83,
    178, 112,   5,  44, 253,  89,  58, 134,
    126, 206,   6, 235, 130, 120,  87, 199,
    141,  67, 175, 180,  28, 212,  91, 205,
    226, 233,  39,  79, 195,   8, 114, 128,
    207, 176, 239, 245,  40, 109, 190,  48,
     77,  52, 146, 213,  14,  60,  34,  50,
    229, 228, 249, 159, 194, 209,  10, 129,
     18, 225, 238, 145, 131, 118, 227, 151,
    230,  97, 138,  23, 121, 164, 183, 220,
    144, 122,  92, 140,   2, 166, 202, 105,
    222,  80,  26,  17, 147, 185,  82, 135,
     88, 252, 237,  29,  55,  73,  27, 106,
    224,  41,  51, 153, 189, 108, 217, 148,
    243,  64,  84, 111, 240, 198, 115, 184,
    214,  62, 101,  24,  68,  31, 221, 103,
     16, 241,  12,  25, 236, 174,   3, 161,
     20, 123, 169,  11, 255, 248, 163, 192,
    162,   1, 247,  46, 188,  36, 104, 117,
     13, 254, 186,  47, 181, 208, 218,  61,
];

/// Decode table from MS-PST section 5.1 (`mpbbI`, `mpbbCrypt[512..768]`).
#[rustfmt::skip]
const MPBB_DECODE: [u8; 256] = [
     71, 241, 180, 230,  11, 106, 114,  72,
    133,  78, 158, 235, 226, 248, 148,  83,
    224, 187, 160,   2, 232,  90,   9, 171,
    219, 227, 186, 198, 124, 195,  16, 221,
     57,   5, 150,  48, 245,  55,  96, 130,
    140, 201,  19,  74, 107,  29, 243, 251,
    143,  38, 151, 202, 145,  23,   1, 196,
     50,  45, 110,  49, 149, 255, 217,  35,
    209,   0,  94, 121, 220,  68,  59,  26,
     40, 197,  97,  87,  32, 144,  61, 131,
    185,  67, 190, 103, 210,  70,  66, 118,
    192, 109,  91, 126, 178,  15,  22,  41,
     60, 169,   3,  84,  13, 218,  93, 223,
    246, 183, 199,  98, 205, 141,   6, 211,
    105,  92, 134, 214,  20, 247, 165, 102,
    117, 172, 177, 233,  69,  33, 112,  12,
    135, 159, 116, 164,  34,  76, 111, 191,
     31,  86, 170,  46, 179, 120,  51,  80,
    176, 163, 146, 188, 207,  25,  28, 167,
     99, 203,  30,  77,  62,  75,  27, 155,
     79, 231, 240, 238, 173,  58, 181,  89,
      4, 234,  64,  85,  37,  81, 229, 122,
    137,  56, 104,  82, 123, 252,  39, 174,
    215, 189, 250,   7, 244, 204, 142,  95,
    239,  53, 156, 132,  43,  21, 213, 119,
     52,  73, 182,  18,  10, 127, 113, 136,
    253, 157,  24,  65, 125, 147, 216,  88,
     44, 206, 254,  36, 175, 222, 184,  54,
    200, 161, 128, 166, 153, 152, 168,  47,
     14, 129, 101, 115, 228, 194, 162, 138,
    212, 225,  17, 208,   8, 139,  42, 242,
    237, 154, 100,  63, 193, 108, 249, 236,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permute_roundtrip() {
        let original = b"Hello, PST world!";
        let mut data = original.to_vec();

        for byte in data.iter_mut() {
            *byte = MPBB_ENCODE[*byte as usize];
        }

        decrypt_permute(&mut data);

        assert_eq!(&data, original);
    }

    #[test]
    fn test_cyclic_roundtrip() {
        let original = b"Test data for cyclic encryption";
        let mut data = original.to_vec();
        let bid: u64 = 0x0000_0042_0000_0084;

        decrypt_cyclic(&mut data, bid);
        decrypt_cyclic(&mut data, bid);

        assert_eq!(&data, original);
    }

    #[test]
    fn test_crypt_method_from_byte() {
        assert_eq!(CryptMethod::from_byte(0).unwrap(), CryptMethod::None);
        assert_eq!(CryptMethod::from_byte(1).unwrap(), CryptMethod::Permute);
        assert_eq!(CryptMethod::from_byte(2).unwrap(), CryptMethod::Cyclic);
        assert!(CryptMethod::from_byte(3).is_err());
    }
}
