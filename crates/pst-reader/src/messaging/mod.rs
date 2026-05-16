//! Messaging Layer — MS-PST §2.4
//!
//! The highest-level abstraction, providing folder traversal and message extraction
//! built on top of LTP property/table contexts.

pub mod attachment;
pub mod folder;
pub mod message;
pub mod store;
