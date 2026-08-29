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
    let promise = invoke(cmd, args_js).map_err(js_err_to_string)?;
    let value = JsFuture::from(promise).await.map_err(js_err_to_string)?;
    serde_wasm_bindgen::from_value(value).map_err(|e| e.to_string())
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
}
