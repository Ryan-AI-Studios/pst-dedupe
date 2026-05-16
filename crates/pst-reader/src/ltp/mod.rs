//! LTP (Lists, Tables & Properties) Layer — MS-PST §2.3
//!
//! Built on top of NDB nodes, LTP provides structured property storage:
//! - **HN (Heap-on-Node):** Heap allocator within a node's data
//! - **BTH (BTree-on-Heap):** B-tree stored inside an HN
//! - **PC (Property Context):** Key-value property storage using BTH
//! - **TC (Table Context):** Tabular data with typed columns

pub mod bth;
pub mod hn;
pub mod pc;
pub mod tc;
