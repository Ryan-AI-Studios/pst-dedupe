//! Built-in cull presets (code constants).

use crate::rules::{
    CullRules, DateRule, EmptyRule, FamilyPolicy, ListMode, MimePrefixesRule, MissingDatePolicy,
    StringListRule,
};

/// Built-in preset name: cull exact duplicates only.
pub const PRESET_UNIQUE_ONLY: &str = "unique_only";
/// Built-in: unique_only + absolute family keep-children.
pub const PRESET_UNIQUE_PLUS_FAMILY: &str = "unique_plus_family";
/// Built-in: unique_only + date window template (operator fills bounds).
pub const PRESET_DATE_WINDOW: &str = "date_window";
/// Built-in: unique_only + zero-size empty + executable mime/category exclude.
pub const PRESET_NOISE_LIGHT: &str = "noise_light";

/// All built-in preset names in stable order.
pub const BUILTIN_PRESET_NAMES: &[&str] = &[
    PRESET_UNIQUE_ONLY,
    PRESET_UNIQUE_PLUS_FAMILY,
    PRESET_DATE_WINDOW,
    PRESET_NOISE_LIGHT,
];

/// Base unique_only rules (shared foundation).
fn unique_only_base() -> CullRules {
    CullRules {
        exclude_exact_duplicates: true,
        family_policy: FamilyPolicy::KeepChildrenWithIncludedParent,
        ..CullRules::default()
    }
}

/// Built-in `unique_only` rules.
pub fn unique_only() -> CullRules {
    unique_only_base()
}

/// Built-in `unique_plus_family` — same as unique_only with explicit family policy.
pub fn unique_plus_family() -> CullRules {
    let mut r = unique_only_base();
    r.family_policy = FamilyPolicy::KeepChildrenWithIncludedParent;
    r
}

/// Built-in `date_window` template — date enabled, bounds null until filled.
pub fn date_window() -> CullRules {
    let mut r = unique_only_base();
    r.date = DateRule {
        enabled: true,
        field: crate::rules::DateField::BestEffort,
        start: None,
        end: None,
        missing_policy: MissingDatePolicy::Include,
    };
    r
}

/// Built-in `noise_light` — unique + empty zero_size + exe mime prefixes
/// + `file_category=executable` exclude (taxonomy_v1 / 0037).
pub fn noise_light() -> CullRules {
    let mut r = unique_only_base();
    r.empty = EmptyRule {
        enabled: true,
        zero_size: true,
        no_text_and_no_native: false,
    };
    r.mime_prefixes = MimePrefixesRule {
        enabled: true,
        mode: ListMode::Exclude,
        values: vec![
            "application/x-msdownload".into(),
            "application/x-dosexec".into(),
        ],
    };
    r.file_categories = StringListRule {
        enabled: true,
        mode: ListMode::Exclude,
        values: vec!["executable".into()],
    };
    r
}

/// Resolve a built-in preset by name (case-sensitive).
pub fn builtin_rules(name: &str) -> Option<CullRules> {
    match name {
        PRESET_UNIQUE_ONLY => Some(unique_only()),
        PRESET_UNIQUE_PLUS_FAMILY => Some(unique_plus_family()),
        PRESET_DATE_WINDOW => Some(date_window()),
        PRESET_NOISE_LIGHT => Some(noise_light()),
        _ => None,
    }
}

/// Serialize a built-in preset's rules to JSON.
pub fn builtin_rules_json(name: &str) -> Option<String> {
    builtin_rules(name).map(|r| serde_json::to_string(&r).expect("serialize preset"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_builtins_validate() {
        for name in BUILTIN_PRESET_NAMES {
            let r = builtin_rules(name).expect(name);
            // date_window has enabled date with no bounds — still valid.
            r.validate().unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }
}
