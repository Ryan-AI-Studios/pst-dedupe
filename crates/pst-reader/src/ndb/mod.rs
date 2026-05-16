//! NDB (Node Database) Layer — MS-PST §2.2.2
//!
//! The NDB provides block-level storage organized as two B-trees:
//! - **NBT (Node BTree):** Maps Node IDs (NIDs) to block references
//! - **BBT (Block BTree):** Maps Block IDs (BIDs) to file offsets and sizes
//!
//! All higher layers (LTP, Messaging) read data through NDB.

pub mod btree;
pub mod block;
pub mod nid;
pub mod page;

pub use nid::NodeId;
pub use block::BlockId;
