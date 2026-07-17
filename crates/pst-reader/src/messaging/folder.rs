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
                        // Prefer RowIndex BTH RowID (child NID). Fall back to
                        // PidTagLtpRowId column when present.
                        let nid_val = table
                            .get_row_id(row)
                            .or_else(|| table.get_row_u32(row, nid::PID_TAG_LTP_ROW_ID));
                        if let Some(nid_val) = nid_val {
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
                        let nid_val = table
                            .get_row_id(row)
                            .or_else(|| table.get_row_u32(row, nid::PID_TAG_LTP_ROW_ID));
                        if let Some(nid_val) = nid_val {
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

#[cfg(test)]
mod diagnose_real_pst {
    use super::*;
    use crate::ltp::tc;
    use crate::ndb::nid::NidType;
    use std::collections::BTreeMap;
    use std::path::Path;

    /// Smoke-diagnose a real desktop PST without asserting product readiness.
    ///
    /// ```powershell
    /// $env:PST_DIAG_PATH = 'C:\Users\RyanB\Desktop\INC0102784-2.pst'
    /// cargo test -p pst-reader diagnose_desktop_pst -- --nocapture --ignored
    /// ```
    #[test]
    #[ignore = "manual real-PST diagnosis; set PST_DIAG_PATH"]
    fn diagnose_desktop_pst() {
        let path = std::env::var("PST_DIAG_PATH").unwrap_or_else(|_| {
            r"C:\Users\RyanB\Desktop\INC0102784-2.pst".to_string()
        });
        assert!(
            Path::new(&path).exists(),
            "PST not found at {path}; set PST_DIAG_PATH"
        );

        let mut pst = PstFile::open(&path).expect("open");
        println!(
            "open ok: size={} nbt={} bbt={} crypt={:?}",
            pst.file_size(),
            pst.nbt.len(),
            pst.bbt.len(),
            pst.crypt_method()
        );

        let mut by_type: BTreeMap<u8, u64> = BTreeMap::new();
        let mut folders = 0u64;
        let mut messages = 0u64;
        let mut hierarchy_tables = 0u64;
        let mut contents_tables = 0u64;
        for (nid_key, entry) in pst.nbt.iter() {
            let nid = NodeId(*nid_key);
            let t = (nid.0 & 0x1F) as u8;
            *by_type.entry(t).or_default() += 1;
            match nid.nid_type() {
                NidType::NormalFolder | NidType::SearchFolder => folders += 1,
                NidType::NormalMessage => messages += 1,
                NidType::HierarchyTable => hierarchy_tables += 1,
                NidType::ContentsTable => contents_tables += 1,
                _ => {}
            }
            let _ = entry;
        }
        println!("NBT type histogram:");
        for (t, c) in &by_type {
            println!("  type 0x{t:02X}: {c}");
        }
        println!(
            "counts: folders={folders} messages={messages} hierarchy_tc={hierarchy_tables} contents_tc={contents_tables}"
        );

        let root = nid::NID_ROOT_FOLDER;
        println!(
            "root 0x{:X} present={} parent_hint",
            root.0,
            pst.nbt.get(root).is_some()
        );
        if let Some(e) = pst.nbt.get(root) {
            println!(
                "  root bid_data={:?} bid_sub={:?} nid_parent={}",
                e.bid_data, e.bid_sub, e.nid_parent
            );
        }
        let hier = root.hierarchy_table();
        let cont = root.contents_table();
        println!(
            "  hierarchy 0x{:X} present={}",
            hier.0,
            pst.nbt.get(hier).is_some()
        );
        println!(
            "  contents  0x{:X} present={}",
            cont.0,
            pst.nbt.get(cont).is_some()
        );

        if let Some(e) = pst.nbt.get(hier) {
            println!(
                "  hierarchy bid_data={:?} bid_sub={:?}",
                e.bid_data, e.bid_sub
            );
            match crate::ndb::block::read_block_data(
                &mut pst.reader,
                &pst.bbt,
                e.bid_data,
                pst.header.crypt_method,
            ) {
                Ok(raw) => {
                    println!(
                        "  hierarchy raw len={} first32={}",
                        raw.len(),
                        raw.iter()
                            .take(32)
                            .map(|b| format!("{b:02X}"))
                            .collect::<Vec<_>>()
                            .join(" ")
                    );
                    if raw.len() >= 12 {
                        println!(
                            "  HN hdr: ibHnpm=0x{:04X} sig=0x{:02X} client=0x{:02X} hidUserRoot=0x{:08X}",
                            u16::from_le_bytes([raw[0], raw[1]]),
                            raw[2],
                            raw[3],
                            u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]])
                        );
                    }
                }
                Err(err) => println!("  hierarchy raw read ERR: {err}"),
            }
            match tc::load_tc(
                &mut pst.reader,
                &pst.nbt,
                &pst.bbt,
                hier,
                pst.header.crypt_method,
            ) {
                Ok(table) => {
                    println!(
                        "  hierarchy TC rows={} cols={} row_size_hint",
                        table.row_count(),
                        table.columns().len()
                    );
                    for col in table.columns() {
                        println!(
                            "    col prop=0x{:04X} type=0x{:04X} ib={} cb={} ibit={}",
                            col.prop_id, col.prop_type, col.ib_data, col.cb_data, col.i_bit
                        );
                    }
                    let max = table.row_count().min(10);
                    for row in 0..max {
                        let row_id = table.get_row_u32(row, nid::PID_TAG_LTP_ROW_ID);
                        let display = table
                            .get_row_string(row, nid::PID_TAG_DISPLAY_NAME)
                            .ok()
                            .flatten();
                        println!("    row[{row}] LtpRowId={row_id:?} DisplayName={display:?}");
                        // dump first 4-byte-looking props if LtpRowId missing
                        if row_id.is_none() {
                            for col in table.columns() {
                                if col.cb_data == 4 {
                                    if let Some(v) = table.get_row_u32(row, col.prop_id) {
                                        println!("      u32 prop 0x{:04X} = 0x{v:08X} ({v})", col.prop_id);
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => println!("  hierarchy TC load ERR: {e}"),
            }
        }

        // Sample a few folder NIDs from NBT and try their hierarchy/contents.
        let mut sample = 0;
        for (nid_key, _) in pst.nbt.iter() {
            let nid = NodeId(*nid_key);
            if !matches!(nid.nid_type(), NidType::NormalFolder) {
                continue;
            }
            if sample >= 8 {
                break;
            }
            sample += 1;
            let h = nid.hierarchy_table();
            let c = nid.contents_table();
            let h_present = pst.nbt.get(h).is_some();
            let c_present = pst.nbt.get(c).is_some();
            let mut h_rows = None;
            let mut c_rows = None;
            if h_present {
                if let Ok(t) = tc::load_tc(
                    &mut pst.reader,
                    &pst.nbt,
                    &pst.bbt,
                    h,
                    pst.header.crypt_method,
                ) {
                    h_rows = Some(t.row_count());
                }
            }
            if c_present {
                if let Ok(t) = tc::load_tc(
                    &mut pst.reader,
                    &pst.nbt,
                    &pst.bbt,
                    c,
                    pst.header.crypt_method,
                ) {
                    c_rows = Some(t.row_count());
                }
            }
            println!(
                "folder 0x{:X}: hier_present={h_present} hier_rows={h_rows:?} cont_present={c_present} cont_rows={c_rows:?}",
                nid.0
            );
        }

        let walked = pst.folders().expect("folders");
        println!(
            "folders() returned {} entries, total message_count={}",
            walked.len(),
            walked.iter().map(|f| f.message_count).sum::<u32>()
        );
        for f in walked.iter().take(20) {
            println!(
                "  walked 0x{:X} path={} msgs={} children={}",
                f.nid.0,
                f.path,
                f.message_count,
                f.child_folder_nids.len()
            );
        }
    }
}
