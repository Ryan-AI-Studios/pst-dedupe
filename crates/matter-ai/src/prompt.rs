//! Versioned prompt builder for first-pass code suggestions (spec §3.5.2 / 0052).

use matter_core::CodeDef;

use crate::provider::{ChatMessage, CompletionRequest};

/// Frozen prompt template id for coding suggest (0051).
pub const PROMPT_TEMPLATE_SUGGEST_CODES_V1: &str = "suggest_codes_v1";

/// Prompt template with grounded citations (0052).
pub const PROMPT_TEMPLATE_SUGGEST_CODES_V2: &str = "suggest_codes_v2";

/// System instruction: strictly apply provided definitions.
const SYSTEM_CORE: &str = "You are a first-pass eDiscovery coding assistant. \
Strictly apply the provided code definitions for this matter. \
Do not invent or substitute generic legal definitions. \
Respond with a JSON array of objects: \
{ \"code_id\"?: string, \"code_name\": string, \"confidence\"?: number, \"rationale_short\"?: string }. \
Only suggest codes from the catalog below. If none apply, return [].";

/// System instruction for v2: catalog rules + grounded citations.
const SYSTEM_CORE_V2: &str = "You are a first-pass eDiscovery coding assistant. \
Strictly apply the provided code definitions for this matter. \
Do not invent or substitute generic legal definitions. \
Respond with a JSON array of objects: \
{ \"code_id\"?: string, \"code_name\": string, \"confidence\"?: number, \
\"rationale_short\"?: string, \
\"citations\"?: [ { \"quote\": string, \"start_offset\"?: number, \"end_offset\"?: number, \"field\"?: string } ] }. \
Citations must be contiguous verbatim substrings of the provided document text. \
Do NOT use ellipses (...) or (…) to combine non-contiguous sentences or clauses. \
Each quote is one continuous span. Prefer under ~50 words per citation. \
Do not invent quotes not present in the text. Offsets are UTF-8 byte indices (hints). \
Empty citations array is allowed. Only suggest codes from the catalog below. If none apply, return [].";

/// Build `suggest_codes_v1` completion request with full catalog guidance.
pub fn build_suggest_codes_v1(
    model: &str,
    catalog: &[CodeDef],
    item_text: &str,
    temperature: f32,
    max_tokens: Option<u32>,
) -> CompletionRequest {
    let system = format!("{SYSTEM_CORE}\n\n{}", format_catalog(catalog));
    let user = format!(
        "Item text (may be middle-drop truncated):\n\n{item_text}\n\n\
         Return JSON array of applicable codes only."
    );
    CompletionRequest {
        model: model.to_string(),
        messages: vec![ChatMessage::system(system), ChatMessage::user(user)],
        temperature: Some(temperature),
        max_tokens,
        response_format_json_object: true,
    }
}

/// Build `suggest_codes_v2` request (catalog + contiguous citation rules).
pub fn build_suggest_codes_v2(
    model: &str,
    catalog: &[CodeDef],
    item_text: &str,
    temperature: f32,
    max_tokens: Option<u32>,
) -> CompletionRequest {
    let system = format!("{SYSTEM_CORE_V2}\n\n{}", format_catalog(catalog));
    let user = format!(
        "Item text (may be middle-drop truncated):\n\n{item_text}\n\n\
         Return JSON array of applicable codes only. Include contiguous verbatim citations when possible."
    );
    CompletionRequest {
        model: model.to_string(),
        messages: vec![ChatMessage::system(system), ChatMessage::user(user)],
        temperature: Some(temperature),
        max_tokens,
        response_format_json_object: true,
    }
}

/// Format active codes with full guidance (fallback to label if guidance empty).
pub fn format_catalog(catalog: &[CodeDef]) -> String {
    let mut lines = Vec::new();
    lines.push("Code catalog (apply these definitions only):".to_string());
    let mut active: Vec<&CodeDef> = catalog.iter().filter(|d| d.is_active != 0).collect();
    active.sort_by(|a, b| {
        a.sort_order
            .cmp(&b.sort_order)
            .then_with(|| a.key.cmp(&b.key))
    });
    for d in active {
        let guidance = d
            .guidance
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(d.label.as_str());
        lines.push(format!(
            "- id={} key={} name={} | guidance: {}",
            d.id, d.key, d.label, guidance
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use matter_core::CodeDef;

    fn sample_def(guidance: Option<&str>) -> CodeDef {
        CodeDef {
            id: "cde_x".into(),
            matter_id: "m".into(),
            key: "privilege".into(),
            label: "Privilege".into(),
            group_key: "privilege".into(),
            cardinality: "multi".into(),
            color: None,
            sort_order: 10,
            is_active: 1,
            created_at: String::new(),
            guidance: guidance.map(|s| s.to_string()),
        }
    }

    #[test]
    fn idiosyncratic_guidance_appears_in_prompt() {
        let marker = "XYZZY_ONLY_WHEN_ATTORNEY_CLIENT_FOO_PROTOCOL_99";
        let defs = vec![sample_def(Some(marker))];
        let req = build_suggest_codes_v1("mock", &defs, "body text", 0.0, Some(512));
        let system = &req.messages[0].content;
        assert!(
            system.contains(marker),
            "prompt must embed full guidance, got: {system}"
        );
        assert!(system.contains("Do not invent"));
        assert_eq!(PROMPT_TEMPLATE_SUGGEST_CODES_V1, "suggest_codes_v1");
    }

    #[test]
    fn empty_guidance_falls_back_to_label() {
        let defs = vec![sample_def(None)];
        let catalog = format_catalog(&defs);
        assert!(catalog.contains("guidance: Privilege"));
    }

    #[test]
    fn v2_prompt_forbids_splice_and_asks_citations() {
        let defs = vec![sample_def(Some("marker_xyz"))];
        let req = build_suggest_codes_v2("mock", &defs, "body text", 0.0, Some(512));
        let system = &req.messages[0].content;
        assert!(system.contains("marker_xyz"));
        assert!(system.contains("Do NOT use ellipses"));
        assert!(system.contains("contiguous"));
        assert!(system.contains("~50 words"));
        assert!(system.contains("citations"));
        assert_eq!(PROMPT_TEMPLATE_SUGGEST_CODES_V2, "suggest_codes_v2");
    }
}
