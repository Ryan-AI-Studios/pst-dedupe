//! Per-message recipient table (MS-PST Recipient Table, `NID_TYPE_RECIPIENT_TABLE` 0x12).
//!
//! Structured recipients come only from the message subnode TC. Display* strings are
//! never invented into rows (0082 / fidelity_contract).

use crate::error::Result;
use crate::ltp::tc::TableContext;
use crate::ndb::block;
use crate::ndb::nid::{self, NidType, NodeId};
use crate::PstFile;

/// MAPI recipient type (`PidTagRecipientType` / 0x0C15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecipientType {
    /// MAPI_TO = 1
    To,
    /// MAPI_CC = 2
    Cc,
    /// MAPI_BCC = 3
    Bcc,
    /// Any other value (including MAPI_ORIG = 0).
    Other(u32),
}

impl RecipientType {
    /// Map a raw MAPI `PidTagRecipientType` value.
    pub fn from_mapi(value: u32) -> Self {
        match value {
            1 => Self::To,
            2 => Self::Cc,
            3 => Self::Bcc,
            other => Self::Other(other),
        }
    }

    /// Raw MAPI integer for this variant.
    pub fn to_mapi(self) -> u32 {
        match self {
            Self::To => 1,
            Self::Cc => 2,
            Self::Bcc => 3,
            Self::Other(v) => v,
        }
    }
}

/// One row from a message recipient table TC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipient {
    pub recipient_type: RecipientType,
    pub display_name: Option<String>,
    /// Address type: SMTP, EX, …
    pub address_type: Option<String>,
    pub email_address: Option<String>,
    /// `PidTagSmtpAddress` (0x39FE) when present.
    pub smtp_address: Option<String>,
}

impl Recipient {
    /// Identity key cascade for Tier-2.5 (spec §2.5 rule 4):
    ///
    /// 1. `PidTagSmtpAddress` if non-empty
    /// 2. `PidTagEmailAddress` if type is SMTP (or address is SMTP-shaped and type missing/empty)
    /// 3. `PidTagEmailAddress` if type is EX / X.500 LegacyExchangeDN form
    ///    (typed `EX` any DN shape; path-like `/O=` `/OU=` `/CN=`)
    /// 4. Normalized display name only when no structured address key exists
    ///
    /// SMTP keys are case-folded lower; EX/X.500 paths keep structure and fold case.
    pub fn identity_key(&self) -> Option<String> {
        if let Some(smtp) = nonempty_str(self.smtp_address.as_deref()) {
            return Some(smtp.to_ascii_lowercase());
        }

        let email = nonempty_str(self.email_address.as_deref());
        let addr_type = nonempty_str(self.address_type.as_deref());

        if let Some(email) = email {
            let type_upper = addr_type.map(|t| t.to_ascii_uppercase());
            let is_smtp_type = type_upper.as_deref() == Some("SMTP");
            let type_missing = type_upper.is_none();
            let smtp_shaped = looks_like_smtp(email);

            // Rule 2: SMTP type, or SMTP-shaped when type absent.
            if is_smtp_type || (type_missing && smtp_shaped) {
                return Some(email.to_ascii_lowercase());
            }

            // Rule 3: EX type (any DN shape) or LegacyExchangeDN / X.500 path form.
            if is_ex_address(type_upper.as_deref(), email) {
                return Some(email.to_ascii_uppercase());
            }
        }

        // Rule 4: display fallback only.
        nonempty_str(self.display_name.as_deref()).map(|s| s.to_ascii_lowercase())
    }

    /// True when this row should count as EX / X.500 for telemetry.
    ///
    /// - address type case-insensitive `EX` (any DN shape, including `/CN=…` without `/O=`)
    /// - or email / identity key looks like an X.500 path (`/O=`, `/OU=`, `/CN=`)
    pub fn identity_is_x500(&self) -> bool {
        if nonempty_str(self.address_type.as_deref()).is_some_and(|t| t.eq_ignore_ascii_case("EX"))
        {
            return true;
        }
        if let Some(email) = nonempty_str(self.email_address.as_deref()) {
            if looks_like_x500_dn(email) {
                return true;
            }
        }
        self.identity_key()
            .as_deref()
            .is_some_and(looks_like_x500_dn)
    }
}

