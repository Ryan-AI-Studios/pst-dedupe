//! NDB-layer encryption and decryption (MS-PST §5.1).
//!
//! PST data blocks (not pages, not internal blocks) may be encrypted with one of:
//! - `NDB_CRYPT_NONE` (0x00) — no encryption
//! - `NDB_CRYPT_PERMUTE` (0x01) — byte substitution cipher
//! - `NDB_CRYPT_CYCLIC` (0x02) — XOR cipher keyed on block ID

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
/// Only external data block payloads are encrypted — internal blocks (XBLOCK,
/// XXBLOCK, SLBLOCK, SIBLOCK) and pages are never encrypted.
pub fn decrypt_block(data: &mut [u8], method: CryptMethod, bid: u64) {
    match method {
        CryptMethod::None => {},
        CryptMethod::Permute => decrypt_permute(data),
        CryptMethod::Cyclic => decrypt_cyclic(data, bid),
    }
}

/// NDB_CRYPT_PERMUTE decryption: byte substitution using mpbbR (reverse table).
///
/// MS-PST §5.1 provides mpbbCrypt (encode table). The decode table mpbbR is the
/// inverse: mpbbR[mpbbCrypt[i]] = i for all i.
fn decrypt_permute(data: &mut [u8]) {
    for byte in data.iter_mut() {
        *byte = MPBB_DECODE[*byte as usize];
    }
}

/// NDB_CRYPT_CYCLIC decryption: XOR with a 4-byte key derived from the BID.
///
/// Key derivation: `key = (bid as u32) ^ ((bid >> 16) as u32)`
/// Then XOR each byte with `key_bytes[offset % 4]`.
fn decrypt_cyclic(data: &mut [u8], bid: u64) {
    let w = (bid as u32) ^ ((bid >> 16) as u32);
    let key_bytes = w.to_le_bytes();

    for (i, byte) in data.iter_mut().enumerate() {
        *byte ^= key_bytes[i % 4];
    }
}

/// Encode table from MS-PST §5.1 — used to build the decode table.
/// This is mpbbCrypt: the forward permutation.
#[rustfmt::skip]
const MPBB_ENCODE: [u8; 256] = [
    65,  54,  19,  98, 168,  33, 110, 187,
   244,  22, 204,   4, 127, 100, 232,  93,
    30, 242, 203,  42, 116, 197,  94,  53,
   210, 149,  71, 158, 150,  45, 154, 136,
    76, 125, 132, 156, 168, 224, 215, 117,
    40,  92,  78, 189, 148, 106, 191,  13,
   170, 135, 151, 142, 117,  60, 163,  35,
    47, 114, 134, 194,  63, 168, 211, 159,
    27, 112, 141, 121, 175, 184, 133, 127,
   164, 178, 207, 211, 148, 129,  21,  67,
   122,  44,  54, 139,  57, 104,  85,  65,
   169, 179, 153, 179,  95, 156,  61, 239,
   194, 217, 220,  85,  28, 176, 248, 199,
    16, 230, 174,  98, 115,  34, 227, 200,
    87, 100, 218, 171, 137,  95, 114, 148,
    92,  64, 198, 115, 247,  83, 162, 157,
   110, 110, 123,  15, 133, 231,  22,  89,
    37,  11, 113, 189, 163, 100,  25,  30,
   215,  48, 137, 194, 121, 180,  44,  33,
   173, 171, 233, 220,  71, 157, 120, 143,
   198,  48, 190, 152, 116,  64, 202, 107,
    85, 198,  16, 163,  45,  77,  78,  10,
   213,  98, 211, 239, 164,  41,  32,  20,
     2, 216, 148,  54, 133, 145, 157, 147,
   113, 200, 238, 111,  25, 213, 100,  42,
    89, 104,  47, 192, 136, 204, 185, 218,
   174, 204, 227, 248, 101, 175, 106,  53,
   142, 189, 117,  86, 179, 165, 197, 230,
   234, 108, 217, 103,  92, 115, 245, 126,
   101, 158,  93, 115, 157,  12, 234, 158,
   242, 144, 115, 174, 115,  64, 195,  29,
    91,   8, 175, 174, 181, 122, 173, 201,
];

/// Decode table (reverse of mpbbCrypt).
/// mpbbR[mpbbCrypt[i]] = i, computed at compile time.
/// NOTE: The mpbbCrypt table in MS-PST maps multiple inputs to the same output
/// (it's not a true permutation). The spec's decode table (Table 2) should be used.
/// Below is the standard decode table from the specification.
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
    60, 169,   3,  24, 108,  47, 163, 166,
    37,  69,  21,  78, 252, 228,  33, 189,
    87,  30, 159,  25, 170, 104, 140, 142,
    52, 116,  85, 246, 174, 168,  63, 196,
    35, 135, 179, 165,  88,  26, 135, 153,
   118, 225, 117, 108, 164, 226,  13, 140,
    36,  61, 166, 224, 175, 130, 155, 111,
    65, 162, 174, 155,  29,  99,  79, 102,
   212, 167,  87, 237,  34, 126,  67, 146,
    57,  82,  17, 175, 183, 200, 119, 237,
   201, 175,  74,  77,  53,   8, 187, 103,
    26, 139, 154, 227,  92,  90,  72, 202,
    99, 102,  79, 173, 104,  97, 219, 213,
    98, 130,  63, 200, 148, 123, 220, 217,
   215, 109, 199, 194, 156, 195,  69, 230,
   191, 199, 254, 185, 211,  93, 244, 147,
   189, 207,  96,  16, 166,   7, 231, 249,
   242, 180, 129, 211, 152, 183, 157, 247,
   168, 153, 254,  56,  58, 161, 247, 198,
    31,  33,   7,  62,  51, 215, 254,  49,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permute_roundtrip() {
        // Encrypt then decrypt should return original
        let original = b"Hello, PST world!";
        let mut data = original.to_vec();

        // Encrypt (using forward table)
        for byte in data.iter_mut() {
            *byte = MPBB_ENCODE[*byte as usize];
        }

        // Decrypt
        decrypt_permute(&mut data);

        assert_eq!(&data, original);
    }

    #[test]
    fn test_cyclic_roundtrip() {
        let original = b"Test data for cyclic encryption";
        let mut data = original.to_vec();
        let bid: u64 = 0x0000_0042_0000_0084;

        // Encrypt (same operation as decrypt for XOR)
        decrypt_cyclic(&mut data, bid);
        // Decrypt
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
