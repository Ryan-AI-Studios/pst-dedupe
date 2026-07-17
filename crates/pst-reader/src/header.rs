//! PST file header parsing (MS-PST §2.2.2).
//!
//! The header occupies the first 564 bytes of the file (Unicode PST), padded to 4096.
//! It contains magic bytes, format version, encryption method, and the ROOT structure
//! which provides entry points to the NDB B-trees.

use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{Read, Seek, SeekFrom};

use crate::crypto::CryptMethod;
use crate::error::{PstError, Result};

/// Magic bytes: "!BDN" read as little-endian u32.
const PST_MAGIC: u32 = 0x4E444221;

/// Client magic: "SM" = 0x4D53
const CLIENT_MAGIC: u16 = 0x4D53;

/// Minimum wVer for Unicode PST format.
const MIN_UNICODE_VERSION: u16 = 23;

/// Parsed PST file header.
#[derive(Debug, Clone)]
pub struct PstHeader {
    /// Format version (23 or 36 for Unicode).
    pub version: u16,
    /// Client version.
    pub ver_client: u16,
    /// Encryption method for data blocks.
    pub crypt_method: CryptMethod,
    /// ROOT structure — NDB entry points and file size.
    pub root: RootStructure,
    /// Next block ID counter.
    pub bid_next_b: u64,
}

/// ROOT structure (MS-PST §2.2.2.6) — 72 bytes for Unicode.
#[derive(Debug, Clone)]
pub struct RootStructure {
    /// Total file size in bytes.
    pub ib_file_eof: u64,
    /// Byte offset of last AMap page.
    pub ib_amap_last: u64,
    /// Free space tracked by AMaps.
    pub cb_amap_free: u64,
    /// BREF to root page of the Node BTree.
    pub bref_nbt: Bref,
    /// BREF to root page of the Block BTree.
    pub bref_bbt: Bref,
    /// Whether the AMap is valid.
    pub f_amap_valid: bool,
}

/// Block Reference (BREF, §2.2.2.4) — 16 bytes for Unicode.
#[derive(Debug, Clone, Copy)]
pub struct Bref {
    /// Block ID.
    pub bid: u64,
    /// Absolute byte offset in the PST file.
    pub ib: u64,
}

impl Bref {
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let bid = reader.read_u64::<LittleEndian>()?;
        let ib = reader.read_u64::<LittleEndian>()?;
        Ok(Self { bid, ib })
    }
}

impl PstHeader {
    /// Read and validate the PST header from the start of the file.
    pub fn read<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        reader.seek(SeekFrom::Start(0))?;

        // dwMagic (offset 0, 4 bytes)
        let magic = reader.read_u32::<LittleEndian>()?;
        if magic != PST_MAGIC {
            return Err(PstError::InvalidMagic(magic));
        }

        // dwCRCPartial (offset 4, 4 bytes) — skip for now
        let _crc_partial = reader.read_u32::<LittleEndian>()?;

        // wMagicClient (offset 8, 2 bytes)
        let client_magic = reader.read_u16::<LittleEndian>()?;
        if client_magic != CLIENT_MAGIC {
            return Err(PstError::InvalidClientMagic(client_magic));
        }

        // wVer (offset 10, 2 bytes)
        let version = reader.read_u16::<LittleEndian>()?;
        if version < MIN_UNICODE_VERSION {
            return Err(PstError::AnsiPstNotSupported(version));
        }

        // wVerClient (offset 12, 2 bytes)
        let ver_client = reader.read_u16::<LittleEndian>()?;

        // bPlatformCreate (1) + bPlatformAccess (1) + dwReserved1 (4) + dwReserved2 (4)
        // + bidUnused (8) + bidNextP (8) + dwUnique (4) = 30 bytes
        let mut skip_buf = [0u8; 30];
        reader.read_exact(&mut skip_buf)?;

        // rgnid[32] — 128 bytes of NID counters, skip
        let mut rgnid_buf = [0u8; 128];
        reader.read_exact(&mut rgnid_buf)?;

        // qwUnused (8 bytes)
        let _unused = reader.read_u64::<LittleEndian>()?;

        // ROOT structure (Unicode offset 0xB4 / 180, 72 bytes)
        let root = Self::read_root(reader)?;

        // dwAlign (4 bytes) — ends at 0x100
        let _align = reader.read_u32::<LittleEndian>()?;

        // Unicode: rgbFM (128) + rgbFP (128) = 256 bytes — ends at 0x200.
        // (Older code skipped 508 bytes and misaligned bCryptMethod.)
        let mut fm_fp_buf = [0u8; 256];
        reader.read_exact(&mut fm_fp_buf)?;

        // bSentinel (offset 0x200) — should be 0x80
        let _sentinel = reader.read_u8()?;

        // bCryptMethod (offset 0x201)
        let crypt_byte = reader.read_u8()?;
        let crypt_method = CryptMethod::from_byte(crypt_byte)?;

        // rgbReserved (2 bytes, offset 0x202)
        let _reserved = reader.read_u16::<LittleEndian>()?;

        // bidNextB (8 bytes, Unicode, offset 0x204)
        let bid_next_b = reader.read_u64::<LittleEndian>()?;

        Ok(Self {
            version,
            ver_client,
            crypt_method,
            root,
            bid_next_b,
        })
    }

    /// Parse the ROOT structure (72 bytes, Unicode).
    fn read_root<R: Read>(reader: &mut R) -> Result<RootStructure> {
        // dwReserved (4 bytes)
        let _reserved = reader.read_u32::<LittleEndian>()?;

        let ib_file_eof = reader.read_u64::<LittleEndian>()?;
        let ib_amap_last = reader.read_u64::<LittleEndian>()?;
        let cb_amap_free = reader.read_u64::<LittleEndian>()?;

        // cbPMapFree (8 bytes, deprecated)
        let _pmap_free = reader.read_u64::<LittleEndian>()?;

        let bref_nbt = Bref::read(reader)?;
        let bref_bbt = Bref::read(reader)?;

        // MS-PST §2.2.2.5 ROOT (Unicode, 72 bytes total):
        // fAMapValid (1) + bReserved (1) + wReserved (2)
        let f_amap_valid = reader.read_u8()? != 0;
        let _b_reserved = reader.read_u8()?;
        let _w_reserved = reader.read_u16::<LittleEndian>()?;

        Ok(RootStructure {
            ib_file_eof,
            ib_amap_last,
            cb_amap_free,
            bref_nbt,
            bref_bbt,
            f_amap_valid,
        })
    }
}
