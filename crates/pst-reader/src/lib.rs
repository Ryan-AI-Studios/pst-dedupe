//! # pst-reader
//!
//! Pure Rust implementation of a read-only PST (Personal Storage Table) parser,
//! built from the [MS-PST] specification.
//!
//! This crate supports **Unicode PST files only** (wVer >= 23). ANSI PSTs (wVer 14/15)
//! are detected and rejected with a clear error.
//!
//! ## Architecture
//!
//! The parser follows the three-layer structure of the MS-PST spec:
//!
//! 1. **NDB (Node Database)** - B-tree storage of nodes and blocks on disk
//! 2. **LTP (Lists, Tables, Properties)** - Structured property access built on NDB
//! 3. **Messaging** - Email folders, messages, and attachments built on LTP
//!
//! ## Usage
//!
//! ```no_run
//! use pst_reader::PstFile;
//!
//! # fn main() -> pst_reader::Result<()> {
//! let mut pst = PstFile::open("archive.pst")?;
//! println!("PST: {} bytes", pst.file_size());
//!
//! for folder in pst.folders()? {
//!     println!("Folder: {} ({} messages)", folder.path, folder.message_count);
//!
//!     for message_nid in folder.message_nids {
//!         // CLI dedup path (body truncated to 4KB preview):
//!         let message = pst.read_message_properties(message_nid)?;
//!         println!(
//!             "  {} - {}",
//!             message.subject.unwrap_or_default(),
//!             message.sender_email.unwrap_or_default()
//!         );
//!         // Desk extract path: `read_message_extract` (full body, DisplayCc/Bcc,
//!         // delivery time, optional HTML). Attachments: `list_attachments` +
//!         // `open_attachment_data` → `Read` stream into CAS.
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Desk extract surfaces (track 0018)
//!
//! | API | Role |
//! |---|---|
//! | `read_message_properties` | CLI Tier-2: body preview truncated to 4KB |
//! | `read_message_extract` / `ExtractedMessage` | Full body, DisplayTo/Cc/Bcc, submit + delivery time, optional HTML |
//! | `list_attachments` / `AttachmentInfo` | NID, filename, size, mime, method |
//! | `open_attachment_data` / `AttachmentDataReader` | `Read` over attach binary (leaf-block stream; no multi-GB `Vec` production path) |
//! | `filetime_to_rfc3339` | FILETIME → RFC3339 UTC helper |

pub mod crypto;
pub mod header;
pub mod ltp;
pub mod messaging;
pub mod ndb;

mod error;
pub use error::{PstError, Result};
pub use messaging::attachment::{AttachmentDataReader, AttachmentInfo, AttachmentMeta};
pub use messaging::folder::FolderInfo;
pub use messaging::message::{
    filetime_to_rfc3339, filetime_to_unix, ExtractedMessage, MessageProperties,
};
pub use ndb::NodeId;

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use header::PstHeader;
use ndb::btree::{BbtIndex, NbtIndex};

/// A read-only handle to a PST file.
///
/// Holds the parsed header, in-memory NBT/BBT indexes, and a file handle
/// for on-demand block reads.
pub struct PstFile {
    /// Original path (used to open independent handles for attachment streaming).
    pub(crate) path: Option<PathBuf>,
    pub(crate) reader: BufReader<File>,
    pub(crate) header: PstHeader,
    pub(crate) nbt: NbtIndex,
    pub(crate) bbt: BbtIndex,
}

impl PstFile {
    /// Open a PST file and build the NDB indexes.
    ///
    /// This reads the header and traverses the full NBT and BBT B-trees into memory.
    /// For a typical PST this takes milliseconds; for very large files (~50GB) it may
    /// take a few seconds.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_buf = path.as_ref().to_path_buf();
        let file = File::open(&path_buf)?;
        let mut reader = BufReader::with_capacity(64 * 1024, file);

        // Phase 1: Parse and validate header
        let header = PstHeader::read(&mut reader)?;
        tracing::info!(
            "Opened PST: version={}, crypto={:?}, size={}",
            header.version,
            header.crypt_method,
            header.root.ib_file_eof
        );

        // Phase 2: Build NBT index (traverse Node BTree from root)
        let nbt = NbtIndex::build(&mut reader, &header)?;
        tracing::info!("NBT loaded: {} nodes", nbt.len());

        // Phase 3: Build BBT index (traverse Block BTree from root)
        let bbt = BbtIndex::build(&mut reader, &header)?;
        tracing::info!("BBT loaded: {} blocks", bbt.len());

        Ok(Self {
            path: Some(path_buf),
            reader,
            header,
            nbt,
            bbt,
        })
    }

    /// Filesystem path this PST was opened from, when known.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// File size in bytes as recorded in the PST header.
    pub fn file_size(&self) -> u64 {
        self.header.root.ib_file_eof
    }

    /// The encryption method used for data blocks.
    pub fn crypt_method(&self) -> crypto::CryptMethod {
        self.header.crypt_method
    }

    /// Read raw data for a node by NID.
    ///
    /// Assembles data from single blocks or XBLOCK/XXBLOCK chains, decrypting as needed.
    pub fn read_node_data(&mut self, nid: NodeId) -> Result<Vec<u8>> {
        let nbt_entry = self.nbt.get(nid).ok_or(PstError::NodeNotFound(nid.0))?;

        if nbt_entry.bid_data.0 == 0 {
            return Ok(Vec::new());
        }

        ndb::block::read_block_data(
            &mut self.reader,
            &self.bbt,
            nbt_entry.bid_data,
            self.header.crypt_method,
        )
    }

    /// Read subnode BTree data for a node (if it has one).
    pub fn read_subnode_data(&mut self, nid: NodeId, sub_nid: NodeId) -> Result<Vec<u8>> {
        let nbt_entry = self.nbt.get(nid).ok_or(PstError::NodeNotFound(nid.0))?;

        if nbt_entry.bid_sub.0 == 0 {
            return Err(PstError::NoSubnodeBTree(nid.0));
        }

        ndb::block::read_subnode_data(
            &mut self.reader,
            &self.bbt,
            nbt_entry.bid_sub,
            sub_nid,
            self.header.crypt_method,
        )
    }

    /// Access the NDB index for direct lookups.
    pub fn nbt(&self) -> &NbtIndex {
        &self.nbt
    }

    /// Access the BBT index for direct lookups.
    pub fn bbt(&self) -> &BbtIndex {
        &self.bbt
    }
}
