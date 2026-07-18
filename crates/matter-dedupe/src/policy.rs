//! Family and matching policy enums.

use serde::{Deserialize, Serialize};

/// How to treat attachment children when a parent email is marked duplicate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FamilyPolicy {
    /// Mark direct attachments `duplicate` / tier `family` (default).
    #[default]
    SuppressChildrenWithParent,
    /// Parents only; leave attach `dedup_role` null.
    ParentsOnly,
}

impl FamilyPolicy {
    /// Wire form used in audit/checkpoint payloads.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SuppressChildrenWithParent => "suppress_children_with_parent",
            Self::ParentsOnly => "parents_only",
        }
    }
}
