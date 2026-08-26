//! Machine-readable writer fidelity contract (track 0080 §3.2).
//!
//! Derived from `docs/pst-writer-fidelity-v1.md`. The contract is an **allowlist**:
//! a property absent from the contract that differs is `unexplained_loss`.

use serde::{Deserialize, Serialize};

/// Contract version string embedded in `qc_report_v1`.
pub const FIDELITY_CONTRACT_VERSION: &str = "fidelity_contract_v1";

/// How the writer treats a property / capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractStatus {
    /// Must round-trip; difference ⇒ `defect`.
    Preserved,
    /// May be missing when ledger / fidelity flags explain it.
    BestEffort,
    /// Intentionally not written; difference ⇒ `known_gap` (never fails).
    DroppedByDesign,
}

/// One declared property in the contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractProperty {
    pub name: &'static str,
    pub status: ContractStatus,
    pub reason: &'static str,
}

/// Classification of an observed source↔output difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingClass {
    Defect,
    UnexplainedLoss,
    KnownGap,
    Explained,
}

/// Full contract table (versioned allowlist).
#[derive(Debug, Clone)]
pub struct FidelityContract {
    pub version: &'static str,
    pub properties: &'static [ContractProperty],
}

impl FidelityContract {
    /// Built-in `fidelity_contract_v1` from the writer fidelity matrix + 0080 decisions.
    pub fn v1() -> Self {
        Self {
            version: FIDELITY_CONTRACT_VERSION,
            properties: CONTRACT_V1,
        }
    }

    /// Look up a property by stable name. `None` ⇒ allowlist miss.
    pub fn get(&self, name: &str) -> Option<&ContractProperty> {
        self.properties.iter().find(|p| p.name == name)
    }

    /// Classify a difference given contract status and whether an explanation exists.
    ///
    /// - `explained` is true when a 0073 ledger row or 0065/0075 fidelity flag covers the loss.
    /// - Property absent from the contract ⇒ `unexplained_loss` (allowlist fail-closed).
    pub fn classify(
        &self,
        property: &str,
        explained: bool,
    ) -> (FindingClass, Option<ContractStatus>) {
        match self.get(property) {
            None => (FindingClass::UnexplainedLoss, None),
            Some(p) => {
                let class = match p.status {
                    ContractStatus::Preserved => FindingClass::Defect,
                    ContractStatus::BestEffort if explained => FindingClass::Explained,
                    ContractStatus::BestEffort => FindingClass::UnexplainedLoss,
                    ContractStatus::DroppedByDesign => FindingClass::KnownGap,
                };
                (class, Some(p.status))
            }
        }
    }
}

/// Standalone classifier for tests / external call sites (same rules as [`FidelityContract::classify`]).
pub fn classify_difference(
    contract_status: Option<ContractStatus>,
    explained: bool,
) -> FindingClass {
    match contract_status {
        None => FindingClass::UnexplainedLoss,
        Some(ContractStatus::Preserved) => FindingClass::Defect,
        Some(ContractStatus::BestEffort) if explained => FindingClass::Explained,
        Some(ContractStatus::BestEffort) => FindingClass::UnexplainedLoss,
        Some(ContractStatus::DroppedByDesign) => FindingClass::KnownGap,
    }
}

