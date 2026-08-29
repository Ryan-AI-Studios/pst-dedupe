//! Create matter under parent/name (Desk-shaped; unencrypted only).

use camino::{Utf8Path, Utf8PathBuf};
use matter_core::Matter;

use crate::error::CommandError;
use crate::params::validate_matter_name;

pub fn create_matter_under(parent: &Utf8Path, name: &str) -> Result<Utf8PathBuf, CommandError> {
    let name = validate_matter_name(name).map_err(CommandError::failed)?;
    let root = parent.join(name);
    Matter::create(&root, name).map_err(|e| CommandError::failed(e.to_string()))?;
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn create_writes_matter_db_under_parent_name() {
        let tmp = tempdir().expect("tempdir");
        let parent = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8");
        let root = create_matter_under(&parent, "SmokeCase").expect("create");
        assert_eq!(root, parent.join("SmokeCase"));
        assert!(root.join("matter.db").as_std_path().exists());
    }

    #[test]
    fn invalid_name_rejected() {
        let tmp = tempdir().expect("tempdir");
        let parent = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8");
        assert!(create_matter_under(&parent, "").is_err());
        assert!(create_matter_under(&parent, "a/b").is_err());
        assert!(!parent.join("a").as_std_path().exists());
    }
}