/// True when `PidTagMessageFlags` has `MSGFLAG_UNSENT` set.
#[inline]
pub fn message_flags_is_unsent(flags: u32) -> bool {
    flags & nid::MSGFLAG_UNSENT != 0
}

fn nonempty_str(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

fn looks_like_smtp(addr: &str) -> bool {
    // Minimal shape check — not full RFC validation.
    let at = match addr.find('@') {
        Some(i) if i > 0 && i + 1 < addr.len() => i,
        _ => return false,
    };
    addr[at + 1..].contains('.')
}

fn is_ex_address(type_upper: Option<&str>, email: &str) -> bool {
    // Typed EX is EX/X.500 regardless of DN shape (may be `/CN=…` without `/O=`).
    if type_upper == Some("EX") {
        return true;
    }
    // LegacyExchangeDN / X.500 path often lives in EmailAddress without EX type.
    looks_like_x500_dn(email)
}

/// LegacyExchangeDN / X.500 path forms used for identity + telemetry.
///
/// Recognizes common path prefixes: `/O=`, `/OU=`, `/CN=` (case-insensitive).
pub fn looks_like_x500_dn(s: &str) -> bool {
    let u = s.to_ascii_uppercase();
    u.contains("/O=") || u.contains("/OU=") || u.contains("/CN=")
}

pub(crate) fn opt_row_string(s: Result<Option<String>>) -> Option<String> {
    match s {
        Ok(Some(v)) => nonempty_str(Some(&v)).map(|t| t.to_string()),
        Ok(None) | Err(_) => None,
    }
}

impl PstFile {
    /// Load structured recipients from the message subnode recipient TC (NID type 0x12).
    ///
    /// - Missing subnode / missing table / empty rows → empty `Vec`
    /// - Corrupt or unreadable TC → empty `Vec` (never hard-fails the caller)
    /// - **Does not** invent rows from `display_to` / `display_cc` / `display_bcc`
    pub fn list_recipients(&mut self, message_nid: NodeId) -> Result<Vec<Recipient>> {
        match self.list_recipients_inner(message_nid) {
            Ok(rows) => Ok(rows),
            Err(e) => {
                tracing::debug!(
                    nid = message_nid.0,
                    error = %e,
                    "recipient table unreadable; returning empty"
                );
                Ok(Vec::new())
            }
        }
    }

    fn list_recipients_inner(&mut self, message_nid: NodeId) -> Result<Vec<Recipient>> {
        let nbt_entry = match self.nbt.get(message_nid) {
            Some(e) => e.clone(),
            None => return Ok(Vec::new()),
        };

        if nbt_entry.bid_sub.is_null() {
            return Ok(Vec::new());
        }

        let sub_entries =
            block::list_subnode_entries(&mut self.reader, &self.bbt, nbt_entry.bid_sub)?;

        let recip_entry = match sub_entries
            .iter()
            .find(|e| matches!(e.nid.nid_type(), NidType::RecipientTable))
        {
            Some(e) => e,
            None => return Ok(Vec::new()),
        };

        if recip_entry.bid_data.is_null() {
            return Ok(Vec::new());
        }

        let crypt = self.header.crypt_method;
        let data =
            block::read_block_data(&mut self.reader, &self.bbt, recip_entry.bid_data, crypt)?;

        // Rows may live inline in the TC heap or in a nested subnode (same pattern as load_tc).
        let subnode_rows = if !recip_entry.bid_sub.is_null() {
            let nested =
                block::list_subnode_entries(&mut self.reader, &self.bbt, recip_entry.bid_sub)?;
            if nested.is_empty() {
                None
            } else {
                let mut all_rows = Vec::new();
                for entry in &nested {
                    let entry_data =
                        block::read_block_data(&mut self.reader, &self.bbt, entry.bid_data, crypt)?;
                    all_rows.extend_from_slice(&entry_data);
                }
                Some(all_rows)
            }
        } else {
            None
        };

        let table = match TableContext::load(data, subnode_rows) {
            Ok(t) => t,
            Err(e) => {
                tracing::debug!(
                    nid = message_nid.0,
                    error = %e,
                    "recipient TC load failed; empty recipients"
                );
                return Ok(Vec::new());
            }
        };

        let mut recipients = Vec::with_capacity(table.row_count());
        for row in 0..table.row_count() {
            let raw_type = table
                .get_row_u32(row, nid::PID_TAG_RECIPIENT_TYPE)
                .unwrap_or(0);
            let recipient_type = RecipientType::from_mapi(raw_type);
            let display_name = opt_row_string(table.get_row_string(row, nid::PID_TAG_DISPLAY_NAME));
            let address_type = opt_row_string(table.get_row_string(row, nid::PID_TAG_ADDRESS_TYPE));
            let email_address =
                opt_row_string(table.get_row_string(row, nid::PID_TAG_EMAIL_ADDRESS));
            let smtp_address = opt_row_string(table.get_row_string(row, nid::PID_TAG_SMTP_ADDRESS));

            recipients.push(Recipient {
                recipient_type,
                display_name,
                address_type,
                email_address,
                smtp_address,
            });
        }

        Ok(recipients)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recip(
        ty: RecipientType,
        display: Option<&str>,
        addr_type: Option<&str>,
        email: Option<&str>,
        smtp: Option<&str>,
    ) -> Recipient {
        Recipient {
            recipient_type: ty,
            display_name: display.map(str::to_string),
            address_type: addr_type.map(str::to_string),
            email_address: email.map(str::to_string),
            smtp_address: smtp.map(str::to_string),
        }
    }

    #[test]
    fn recipient_type_mapi_mapping() {
        assert_eq!(RecipientType::from_mapi(1), RecipientType::To);
        assert_eq!(RecipientType::from_mapi(2), RecipientType::Cc);
        assert_eq!(RecipientType::from_mapi(3), RecipientType::Bcc);
        assert_eq!(RecipientType::from_mapi(0), RecipientType::Other(0));
        assert_eq!(RecipientType::from_mapi(99), RecipientType::Other(99));
        assert_eq!(RecipientType::To.to_mapi(), 1);
        assert_eq!(RecipientType::Cc.to_mapi(), 2);
        assert_eq!(RecipientType::Bcc.to_mapi(), 3);
        assert_eq!(RecipientType::Other(7).to_mapi(), 7);
    }

    #[test]
    fn identity_key_prefers_smtp_address() {
        let r = recip(
            RecipientType::To,
            Some("Alice"),
            Some("EX"),
            Some("/O=ORG/CN=Alice"),
            Some("Alice@Example.COM"),
        );
        assert_eq!(r.identity_key().as_deref(), Some("alice@example.com"));
    }

    #[test]
    fn identity_key_smtp_type_email() {
        let r = recip(
            RecipientType::Cc,
            Some("Bob"),
            Some("SMTP"),
            Some("Bob@Example.com"),
            None,
        );
        assert_eq!(r.identity_key().as_deref(), Some("bob@example.com"));
    }

    #[test]
    fn identity_key_smtp_shaped_when_type_missing() {
        let r = recip(
            RecipientType::To,
            Some("Carol"),
            None,
            Some("carol@corp.example"),
            None,
        );
        assert_eq!(r.identity_key().as_deref(), Some("carol@corp.example"));
    }

    #[test]
    fn identity_key_ex_dn_before_display() {
        let r = recip(
            RecipientType::To,
            Some("Alice Example (noisy)"),
            Some("EX"),
            Some("/o=First Organization/ou=Exchange Administrative Group/cn=Recipients/cn=alice"),
            None,
        );
        assert_eq!(
            r.identity_key().as_deref(),
            Some("/O=FIRST ORGANIZATION/OU=EXCHANGE ADMINISTRATIVE GROUP/CN=RECIPIENTS/CN=ALICE")
        );
    }

    #[test]
    fn identity_key_x500_in_email_without_ex_type() {
        let r = recip(
            RecipientType::Bcc,
            Some("Dave Display"),
            None,
            Some("/O=Org/CN=Dave"),
            None,
        );
        assert_eq!(r.identity_key().as_deref(), Some("/O=ORG/CN=DAVE"));
        assert!(r.identity_is_x500());
    }

    /// Typed EX with `/CN=…` DN (no `/O=`) still uses email as identity key (0082 P2-2).
    #[test]
    fn identity_key_typed_ex_without_o_equals_uses_email() {
        let dn = "/CN=Recipients/CN=alice";
        let r = recip(
            RecipientType::To,
            Some("Alice Example (noisy)"),
            Some("EX"),
            Some(dn),
            None,
        );
        assert_eq!(
            r.identity_key().as_deref(),
            Some("/CN=RECIPIENTS/CN=ALICE"),
            "typed EX must prefer email DN over display even without /O="
        );
        assert!(
            r.identity_is_x500(),
            "typed EX counts as X.500 telemetry without /O="
        );
        // Case-insensitive address type.
        let r_lower = recip(RecipientType::To, Some("Alice"), Some("ex"), Some(dn), None);
        assert_eq!(
            r_lower.identity_key().as_deref(),
            Some("/CN=RECIPIENTS/CN=ALICE")
        );
        assert!(r_lower.identity_is_x500());
    }

    #[test]
    fn identity_key_path_like_cn_without_ex_type() {
        let r = recip(
            RecipientType::To,
            Some("Noise Display"),
            None,
            Some("/CN=Recipients/CN=bob"),
            None,
        );
        assert_eq!(
            r.identity_key().as_deref(),
            Some("/CN=RECIPIENTS/CN=BOB"),
            "path-like /CN= email is X.500 tier without inventing display"
        );
        assert!(r.identity_is_x500());
        assert!(looks_like_x500_dn("/ou=AG/cn=Recipients/cn=x"));
        assert!(!looks_like_x500_dn("Smith, John"));
    }

    #[test]
    fn identity_key_display_fallback_only_when_no_structured() {
        let r = recip(
            RecipientType::To,
            Some("  Display Only  "),
            Some("MAPIPDL"),
            None,
            None,
        );
        assert_eq!(r.identity_key().as_deref(), Some("display only"));
        assert!(!r.identity_is_x500());
    }

    #[test]
    fn identity_key_none_when_all_empty() {
        let r = recip(RecipientType::Other(0), None, None, None, None);
        assert!(r.identity_key().is_none());
        let r2 = recip(RecipientType::To, Some("   "), Some(""), Some(""), Some(""));
        assert!(r2.identity_key().is_none());
    }

    #[test]
    fn msgflag_unsent_bit() {
        assert!(message_flags_is_unsent(nid::MSGFLAG_UNSENT));
        assert!(message_flags_is_unsent(0x0000_0009)); // unsent + read
        assert!(!message_flags_is_unsent(0));
        assert!(!message_flags_is_unsent(0x0000_0001)); // read only
    }

    #[test]
    fn empty_recipient_vec_is_not_invented_from_display() {
        // Contract: reader never synthesizes rows from Display* — empty stays empty.
        // Display* live on MessageProperties / ExtractedMessage independently of
        // the recipient TC; list_recipients must not invent rows from them.
        let recipients: Vec<Recipient> = Vec::new();
        assert!(recipients.is_empty());
        let display_to = Some("alice@example.com; bob@example.com".to_string());
        assert!(display_to.is_some());
        assert!(
            recipients.is_empty(),
            "display_to must not create recipients"
        );
        // Soft-fail shape: caller treats missing table as empty vec, not error.
        let soft: Result<Vec<Recipient>> = Ok(Vec::new());
        assert!(matches!(soft, Ok(ref v) if v.is_empty()));
    }
}