const CONTRACT_V1: &[ContractProperty] = &[
    ContractProperty {
        name: "unicode_unencrypted_pst",
        status: ContractStatus::Preserved,
        reason: "wVer=23, bCryptMethod=0 production write path",
    },
    ContractProperty {
        name: "body_plain",
        status: ContractStatus::Preserved,
        reason: "Full plain body via XBLOCK/XXBLOCK; no silent truncation",
    },
    ContractProperty {
        name: "body_html",
        status: ContractStatus::Preserved,
        reason: "HTML body as PtypBinary when present on source",
    },
    ContractProperty {
        name: "body_unavailable",
        status: ContractStatus::BestEffort,
        reason: "When body_unavailable, no body is written (never invented)",
    },
    ContractProperty {
        name: "PidTagInternetMessageId",
        status: ContractStatus::Preserved,
        reason: "Written when present on source",
    },
    ContractProperty {
        name: "PidTagSubject",
        status: ContractStatus::Preserved,
        reason: "Written from source subject",
    },
    ContractProperty {
        name: "PidTagSenderEmailAddress",
        status: ContractStatus::Preserved,
        reason: "Written when present",
    },
    ContractProperty {
        name: "PidTagDisplayTo",
        status: ContractStatus::Preserved,
        reason: "Display To string when present",
    },
    ContractProperty {
        name: "PidTagDisplayCc",
        status: ContractStatus::Preserved,
        reason: "0080 §3.11 — CC display string preserved (was silently dropped pre-0080)",
    },
    ContractProperty {
        name: "PidTagDisplayBcc",
        status: ContractStatus::DroppedByDesign,
        reason: "0082 BCC disclosure: default omit Bcc rows + PidTagDisplayBcc; opt-in via --include-bcc-recipients (over-disclosure on consolidated unique-PST)",
    },
    ContractProperty {
        name: "display_bcc",
        status: ContractStatus::DroppedByDesign,
        reason: "Alias of PidTagDisplayBcc — DroppedByDesign unless --include-bcc-recipients; ledger column bcc_suppressed records omissions",
    },
    ContractProperty {
        name: "PidTagClientSubmitTime",
        status: ContractStatus::Preserved,
        reason: "FILETIME passthrough when present",
    },
    ContractProperty {
        name: "PidTagMessageClass",
        status: ContractStatus::Preserved,
        reason: "Defaults to IPM.Note when absent",
    },
    ContractProperty {
        name: "attachment_by_value",
        status: ContractStatus::Preserved,
        reason: "ATTACH_BY_VALUE file attaches + payload (0069)",
    },
    ContractProperty {
        name: "attachment_payload_sha256",
        status: ContractStatus::Preserved,
        reason: "Payload bytes must match source when written",
    },
    ContractProperty {
        name: "attachment_stream_soft_fail",
        status: ContractStatus::BestEffort,
        reason: "Missing/unreadable attach soft-fails with 0073 ledger event",
    },
    ContractProperty {
        name: "attachment_embedded",
        status: ContractStatus::BestEffort,
        reason: "Embedded messages within max_embedded_depth; deeper → ledger",
    },
    ContractProperty {
        name: "folder_path_preservation",
        status: ContractStatus::Preserved,
        reason: "PreservePaths under IPM_SUBTREE (0069)",
    },
    ContractProperty {
        name: "folder_tree_structure",
        status: ContractStatus::Preserved,
        reason: "Output folder tree must match expected keep-set layout (not only summed counts)",
    },
    ContractProperty {
        name: "multi_source_prefix",
        status: ContractStatus::Preserved,
        reason: "0095: unique-pst pre-seeds known_source_paths so prefixes are stable from message 1 when ≥2 sources (D-0070 closed). Unseeded direct-writer callers may still discover prefixes from stream order",
    },
    ContractProperty {
        name: "recipient_table",
        status: ContractStatus::Preserved,
        reason: "0082: MS-PST recipient TC (template 0x692) written per message; empty table when source had none. BCC rows remain DroppedByDesign unless --include-bcc-recipients",
    },
    ContractProperty {
        name: "named_properties",
        status: ContractStatus::BestEffort,
        reason: "0092: allowlisted NPMAP (PSETID_Attachment ProviderType/Url/PermissionType) written when used; empty stub when unused; full named-prop encyclopedia still out of scope",
    },
    ContractProperty {
        name: "PidTagRtfCompressed",
        status: ContractStatus::DroppedByDesign,
        reason: "RTF never written in v1",
    },
    ContractProperty {
        name: "encrypted_permute_output",
        status: ContractStatus::DroppedByDesign,
        reason: "Unencrypted output only",
    },
    ContractProperty {
        name: "ansi_pst",
        status: ContractStatus::DroppedByDesign,
        reason: "Unicode PST only; ANSI never",
    },
    ContractProperty {
        name: "cloud_modern_attachments",
        // BestEffort: attach-table + body-inline document-shaped detect offline; payload never Preserved.
        status: ContractStatus::BestEffort,
        reason: "0084+0085+0088+0092: attachment-table web-ref / OneDrive-SharePoint cloud attaches are detected (ATTACH_CLOUD_LINK + incomplete for Mode A) AND body-inline document-shaped SharePoint/OneDrive URLs are detected offline (export_body_cloud_links.csv / body_cloud_link_count; commercial + US GCC High/DoD hosts). Offline payload is NOT collected and must never be claimed Preserved. Body hits do NOT set is_attach_incomplete / Mode A promote (known gap: physical attach peer is NOT preferred over HTML-inline-only via Mode A). D-0085-sovereign-cloud-hosts closed in 0088; residual D-0088-usgovcloud-microsoft-tld for future .microsoft TLD content hosts. Pointer metadata + allowlisted named props (ProviderType/Url/Permission when known) preserved on unique-PST; D-0084-cloud-named-prop-write closed in 0092 (encyclopedia residual out of scope)",
    },
    ContractProperty {
        name: "PidNameAttachmentProviderType",
        status: ContractStatus::Preserved,
        reason: "0092: re-emitted on unique-PST cloud pointer attaches when source had a known provider (NPMAP allowlisted write + attach PC); absence on source is not a defect; provider string open (OneDrivePro/OneDriveConsumer/other). Payload never Preserved offline",
    },
    ContractProperty {
        name: "PidNameAttachmentPermissionType",
        status: ContractStatus::Preserved,
        reason: "0096: re-emitted on unique-PST cloud pointer attaches when source had AttachmentPermissionType (PtypInteger32; open-world preserve any i32); absence on source is not a defect. Payload never Preserved offline",
    },
    ContractProperty {
        name: "message_content_digest",
        status: ContractStatus::Preserved,
        reason: "Aggregate MID+subject+recipients+body+attach-payload digest must match source when compared",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_unknown_is_unexplained_loss() {
        let c = FidelityContract::v1();
        let (class, st) = c.classify("totally_unknown_prop", false);
        assert_eq!(class, FindingClass::UnexplainedLoss);
        assert!(st.is_none());
    }

    #[test]
    fn preserved_diff_is_defect() {
        let c = FidelityContract::v1();
        let (class, _) = c.classify("PidTagDisplayCc", false);
        assert_eq!(class, FindingClass::Defect);
    }

    #[test]
    fn bcc_is_known_gap() {
        let c = FidelityContract::v1();
        let (class, st) = c.classify("PidTagDisplayBcc", false);
        assert_eq!(class, FindingClass::KnownGap);
        assert_eq!(st, Some(ContractStatus::DroppedByDesign));
        let (class2, _) = c.classify("display_bcc", false);
        assert_eq!(class2, FindingClass::KnownGap);
    }

    #[test]
    fn best_effort_explained_vs_not() {
        let c = FidelityContract::v1();
        let (a, _) = c.classify("attachment_stream_soft_fail", true);
        assert_eq!(a, FindingClass::Explained);
        let (b, _) = c.classify("attachment_stream_soft_fail", false);
        assert_eq!(b, FindingClass::UnexplainedLoss);
    }

    #[test]
    fn cloud_attachments_not_silently_preserved() {
        let c = FidelityContract::v1();
        let p = c.get("cloud_modern_attachments").expect("present");
        // 0084: BestEffort (detect + ledger) — never Preserved for offline payload.
        assert_ne!(p.status, ContractStatus::Preserved);
        assert_eq!(
            p.status,
            ContractStatus::BestEffort,
            "cloud payload must stay BestEffort, got {:?}",
            p.status
        );
        assert!(
            p.reason.contains("attachment-table") || p.reason.contains("ATTACH_CLOUD_LINK"),
            "reason must state attach-table scope: {}",
            p.reason
        );
        assert!(
            p.reason.contains("body-inline") || p.reason.contains("export_body_cloud_links"),
            "reason must state body-inline detect closed: {}",
            p.reason
        );
        assert!(
            !p.reason.contains("D-0084-body-cloud-links"),
            "body residual must be closed (no open D-0084-body-cloud-links): {}",
            p.reason
        );
        assert!(
            p.reason.contains("Mode A") || p.reason.contains("is_attach_incomplete"),
            "reason must state Mode A body-only known gap: {}",
            p.reason
        );
        assert!(
            p.reason.contains("D-0085-sovereign-cloud-hosts") && p.reason.contains("closed"),
            "reason must note D-0085 closed: {}",
            p.reason
        );
        assert!(
            p.reason.contains("D-0088-usgovcloud-microsoft-tld") || p.reason.contains(".microsoft"),
            "reason must name .microsoft TLD residual: {}",
            p.reason
        );
        let provider = c.get("PidNameAttachmentProviderType").expect("present");
        assert_eq!(
            provider.status,
            ContractStatus::Preserved,
            "0092 writes ProviderType back when source had it"
        );
        assert!(
            provider.reason.contains("0092") || provider.reason.contains("re-emit"),
            "reason should cite 0092 write-back: {}",
            provider.reason
        );
        let permission = c.get("PidNameAttachmentPermissionType").expect("present");
        assert_eq!(
            permission.status,
            ContractStatus::Preserved,
            "0096 writes PermissionType back when source had it"
        );
        assert!(
            permission.reason.contains("0096") || permission.reason.contains("PermissionType"),
            "reason should cite 0096 PermissionType: {}",
            permission.reason
        );
        let named = c.get("named_properties").expect("present");
        assert_eq!(named.status, ContractStatus::BestEffort);
        assert!(
            p.reason.contains("D-0084-cloud-named-prop-write") && p.reason.contains("closed"),
            "cloud reason must note D-0084 closed: {}",
            p.reason
        );
    }

    /// 0082 DoD-6: recipient_table is Preserved (not DroppedByDesign).
    #[test]
    fn recipient_table_preserved() {
        let c = FidelityContract::v1();
        let p = c.get("recipient_table").expect("present");
        assert_eq!(p.status, ContractStatus::Preserved);
        assert!(
            p.reason.contains("0082") || p.reason.contains("0x692"),
            "reason should cite 0082/template: {}",
            p.reason
        );
        // Diff on preserved recipient_table is a defect.
        let (class, _) = c.classify("recipient_table", false);
        assert_eq!(class, FindingClass::Defect);
        // BCC still dropped by design.
        let (bcc_class, _) = c.classify("PidTagDisplayBcc", false);
        assert_eq!(bcc_class, FindingClass::KnownGap);
    }

    #[test]
    fn classify_difference_none_status() {
        assert_eq!(
            classify_difference(None, false),
            FindingClass::UnexplainedLoss
        );
    }
}
