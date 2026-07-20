//! Pass 1: parse headers → normalize → item_participants (+ person stubs).

use std::collections::HashSet;

use matter_core::{
    participant_role, Matter, PeoplePass1Candidate, UpsertItemParticipantInput,
    UpsertPersonStubInput,
};

use crate::error::Result;
use crate::normalize::{normalize_participant, NormalizedParticipant};
use crate::params::PeopleGraphParams;

/// Source marker for header-derived participants.
pub const SOURCE_HEADER: &str = "header";

/// One expanded participant for an item+role (de-duped by person key).
#[derive(Debug, Clone)]
pub struct ExpandedParticipant {
    pub person: NormalizedParticipant,
    pub role: &'static str,
    pub raw: String,
}

/// Prefer sent_at → received_at → created_at for timeline denorm.
pub fn item_best_at(cand: &PeoplePass1Candidate) -> Option<&str> {
    cand.sent_at
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            cand.received_at
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            cand.created_at
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
}

/// Parse From + JSON To/Cc/Bcc into de-duped participants (cap applied).
pub fn expand_item_participants(
    cand: &PeoplePass1Candidate,
    max_recipients: u32,
) -> (Vec<ExpandedParticipant>, u64 /* overflow skipped */) {
    let mut out: Vec<ExpandedParticipant> = Vec::new();
    let mut seen: HashSet<(String, String, String)> = HashSet::new(); // kind,key,role
    let mut overflow = 0u64;
    let mut recipient_slots: u32 = 0;

    if let Some(from) = cand.from_addr.as_deref() {
        try_push_participant(from, participant_role::FROM, &mut out, &mut seen);
    }

    for (json, role) in [
        (cand.to_addrs_json.as_deref(), participant_role::TO),
        (cand.cc_addrs_json.as_deref(), participant_role::CC),
        (cand.bcc_addrs_json.as_deref(), participant_role::BCC),
    ] {
        for raw in parse_addr_list(json) {
            if recipient_slots >= max_recipients {
                overflow += 1;
                continue;
            }
            let before = out.len();
            try_push_participant(&raw, role, &mut out, &mut seen);
            if out.len() > before {
                recipient_slots = recipient_slots.saturating_add(1);
            }
        }
    }

    (out, overflow)
}

fn try_push_participant(
    raw: &str,
    role: &'static str,
    out: &mut Vec<ExpandedParticipant>,
    seen: &mut HashSet<(String, String, String)>,
) {
    let Some(person) = normalize_participant(raw) else {
        return;
    };
    let key = (
        person.identity_kind.clone(),
        person.normalized_key.clone(),
        role.to_string(),
    );
    if !seen.insert(key) {
        return;
    }
    out.push(ExpandedParticipant {
        person,
        role,
        raw: raw.trim().to_string(),
    });
}

/// Parse JSON string array, single JSON string, or bare comma-ish string.
pub fn parse_addr_list(json: Option<&str>) -> Vec<String> {
    let Some(s) = json.map(str::trim).filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    if s == "[]" || s == "null" {
        return Vec::new();
    }
    // Prefer JSON array of strings.
    if let Ok(vals) = serde_json::from_str::<Vec<String>>(s) {
        return vals
            .into_iter()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect();
    }
    // Single JSON string.
    if let Ok(one) = serde_json::from_str::<String>(s) {
        let t = one.trim().to_string();
        if t.is_empty() {
            return Vec::new();
        }
        return vec![t];
    }
    // Bare value.
    vec![s.to_string()]
}

/// Upsert stubs + participants for one item (idempotent).
pub fn process_pass1_item(
    matter: &Matter,
    cand: &PeoplePass1Candidate,
    params: &PeopleGraphParams,
) -> Result<(u64 /* participants written */, u64 /* overflow */)> {
    let (expanded, overflow) = expand_item_participants(cand, params.max_recipients_per_item);
    let item_at = item_best_at(cand);
    let mut written = 0u64;
    for exp in &expanded {
        let pid = matter.upsert_person_stub(UpsertPersonStubInput {
            identity_kind: &exp.person.identity_kind,
            normalized_key: &exp.person.normalized_key,
            email_domain: exp.person.email_domain.as_deref(),
            display_label: Some(exp.person.display_label.as_str()),
        })?;
        matter.upsert_item_participant(UpsertItemParticipantInput {
            item_id: &cand.id,
            person_id: &pid,
            role: exp.role,
            source: SOURCE_HEADER,
            raw_value: Some(exp.raw.as_str()),
            item_at,
        })?;
        written += 1;
    }
    // include_entity_emails is rejected at params.validate() when true.
    debug_assert!(
        !params.include_entity_emails,
        "include_entity_emails must fail closed before Pass 1"
    );
    Ok((written, overflow))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_json_array_and_dedupe() {
        let cand = PeoplePass1Candidate {
            id: "i1".into(),
            from_addr: Some("Alice@Example.com".into()),
            to_addrs_json: Some(
                r#"["bob@example.com","bob@example.com,","Carol@Example.com"]"#.into(),
            ),
            cc_addrs_json: None,
            bcc_addrs_json: Some(r#"["secret@example.com"]"#.into()),
            sent_at: Some("2024-01-02T00:00:00Z".into()),
            received_at: None,
            created_at: None,
        };
        let (exp, overflow) = expand_item_participants(&cand, 200);
        assert_eq!(overflow, 0);
        // from + bob (once) + carol + secret
        assert_eq!(exp.len(), 4);
        assert!(exp.iter().any(|e| e.role == participant_role::BCC));
        assert!(exp.iter().any(
            |e| e.role == participant_role::TO && e.person.normalized_key == "bob@example.com"
        ));
    }
}
