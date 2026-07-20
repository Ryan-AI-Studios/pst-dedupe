//! JSON envelopes and stdout isolation helpers (track 0045 §3.3).

use serde::Serialize;
use serde_json::{json, Value};

use crate::error::{CliError, Result};

/// Write a success or structured value as the sole stdout payload when `json` is true.
///
/// When `json` is false, no-op (caller prints human text).
pub fn emit_json(json: bool, value: &impl Serialize) -> Result<()> {
    if json {
        // Single write to stdout — no progress/logs (those go to stderr via tracing).
        let s = serde_json::to_string_pretty(value)?;
        println!("{s}");
    }
    Ok(())
}

/// Standard success envelope.
pub fn ok_envelope(extra: Value) -> Value {
    let mut base = json!({ "ok": true });
    if let Value::Object(map) = extra {
        if let Value::Object(ref mut b) = base {
            for (k, v) in map {
                b.insert(k, v);
            }
        }
    }
    base
}

/// Standard failure envelope (also printed when `--json` on error paths).
pub fn err_envelope(err: &CliError, job_id: Option<&str>, state: Option<&str>) -> Value {
    let mut e = json!({
        "ok": false,
        "error": {
            "code": err.error_code(),
            "message": err.to_string(),
        }
    });
    if let Some(id) = job_id {
        e["job_id"] = json!(id);
    }
    if let Some(st) = state {
        e["state"] = json!(st);
    }
    if let CliError::JobFailed {
        job_id: Some(id),
        state: Some(st),
        ..
    } = err
    {
        e["job_id"] = json!(id);
        e["state"] = json!(st);
    }
    e
}

/// Emit error envelope to stdout when `--json`, else message to stderr.
pub fn emit_error(json: bool, err: &CliError) {
    if json {
        let env = err_envelope(err, None, None);
        if let Ok(s) = serde_json::to_string_pretty(&env) {
            println!("{s}");
        }
    } else {
        eprintln!("error: {err}");
    }
}
