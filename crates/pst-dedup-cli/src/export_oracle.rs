//! Export equivalence oracle for unique-pst (track 0079 §3.2).
//!
//! Unique-pst output is **not** byte-reproducible (D10: store record key uses
//! `SystemTime::now()` + pid). This oracle compares **structure and content**,
//! not raw PST bytes.
//!
//! Compared (exact, modulo allowlist):
//! - `keepset.json`, `decisions.csv`, `export_messages.csv`, `export_attachments.csv`
//! - `summary.json` modulo timing / path / hash fields
//! - Per-volume structural digests (message count, folder paths, body+attach digests)
//! - Integrity counters / `degraded_reasons` via summary + keep-set

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

/// Fields stripped / ignored when comparing `summary.json` (volatile or path-local).
const SUMMARY_ALLOWLIST_KEYS: &[&str] = &[
    "duration_ms",
    "duration_secs",
    "phase_timings",
    "hash_ms",
    "summary_path",
    "out",
    "report_dir",
    "decision_csv",
    "keep_set_json",
    "inputs",
    // Volume digests change with store record key (D10).
    "sha256_hex",
    "md5_hex",
    "sha256",
    "md5",
    "path",
    "bytes",
    // Handle open counts / peak RAM are measurement, not product.
    "source_pst_opens",
    "prepared_bytes_peak",
    "messages_materialized",
    "bytes_written_total",
];

/// One volume's structural fingerprint (order-stable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeStructuralDigest {
    pub message_count: u64,
    /// Folder display paths in traversal order.
    pub folder_paths: Vec<String>,
    /// Per-message content digests in folder/message traversal order.
    pub message_digests: Vec<String>,
}

/// Full export pack comparison result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleDiff {
    pub mismatches: Vec<String>,
}

impl OracleDiff {
    pub fn ok(&self) -> bool {
        self.mismatches.is_empty()
    }

    pub fn assert_equivalent(&self) {
        assert!(
            self.ok(),
            "export equivalence oracle failed:\n{}",
            self.mismatches.join("\n")
        );
    }
}

