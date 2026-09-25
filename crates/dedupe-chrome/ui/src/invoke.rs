//! Tauri `invoke` via `window.__TAURI__.core` (`withGlobalTauri: true`).

use serde::de::DeserializeOwned;
use serde::Serialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], catch)]
    fn invoke(cmd: &str, args: JsValue) -> Result<js_sys::Promise, JsValue>;
}

pub async fn tauri_invoke<T, A>(cmd: &str, args: &A) -> Result<T, String>
where
    T: DeserializeOwned,
    A: Serialize,
{
    let args_js = serde_wasm_bindgen::to_value(args).map_err(|e| e.to_string())?;
    // Tauri 2 IPC expects camelCase keys (`paramsJson`, `jobId`, `filterJson`).
    let args_js = js_args_to_camel(args_js)?;
    let promise = invoke(cmd, args_js).map_err(js_err_to_string)?;
    let value = JsFuture::from(promise).await.map_err(js_err_to_string)?;
    serde_wasm_bindgen::from_value(value).map_err(|e| e.to_string())
}

fn to_camel_case_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut upper = false;
    for ch in key.chars() {
        if ch == '_' {
            upper = true;
            continue;
        }
        if upper {
            for u in ch.to_uppercase() {
                out.push(u);
            }
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Shallow own-key rename. Nested objects/arrays are copied as values (no recursion).
/// Host-tested twin of `js_args_to_camel`; wasm production path uses the JS adapter.
#[allow(dead_code)]
fn shallow_camel_object_keys(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(to_camel_case_key(&k), v);
            }
            serde_json::Value::Object(out)
        }
        other => other,
    }
}

fn js_args_to_camel(value: JsValue) -> Result<JsValue, String> {
    if value.is_null() || value.is_undefined() || !value.is_object() {
        return Ok(value);
    }
    if js_sys::Array::is_array(&value) {
        return Ok(value);
    }
    if let Some(map) = value.dyn_ref::<js_sys::Map>() {
        return js_map_to_camel_object(map);
    }
    let obj = js_sys::Object::from(value);
    let out = js_sys::Object::new();
    let keys = js_sys::Object::keys(&obj);
    for i in 0..keys.length() {
        let key = keys
            .get(i)
            .as_string()
            .ok_or_else(|| "non-string invoke arg key".to_string())?;
        let camel = to_camel_case_key(&key);
        let val = js_sys::Reflect::get(&obj, &JsValue::from_str(&key)).map_err(js_err_to_string)?;
        js_sys::Reflect::set(&out, &JsValue::from_str(&camel), &val).map_err(js_err_to_string)?;
    }
    Ok(JsValue::from(out))
}

fn js_map_to_camel_object(map: &js_sys::Map) -> Result<JsValue, String> {
    let out = js_sys::Object::new();
    let entries = map.entries();
    loop {
        let next = entries.next().map_err(js_err_to_string)?;
        if next.done() {
            break;
        }
        let pair = js_sys::Array::from(&next.value());
        let key = pair
            .get(0)
            .as_string()
            .ok_or_else(|| "non-string invoke arg key".to_string())?;
        let val = pair.get(1);
        js_sys::Reflect::set(&out, &JsValue::from_str(&to_camel_case_key(&key)), &val)
            .map_err(js_err_to_string)?;
    }
    Ok(JsValue::from(out))
}

