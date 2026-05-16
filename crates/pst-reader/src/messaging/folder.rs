//! Folder hierarchy traversal — MS-PST §2.4.4
//!
//! Folders are accessed via their hierarchy and contents Table Contexts.
//! The root folder (NID 0x122) is the entry point.

use crate::error::Result;
use crate::ltp::{pc, tc};
use crate::ndb::nid::{self, NodeId};
use crate::PstFile;

/// A folder descriptor with enough info for traversal and dedup.
#[derive(Debug, Clone)]
pub struct FolderInfo {
    /// This folder's NID.
    pub nid: NodeId,
    /// Display name.
    pub name: String,
    /// Full path (e.g., "Root/Inbox/Projects").
    pub path: String,
    /// Number of messages (counted from contents TC).
    pub message_count: u32,
    /// NIDs of child folders.
    pub child_folder_nids: Vec<NodeId>,
    /// NIDs of messages in this folder.
    pub message_nids: Vec<NodeId>,
}

impl PstFile {
    /// Walk the complete folder hierarchy and return a flat list of all folders.
    pub fn folders(&mut self) -> Result<Vec<FolderInfo>> {
        let mut results = Vec::new();
        self.walk_folder(nid::NID_ROOT_FOLDER, String::new(), &mut results)?;
        Ok(results)
    }

    fn walk_folder(
        &mut self,
        folder_nid: NodeId,
        parent_path: String,
        results: &mut Vec<FolderInfo>,
    ) -> Result<()> {
        // Read folder name from its Property Context
        let name = {
            let crypt = self.header.crypt_method;
            match pc::load_pc(&mut self.reader, &self.nbt, &self.bbt, folder_nid, crypt) {
                Ok(prop_ctx) => prop_ctx
                    .get_string(nid::PID_TAG_DISPLAY_NAME)?
                    .unwrap_or_else(|| format!("[Folder 0x{:X}]", folder_nid.0)),
                Err(_) => format!("[Folder 0x{:X}]", folder_nid.0),
            }
        };

        let path = if parent_path.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", parent_path, name)
        };

        // Read hierarchy table for child folders
        let hierarchy_nid = folder_nid.hierarchy_table();
        let child_folder_nids = if self.nbt.get(hierarchy_nid).is_some() {
            let crypt = self.header.crypt_method;
            match tc::load_tc(&mut self.reader, &self.nbt, &self.bbt, hierarchy_nid, crypt) {
                Ok(table) => {
                    let mut nids = Vec::new();
                    for row in 0..table.row_count() {
                        if let Some(nid_val) = table.get_row_u32(row, nid::PID_TAG_LTP_ROW_ID) {
                            nids.push(NodeId(nid_val as u64));
                        }
                    }
                    nids
                }
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };

        // Read contents table for message NIDs
        let contents_nid = folder_nid.contents_table();
        let message_nids = if self.nbt.get(contents_nid).is_some() {
            let crypt = self.header.crypt_method;
            match tc::load_tc(&mut self.reader, &self.nbt, &self.bbt, contents_nid, crypt) {
                Ok(table) => {
                    let mut nids = Vec::new();
                    for row in 0..table.row_count() {
                        if let Some(nid_val) = table.get_row_u32(row, nid::PID_TAG_LTP_ROW_ID) {
                            nids.push(NodeId(nid_val as u64));
                        }
                    }
                    nids
                }
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };

        let info = FolderInfo {
            nid: folder_nid,
            name,
            path: path.clone(),
            message_count: message_nids.len() as u32,
            child_folder_nids: child_folder_nids.clone(),
            message_nids,
        };

        results.push(info);

        // Recurse into children
        for child_nid in child_folder_nids {
            self.walk_folder(child_nid, path.clone(), results)?;
        }

        Ok(())
    }
}