/// Compare two completed unique-pst report packs (+ volume PSTs).
///
/// `report_a` / `report_b` are report-dir paths. `out_a` / `out_b` are primary
/// `--out` paths (volume 1); multi-volume siblings are discovered via `volumes.csv`
/// or summary when present.
pub fn compare_export_packs(
    report_a: &Path,
    out_a: &Path,
    report_b: &Path,
    out_b: &Path,
) -> OracleDiff {
    let mut mismatches = Vec::new();

    // keepset.json: strip path-local absolute source paths to basenames.
    match (
        load_json(&report_a.join("keepset.json")),
        load_json(&report_b.join("keepset.json")),
    ) {
        (Ok(mut ka), Ok(mut kb)) => {
            normalize_keepset_paths(&mut ka);
            normalize_keepset_paths(&mut kb);
            if ka != kb {
                mismatches.push("keepset.json differs after path normalize".into());
            }
        }
        (Err(e), _) => mismatches.push(format!("keepset.json A: {e}")),
        (_, Err(e)) => mismatches.push(format!("keepset.json B: {e}")),
    }
    compare_csv_normalized(
        &report_a.join("decisions.csv"),
        &report_b.join("decisions.csv"),
        "decisions.csv",
        &["source_path", "winner_source_path"],
        &[],
        &mut mismatches,
    );
    // export_messages: volume_path differs by --out; source_path basenames only.
    compare_csv_normalized(
        &report_a.join("export_messages.csv"),
        &report_b.join("export_messages.csv"),
        "export_messages.csv",
        &["source_path"],
        &["volume_path"],
        &mut mismatches,
    );
    // Attach ledger is optional when mode=off or zero fails; compare when both exist.
    let att_a = report_a.join("export_attachments.csv");
    let att_b = report_b.join("export_attachments.csv");
    match (att_a.is_file(), att_b.is_file()) {
        (true, true) => compare_csv_normalized(
            &att_a,
            &att_b,
            "export_attachments.csv",
            &["source_path"],
            &["volume_path"],
            &mut mismatches,
        ),
        (false, false) => {}
        (a, b) => mismatches.push(format!(
            "export_attachments.csv presence mismatch: a={a} b={b}"
        )),
    }

    match (
        load_json(&report_a.join("summary.json")),
        load_json(&report_b.join("summary.json")),
    ) {
        (Ok(mut sa), Ok(mut sb)) => {
            normalize_summary_for_oracle(&mut sa);
            normalize_summary_for_oracle(&mut sb);
            if sa != sb {
                // Narrow the mismatch for diagnostics.
                if let (Some(oa), Some(ob)) = (sa.as_object(), sb.as_object()) {
                    for key in oa.keys().chain(ob.keys()).collect::<BTreeSet<_>>() {
                        if oa.get(key.as_str()) != ob.get(key.as_str()) {
                            mismatches.push(format!(
                                "summary.json field '{key}' differs after allowlist strip"
                            ));
                        }
                    }
                } else {
                    mismatches.push("summary.json structural inequality".into());
                }
            }
            // Integrity / risk level (subset of summary, already compared).
            compare_integrity_counters(&sa, &sb, &mut mismatches);
        }
        (Err(e), _) => mismatches.push(format!("summary.json A: {e}")),
        (_, Err(e)) => mismatches.push(format!("summary.json B: {e}")),
    }

    // Per-volume structural digests.
    let digests_a = volume_digests_for_pack(report_a, out_a);
    let digests_b = volume_digests_for_pack(report_b, out_b);
    match (digests_a, digests_b) {
        (Ok(a), Ok(b)) => {
            if a.len() != b.len() {
                mismatches.push(format!("volume count {} vs {}", a.len(), b.len()));
            } else {
                for (i, (da, db)) in a.iter().zip(b.iter()).enumerate() {
                    if da != db {
                        mismatches.push(format!(
                            "volume[{i}] structural digest mismatch: msgs {} vs {}, folders {} vs {}, digests {} vs {}",
                            da.message_count,
                            db.message_count,
                            da.folder_paths.len(),
                            db.folder_paths.len(),
                            da.message_digests.len(),
                            db.message_digests.len()
                        ));
                        if da.message_digests != db.message_digests {
                            let first_diff = da
                                .message_digests
                                .iter()
                                .zip(db.message_digests.iter())
                                .position(|(x, y)| x != y);
                            if let Some(p) = first_diff {
                                mismatches.push(format!(
                                    "volume[{i}] first message digest index {p} differs"
                                ));
                            }
                        }
                    }
                }
            }
        }
        (Err(e), _) => mismatches.push(format!("volume digests A: {e}")),
        (_, Err(e)) => mismatches.push(format!("volume digests B: {e}")),
    }

    OracleDiff { mismatches }
}

fn compare_integrity_counters(a: &Value, b: &Value, mismatches: &mut Vec<String>) {
    // Keep-set degraded_winners + export attach fails must match (already in summary strip).
    let paths = [
        "/keep_set/stats/degraded_winners",
        "/export/attachments_failed",
        "/export/attachments_written",
        "/export/messages_written_total",
        "/export_risk/level",
        "/scan/block_crc_rate",
        "/scan/block_crc_read_rate",
    ];
    for p in paths {
        let va = a.pointer(p);
        let vb = b.pointer(p);
        if va != vb {
            mismatches.push(format!("integrity pointer {p}: {va:?} vs {vb:?}"));
        }
    }
}

