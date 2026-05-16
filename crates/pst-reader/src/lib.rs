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
//!         let message = pst.read_message_properties(message_nid)?;
//!         println!(
//!             "  {} - {}",
//!             message.subject.unwrap_or_default(),
//!             message.sender_email.unwrap_or_default()
//!         );
//!     }
//! }
//! # Ok(())
//! # }
//! ```

pub mod crypto;
pub mod header;
pub mod ltp;
pub mod messaging;
pub mod ndb;

mod error;
pub use error::{PstError, Result};

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use header::PstHeader;
use ndb::btree::{BbtIndex, NbtIndex};
use ndb::NodeId;

/// A read-only handle to a PST file.
///
/// Holds the parsed header, in-memory NBT/BBT indexes, and a file handle
/// for on-demand block reads.
pub struct PstFile {
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
        let file = File::open(path.as_ref())?;
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
            reader,
            header,
            nbt,
            bbt,
        })
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