fn js_err_to_string(err: JsValue) -> String {
    if let Some(s) = err.as_string() {
        return s;
    }
    if let Ok(obj) = serde_wasm_bindgen::from_value::<serde_json::Value>(err.clone()) {
        if let Some(kind) = obj.get("kind").and_then(|v| v.as_str()) {
            let msg = obj
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            return format!("{kind}: {msg}");
        }
        return obj.to_string();
    }
    format!("{err:?}")
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RootArgs {
    pub root: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CreateArgs {
    pub parent: String,
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RememberArgs {
    pub root: String,
    pub name: String,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct RecentMatter {
    pub root: String,
    pub name: String,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct MatterOverview {
    pub name: String,
    pub matter_id: String,
    pub schema_version: u32,
    pub generated_at: String,
    pub sources: u64,
    pub processed: u64,
    pub exceptions: u64,
    pub unreviewed: u64,
    pub privileged: u64,
    pub withhold: u64,
    pub custodians: u64,
    pub custodians_plus: bool,
    pub other_custodians_item_count: u64,
    pub produced: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewQueuePageArgs {
    pub root: String,
    pub filter_json: Option<String>,
    pub keyword: Option<String>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    pub extras: Option<bool>,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct QueueRow {
    pub id: String,
    pub review_order: Option<i64>,
    pub date: Option<String>,
    pub from_addr: Option<String>,
    pub subject: Option<String>,
    pub parent_item_id: Option<String>,
    pub role: Option<String>,
    pub family_id: Option<String>,
    pub family_size: u64,
    pub resp: Option<String>,
    pub privilege_coded: bool,
    pub withhold: bool,
    pub custodian: Option<String>,
    #[serde(default)]
    pub confidential: Option<bool>,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct ReviewQueuePage {
    pub total: u64,
    pub offset: u64,
    pub limit: u64,
    pub extras: bool,
    pub rows: Vec<QueueRow>,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct CodeCatalogEntry {
    pub id: String,
    pub key: String,
    pub label: String,
    pub group_key: String,
    pub cardinality: String,
    pub sort_order: i64,
    pub is_active: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SavedSearchUpsertArgs {
    pub root: String,
    pub name: String,
    pub filter_json: String,
    pub keyword: Option<String>,
    pub description: Option<String>,
    pub id: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct SavedSearchDto {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub scope: String,
    pub filter_json: String,
    pub keyword: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewCodesPreviewArgs {
    pub root: String,
    pub item_ids: Vec<String>,
    pub add_code_ids: Vec<String>,
    pub remove_code_ids: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct ReviewCodesPreview {
    pub privilege_would_change: u64,
    pub selected_count: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewApplyCodesArgs {
    pub root: String,
    pub item_ids: Vec<String>,
    pub add_code_ids: Vec<String>,
    pub remove_code_ids: Vec<String>,
    pub propagate_family: Option<bool>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewDocumentArgs {
    pub root: String,
    pub item_id: String,
    pub filter_json: Option<String>,
    pub keyword: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct FamilyMemberThin {
    pub id: String,
    pub parent_item_id: Option<String>,
    pub subject: Option<String>,
    pub role: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct ItemCodeInfo {
    pub code_id: String,
    pub key: String,
    pub label: String,
    pub group_key: String,
    pub cardinality: String,
    pub color: Option<String>,
    pub sort_order: i64,
    pub is_active: i64,
    pub set_at: String,
    pub set_by: String,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct ItemPrivilegeDto {
    pub item_id: String,
    pub basis: String,
    pub description: String,
    pub status: String,
    pub withhold: i64,
    pub include_on_log: i64,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct ItemNoteDto {
    pub id: String,
    pub item_id: String,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
    pub created_by: String,
    pub updated_by: String,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct PredictionSlot {
    pub present: bool,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct ReviewDocument {
    pub item_id: String,
    pub from_addr: Option<String>,
    pub to_addrs_json: Option<String>,
    pub cc_addrs_json: Option<String>,
    pub subject: Option<String>,
    pub sent_at: Option<String>,
    pub received_at: Option<String>,
    pub mime_type: Option<String>,
    pub path: Option<String>,
    pub review_order: Option<i64>,
    pub attachment_count: Option<i64>,
    pub family_id: Option<String>,
    pub family_size: u64,
    pub family_truncated: bool,
    pub family_members: Vec<FamilyMemberThin>,
    pub apply_to_family_enabled: bool,
    pub codes: Vec<ItemCodeInfo>,
    pub privilege: Option<ItemPrivilegeDto>,
    pub notes: Vec<ItemNoteDto>,
    pub prev_id: Option<String>,
    pub next_id: Option<String>,
    pub position: u64,
    pub total: u64,
    #[serde(default)]
    pub neighbors_error: Option<String>,
    pub control_number: String,
    pub bates: String,
    pub bates_note: String,
    pub prediction: PredictionSlot,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewDocumentBodyArgs {
    pub root: String,
    pub item_id: String,
    pub pane: String,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct ReviewDocumentBody {
    pub item_id: String,
    pub pane: String,
    pub text: String,
    pub truncated: bool,
    pub empty: bool,
    pub digest: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewWindowApplyArgs {
    pub root: String,
    pub item_ids: Vec<String>,
    pub add_code_ids: Vec<String>,
    pub remove_code_ids: Vec<String>,
    pub propagate_family: Option<bool>,
    pub privilege_basis: Option<String>,
    pub withhold: Option<bool>,
    pub include_on_log: Option<bool>,
    pub privilege_description: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewUpsertNoteArgs {
    pub root: String,
    pub item_id: String,
    pub body: String,
    pub id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewUpsertPrivilegeArgs {
    pub root: String,
    pub item_id: String,
    pub basis: String,
    pub withhold: Option<bool>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct ProductionSetThin {
    pub id: String,
    pub name: String,
    pub status: String,
    pub produced_ok_count: u64,
    pub bates_prefix: String,
    pub next_seq: u64,
    pub output_root: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct ProductionProfileThin {
    pub slug: String,
    pub name: String,
    pub qc_pack_id: String,
    #[serde(default)]
    pub include_images: bool,
    #[serde(default)]
    pub bates_mode: String,
    #[serde(default)]
    pub pad_width: u32,
    #[serde(default)]
    pub layout_data: String,
    #[serde(default)]
    pub layout_natives: String,
    #[serde(default)]
    pub layout_text: String,
    #[serde(default)]
    pub layout_images: String,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct QcGateDto {
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct ChromeQcFinding {
    pub item_id: Option<String>,
    pub rule_id: String,
    pub severity: String,
    pub message: String,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct ChromeExtra {
    pub kind: String,
    pub severity: String,
    pub item_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct WarningOverride {
    pub recorded_by: String,
    pub reason: String,
    pub rule_id: String,
    pub item_id: Option<String>,
    pub qc_run_id: String,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct ProducePageResponse {
    pub sets: Vec<ProductionSetThin>,
    pub default_count: u64,
    pub default_filter_json: String,
    pub qc_gate: QcGateDto,
    pub next_seq_hint: Option<u64>,
    pub produced_count: u64,
    pub profiles: Vec<ProductionProfileThin>,
    pub bates_prefix: String,
    #[serde(default)]
    pub need_burn: u64,
    #[serde(default)]
    pub burned_fresh: u64,
    #[serde(default)]
    pub unmapped_text: u64,
    #[serde(default)]
    pub ordered_ids: Vec<String>,
    #[serde(default)]
    pub protocol_log_format: String,
    #[serde(default)]
    pub protocol_fre_502d_note: Option<String>,
    #[serde(default)]
    pub protocol_fre_502e_note: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProduceQcRunArgs {
    pub root: String,
    pub filter_json: Option<String>,
    pub item_ids: Option<Vec<String>>,
    pub production_profile: Option<String>,
    pub source_entire_corpus: Option<bool>,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct ProduceQcRun {
    pub ordered_ids: Vec<String>,
    pub pack_id: String,
    pub scope: String,
    pub findings: Vec<ChromeQcFinding>,
    pub extras: Vec<ChromeExtra>,
    pub error_count: u64,
    pub warn_count: u64,
    pub passed: bool,
    pub qc_run_id: String,
    #[serde(default)]
    pub need_burn: u64,
    #[serde(default)]
    pub burned_fresh: u64,
    #[serde(default)]
    pub unmapped_text: u64,
    #[serde(default)]
    pub job_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProduceStartArgs {
    pub root: String,
    pub filter_json: Option<String>,
    pub item_ids: Option<Vec<String>>,
    pub production_profile: Option<String>,
    pub source_entire_corpus: Option<bool>,
    pub bates_prefix: Option<String>,
    pub bates_start: Option<u64>,
    pub warning_overrides: Option<Vec<WarningOverride>>,
    pub log_format: Option<String>,
    pub last_findings: Option<Vec<ChromeQcFinding>>,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct ProduceStart {
    pub ok: bool,
    pub blockers: Vec<ChromeExtra>,
    pub ordered_ids: Vec<String>,
    pub pack_id: String,
    pub scope: String,
    pub fail_if_withheld: bool,
    pub require_qc_pass: bool,
    pub produce_params: serde_json::Value,
    pub output_root: Option<String>,
    pub produced_count: u64,
    pub production_set_id: Option<String>,
    pub privilege_log_path: Option<String>,
    #[serde(default)]
    pub job_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewRasterPageArgs {
    pub root: String,
    pub item_id: String,
    pub page_index: Option<u32>,
    pub dpi: Option<u32>,
    pub generation: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct BoxF {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct ReviewRasterPage {
    pub item_id: String,
    pub generation: u64,
    pub png_base64: String,
    pub page_index: u32,
    pub page_count: u32,
    pub media_box: BoxF,
    pub crop_box: BoxF,
    pub rotate: i32,
    pub width: u32,
    pub height: u32,
    pub native_width: u32,
    pub native_height: u32,
    pub kind: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewGeomListArgs {
    pub root: String,
    pub item_id: String,
    pub generation: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct GeomDto {
    pub id: String,
    pub item_id: String,
    pub page_index: i64,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub reason: String,
    pub label: Option<String>,
    pub status: String,
    pub source: String,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct ReviewGeomList {
    pub item_id: String,
    pub generation: u64,
    pub boxes: Vec<GeomDto>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewGeomUpsertArgs {
    pub root: String,
    pub item_id: String,
    pub page_index: u32,
    pub px: f64,
    pub py: f64,
    pub pw: f64,
    pub ph: f64,
    pub raster_width: f64,
    pub raster_height: f64,
    pub reason: Option<String>,
    pub label: Option<String>,
    pub source: Option<String>,
    pub generation: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewGeomDeleteArgs {
    pub root: String,
    pub geom_id: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewGeomFromHitsArgs {
    pub root: String,
    pub item_id: String,
    pub query: Option<String>,
    pub reason: Option<String>,
    pub generation: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct ReviewGeomFromHits {
    pub item_id: String,
    pub generation: u64,
    pub inserted: u64,
    pub hit_count: u64,
    pub unmapped: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewBurnNativeArgs {
    pub root: String,
    pub item_id: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProduceBurnSetArgs {
    pub root: String,
    pub item_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct ProduceBurnSet {
    pub burned: u64,
    pub skipped: u64,
    pub errors: Vec<String>,
    #[serde(default)]
    pub need_burn: u64,
    #[serde(default)]
    pub burned_fresh: u64,
    #[serde(default)]
    pub unmapped_text: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProcessPageArgs {
    pub root: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProcessStartArgs {
    pub root: String,
    pub kind: String,
    pub params_json: String,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct ProcessStartResponse {
    pub job_id: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProcessCancelArgs {
    pub job_id: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProcessResumeArgs {
    pub root: String,
    pub job_id: String,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct JobProgressSnapshot {
    pub job_id: String,
    pub kind: String,
    pub matter_id: String,
    pub state: String,
    pub stage: Option<String>,
    pub completed_count: u64,
    pub total_hint: Option<u64>,
    pub message: Option<String>,
    pub error_summary: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct ProcessSourceRow {
    pub id: String,
    pub path: String,
    pub kind: String,
    pub status: String,
    #[serde(default)]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct ProcessPstRow {
    pub id: String,
    pub source_id: Option<String>,
    pub path: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct ProcessJobRow {
    pub id: String,
    pub kind: String,
    pub state: String,
    pub parent_job_id: Option<String>,
    pub error_summary: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    #[serde(default)]
    pub source_label: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct ProcessErrorGroup {
    pub code: String,
    pub count: u64,
    pub sample_message: String,
    #[serde(default)]
    pub sample_job_id: Option<String>,
    #[serde(default)]
    pub sample_item_id: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct BuiltinProfileFlags {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub classify: bool,
    pub office_extract: bool,
    pub pdf_extract: bool,
    pub ics_extract: bool,
    pub ocr: bool,
    pub fts: bool,
    pub dedupe: bool,
    pub thread: bool,
    pub neardup: bool,
    pub cull: bool,
    pub promote: bool,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct ProcessPageResponse {
    pub matter_id: String,
    pub schema_version: u32,
    pub sources: Vec<ProcessSourceRow>,
    pub pst_inventory: Vec<ProcessPstRow>,
    pub jobs: Vec<ProcessJobRow>,
    pub error_groups: Vec<ProcessErrorGroup>,
    pub selected_profile: String,
    pub builtins: Vec<BuiltinProfileFlags>,
    pub discovered: u64,
    pub exceptions: u64,
    pub in_review: u64,
    pub still_processing: u64,
    pub unaccounted_for: u64,
    pub denist: Option<u64>,
    pub dupes: Option<u64>,
    pub families: u64,
    #[serde(default)]
    pub pdf_needs_ocr: u64,
    #[serde(default)]
    pub unextracted_psts: Vec<ProcessPstRow>,
    #[serde(default)]
    pub failed_unlogged: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProcessExportReportArgs {
    pub root: String,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct ProcessExportReportResponse {
    #[serde(default)]
    pub output_dir: String,
    #[serde(default)]
    pub files_written: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProduceQcFindingsArgs {
    pub root: String,
    pub job_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{shallow_camel_object_keys, to_camel_case_key};
    use serde_json::json;

    #[test]
    fn tauri_ipc_keys_are_camel_case() {
        assert_eq!(to_camel_case_key("params_json"), "paramsJson");
        assert_eq!(to_camel_case_key("job_id"), "jobId");
        assert_eq!(to_camel_case_key("filter_json"), "filterJson");
        assert_eq!(to_camel_case_key("item_ids"), "itemIds");
        assert_eq!(
            to_camel_case_key("source_entire_corpus"),
            "sourceEntireCorpus"
        );
        assert_eq!(to_camel_case_key("warning_overrides"), "warningOverrides");
        assert_eq!(to_camel_case_key("page_index"), "pageIndex");
        assert_eq!(to_camel_case_key("root"), "root");
        assert_eq!(to_camel_case_key("paramsJson"), "paramsJson");
        assert_eq!(to_camel_case_key(""), "");
        assert_eq!(to_camel_case_key("a"), "a");
        assert_eq!(to_camel_case_key("item_id_2"), "itemId2");
        assert_eq!(to_camel_case_key("foo__bar"), "fooBar");
        assert_eq!(to_camel_case_key("job_"), "job");
    }

    #[test]
    fn shallow_walker_renames_own_keys_only() {
        let input = json!({
            "params_json": "{\"k\":1}",
            "nested": { "item_id": "keep-me" },
            "overrides": [ { "item_id": "also-keep", "rule_id": "r1" } ],
        });
        let out = shallow_camel_object_keys(input);
        assert_eq!(out["paramsJson"], "{\"k\":1}");
        assert!(out.get("params_json").is_none());
        assert_eq!(out["nested"]["item_id"], "keep-me");
        assert!(out["nested"].get("itemId").is_none());
        assert_eq!(out["overrides"][0]["item_id"], "also-keep");
        assert!(out["overrides"][0].get("itemId").is_none());
    }

    #[test]
    fn shallow_walker_leaves_array_null_and_scalars() {
        let arr = json!([{ "item_id": "x" }, "params_json"]);
        let out_arr = shallow_camel_object_keys(arr.clone());
        assert_eq!(out_arr, arr);
        assert!(out_arr.is_array());
        assert!(out_arr.as_object().is_none());

        assert_eq!(shallow_camel_object_keys(json!(null)), json!(null));
        assert_eq!(
            shallow_camel_object_keys(json!("params_json")),
            json!("params_json")
        );
        assert_eq!(shallow_camel_object_keys(json!(7)), json!(7));
        assert_eq!(shallow_camel_object_keys(json!(true)), json!(true));
    }

    #[test]
    fn invoke_adapter_is_one_shallow_pass_before_invoke() {
        let src = include_str!("invoke.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap_or(src);
        let invoke_fn = prod
            .split("pub async fn tauri_invoke")
            .nth(1)
            .expect("tauri_invoke");
        let invoke_fn = invoke_fn.split("\nfn ").next().unwrap_or(invoke_fn);
        let camel_at = invoke_fn
            .find("js_args_to_camel(args_js)")
            .expect("one camel pass on args");
        let invoke_at = invoke_fn.find("invoke(cmd, args_js)").expect("invoke call");
        assert!(camel_at < invoke_at, "camel rewrite must run before invoke");
        let after_invoke = &invoke_fn[invoke_at..];
        assert!(
            !after_invoke.contains("js_args_to_camel"),
            "JsFuture result must not be camel-cased"
        );
        assert!(
            !after_invoke.contains("shallow_camel_object_keys"),
            "result path must not run the walker"
        );
        assert_eq!(
            invoke_fn.matches("js_args_to_camel").count(),
            1,
            "exactly one camel pass in tauri_invoke"
        );
        assert!(prod.contains("Array::is_array"), "array early-return");
        assert!(prod.contains("js_sys::Map"), "Map branch");
        assert!(
            prod.contains("map.entries()"),
            "Map copied via entries, not Object.keys"
        );
        assert!(
            prod.contains("out.insert(to_camel_case_key(&k), v)"),
            "walker must copy values without renaming nested keys"
        );
        assert!(
            !prod.contains("shallow_camel_object_keys(v)"),
            "walker must not recurse into values"
        );
        assert!(
            !prod.contains("js_args_to_camel(val)"),
            "JS adapter must not re-walk copied values"
        );
    }
}