/// Compare CSV files after normalizing path columns.
///
/// * `basename_cols` — replace with file basename (source_path).
/// * `blank_cols` — replace with empty (volume_path differs by --out under D10).
fn compare_csv_normalized(
    a: &Path,
    b: &Path,
    label: &str,
    basename_cols: &[&str],
    blank_cols: &[&str],
    mismatches: &mut Vec<String>,
) {
    match (fs::read_to_string(a), fs::read_to_string(b)) {
        (Ok(sa), Ok(sb)) => {
            let na = normalize_csv_path_cols(&sa, basename_cols, blank_cols);
            let nb = normalize_csv_path_cols(&sb, basename_cols, blank_cols);
            if na != nb {
                mismatches.push(format!(
                    "{label}: content differs after path-column normalize ({} vs {} bytes)",
                    na.len(),
                    nb.len()
                ));
            }
        }
        (Err(e), _) => mismatches.push(format!("{label} A: {e}")),
        (_, Err(e)) => mismatches.push(format!("{label} B: {e}")),
    }
}

fn normalize_csv_path_cols(csv: &str, basename_cols: &[&str], blank_cols: &[&str]) -> String {
    let mut lines = csv.lines();
    let Some(header) = lines.next() else {
        return String::new();
    };
    let cols: Vec<&str> = header.split(',').collect();
    let base_idx: Vec<usize> = basename_cols
        .iter()
        .filter_map(|name| cols.iter().position(|c| c == name))
        .collect();
    let blank_idx: Vec<usize> = blank_cols
        .iter()
        .filter_map(|name| cols.iter().position(|c| c == name))
        .collect();
    let mut out = String::with_capacity(csv.len());
    out.push_str(header);
    out.push('\n');
    for line in lines {
        if line.is_empty() {
            continue;
        }
        // Simple CSV split (fixture paths lack embedded commas).
        let mut fields: Vec<String> = line.split(',').map(|s| s.to_string()).collect();
        for &i in &base_idx {
            if let Some(f) = fields.get_mut(i) {
                let bare = f.trim_matches('"');
                // Strip Windows extended-path prefix for basename.
                let bare = bare.strip_prefix(r"\\?\").unwrap_or(bare);
                let base = Path::new(bare)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(bare);
                *f = base.to_string();
            }
        }
        for &i in &blank_idx {
            if let Some(f) = fields.get_mut(i) {
                *f = String::new();
            }
        }
        out.push_str(&fields.join(","));
        out.push('\n');
    }
    out
}

fn load_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn basename_value(v: &Value) -> Value {
    match v.as_str() {
        Some(s) => Value::String(
            Path::new(s)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(s)
                .to_string(),
        ),
        None => v.clone(),
    }
}

fn normalize_keepset_paths(v: &mut Value) {
    if let Some(winners) = v.get_mut("winners").and_then(|x| x.as_array_mut()) {
        for w in winners {
            if let Some(p) = w.pointer_mut("/locus/source_path") {
                *p = basename_value(p);
            }
            if let Some(p) = w.pointer_mut("/locus/source_pst") {
                *p = basename_value(p);
            }
        }
    }
    if let Some(inputs) = v
        .pointer_mut("/provenance/input_files")
        .and_then(|x| x.as_array_mut())
    {
        for i in inputs.iter_mut() {
            *i = basename_value(i);
        }
    }
}

/// Remove / zero volatile fields so two runs can compare equal under D10.
pub fn normalize_summary_for_oracle(v: &mut Value) {
    strip_keys_recursive(v, SUMMARY_ALLOWLIST_KEYS);
    // Volume path strings inside export.volumes[].path.
    if let Some(vols) = v
        .pointer_mut("/export/volumes")
        .and_then(|x| x.as_array_mut())
    {
        for vol in vols {
            if let Some(obj) = vol.as_object_mut() {
                obj.insert("path".into(), Value::String(String::new()));
                obj.insert("sha256_hex".into(), Value::String(String::new()));
                obj.insert("md5_hex".into(), Value::String(String::new()));
            }
        }
    }
    // verification volume paths
    if let Some(vols) = v
        .pointer_mut("/verification/volumes")
        .and_then(|x| x.as_array_mut())
    {
        for vol in vols {
            if let Some(obj) = vol.as_object_mut() {
                obj.insert("path".into(), Value::String(String::new()));
            }
        }
    }
    // Scan file rows carry absolute paths (and may differ only by path spelling).
    if let Some(files) = v.pointer_mut("/scan/files").and_then(|x| x.as_array_mut()) {
        for f in files.iter_mut() {
            if let Some(obj) = f.as_object_mut() {
                if let Some(p) = obj.get("path").and_then(|x| x.as_str()) {
                    let base = Path::new(p)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();
                    obj.insert("path".into(), Value::String(base));
                }
            }
        }
    }
    // keep_set winners: normalize absolute source paths to basenames for compare.
    if let Some(winners) = v
        .pointer_mut("/keep_set/winners")
        .and_then(|x| x.as_array_mut())
    {
        for w in winners {
            if let Some(p) = w.pointer_mut("/locus/source_path") {
                if let Some(s) = p.as_str() {
                    let base = Path::new(s)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(s)
                        .to_string();
                    *p = Value::String(base);
                }
            }
        }
    }
    if let Some(obj) = v.as_object_mut() {
        obj.insert("inputs".into(), Value::Array(vec![]));
    }
}

fn strip_keys_recursive(v: &mut Value, keys: &[&str]) {
    match v {
        Value::Object(map) => {
            for k in keys {
                map.remove(*k);
            }
            // Also strip nested timing-ish keys by name.
            let nested: Vec<String> = map.keys().cloned().collect();
            for k in nested {
                if let Some(child) = map.get_mut(&k) {
                    strip_keys_recursive(child, keys);
                }
            }
        }
        Value::Array(arr) => {
            for child in arr {
                strip_keys_recursive(child, keys);
            }
        }
        _ => {}
    }
}

fn volume_digests_for_pack(
    report_dir: &Path,
    out: &Path,
) -> Result<Vec<VolumeStructuralDigest>, String> {
    let mut paths = volume_paths_from_summary(report_dir, out)?;
    if paths.is_empty() && out.is_file() {
        paths.push(out.to_path_buf());
    }
    let mut out_digests = Vec::with_capacity(paths.len());
    for p in paths {
        out_digests.push(structural_digest_pst(&p)?);
    }
    Ok(out_digests)
}

fn volume_paths_from_summary(report_dir: &Path, out: &Path) -> Result<Vec<PathBuf>, String> {
    let summary_path = report_dir.join("summary.json");
    if !summary_path.is_file() {
        return Ok(if out.is_file() {
            vec![out.to_path_buf()]
        } else {
            vec![]
        });
    }
    let v = load_json(&summary_path)?;
    let mut paths = Vec::new();
    if let Some(vols) = v.pointer("/export/volumes").and_then(|x| x.as_array()) {
        for vol in vols {
            if let Some(p) = vol.get("path").and_then(|x| x.as_str()) {
                let pb = PathBuf::from(p);
                if pb.is_file() {
                    paths.push(pb);
                }
            }
        }
    }
    if paths.is_empty() && out.is_file() {
        paths.push(out.to_path_buf());
    }
    Ok(paths)
}

/// Structural digest of one written PST volume (0079 §3.2 item 3).
pub fn structural_digest_pst(path: &Path) -> Result<VolumeStructuralDigest, String> {
    let mut pst =
        pst_reader::PstFile::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let folders = pst
        .folders()
        .map_err(|e| format!("folders {}: {e}", path.display()))?;
    let mut folder_paths = Vec::new();
    let mut message_digests = Vec::new();
    let mut message_count = 0u64;

    for folder in &folders {
        folder_paths.push(folder.path.clone());
        for &nid in &folder.message_nids {
            message_count += 1;
            let digest = message_content_digest(&mut pst, nid.0)?;
            message_digests.push(digest);
        }
    }

    Ok(VolumeStructuralDigest {
        message_count,
        folder_paths,
        message_digests,
    })
}

fn message_content_digest(pst: &mut pst_reader::PstFile, nid: u64) -> Result<String, String> {
    use pst_reader::NodeId;
    let extract = pst
        .read_message_extract(NodeId(nid))
        .map_err(|e| format!("extract nid={nid:#x}: {e}"))?;
    let mid = extract
        .message_id
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let subject = extract.subject.as_deref().unwrap_or("");
    let body_plain = extract.body_text.as_deref().unwrap_or("");
    let body_html = extract.body_html.as_deref().unwrap_or(&[][..]);

    let mut attaches: Vec<(String, u64, String, String)> = Vec::new();
    if let Ok(list) = pst.list_attachments(NodeId(nid)) {
        for meta in &list {
            let filename = meta.filename.clone();
            let size = u64::from(meta.size);
            let mime = meta.mime_tag.clone().unwrap_or_default();
            let mut payload_hash = String::new();
            if let Ok(mut reader) = pst.open_attachment_data(NodeId(nid), meta.nid) {
                let mut buf = Vec::new();
                if reader.read_to_end(&mut buf).is_ok() {
                    payload_hash = hex_sha256(&buf);
                }
            }
            attaches.push((filename, size, mime, payload_hash));
        }
    }

    let mut h = Sha256::new();
    h.update(mid.as_bytes());
    h.update([0]);
    h.update(subject.as_bytes());
    h.update([0]);
    h.update(body_plain.as_bytes());
    h.update([0]);
    h.update(body_html);
    h.update([0]);
    for (fnm, sz, mime, ph) in &attaches {
        h.update(fnm.as_bytes());
        h.update([0]);
        h.update(sz.to_le_bytes());
        h.update(mime.as_bytes());
        h.update([0]);
        h.update(ph.as_bytes());
        h.update([0]);
    }
    Ok(hex_sha256_digest(h.finalize().as_slice()))
}

fn hex_sha256(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    hex_sha256_digest(d.as_slice())
}

fn hex_sha256_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

/// Collect per-winner degraded_reasons from keepset.json for reason-set tests.
pub fn degraded_reasons_by_winner(
    report_dir: &Path,
) -> Result<BTreeMap<String, Vec<String>>, String> {
    let v = load_json(&report_dir.join("keepset.json"))?;
    let mut map = BTreeMap::new();
    let winners = v
        .get("winners")
        .and_then(|w| w.as_array())
        .ok_or_else(|| "keepset.json missing winners".to_string())?;
    for w in winners {
        let path = w
            .pointer("/locus/source_path")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        let nid = w
            .pointer("/locus/nid")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let key = format!("{path}|{nid}");
        let mut reasons = Vec::new();
        if let Some(arr) = w
            .pointer("/integrity/degraded_reasons")
            .and_then(|x| x.as_array())
        {
            for r in arr {
                if let Some(s) = r.as_str() {
                    reasons.push(s.to_string());
                }
            }
        }
        reasons.sort();
        map.insert(key, reasons);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_strips_timing_and_paths() {
        let mut v = json!({
            "duration_ms": 123,
            "phase_timings": { "scan_ms": 1, "total_ms": 2 },
            "out": "C:/a.pst",
            "ok": true,
            "export": {
                "volumes": [{ "path": "C:/a.pst", "sha256_hex": "abc", "md5_hex": "def", "messages_written": 1 }]
            }
        });
        normalize_summary_for_oracle(&mut v);
        assert!(v.get("duration_ms").is_none());
        assert!(v.get("phase_timings").is_none());
        assert!(v.get("out").is_none());
        assert_eq!(v["ok"], true);
        assert_eq!(v["export"]["volumes"][0]["path"], "");
        assert_eq!(v["export"]["volumes"][0]["messages_written"], 1);
    }

    #[test]
    fn oracle_diff_ok_when_empty() {
        let d = OracleDiff { mismatches: vec![] };
        assert!(d.ok());
    }
}
