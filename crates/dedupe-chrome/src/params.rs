//! Matter name validation (mirrors Desk `params::validate_matter_name`; no desk dep).

pub fn validate_matter_name(name: &str) -> Result<&str, String> {
    let t = name.trim();
    if t.is_empty() {
        return Err("Matter name cannot be empty.".into());
    }
    if t.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
        return Err("Matter name contains invalid characters.".into());
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_trimmed_simple_name() {
        assert_eq!(validate_matter_name("  Case-42  ").unwrap(), "Case-42");
    }

    #[test]
    fn rejects_empty_and_path_chars() {
        assert!(validate_matter_name("").is_err());
        assert!(validate_matter_name("a/b").is_err());
        assert!(validate_matter_name(r"a\b").is_err());
    }
}
