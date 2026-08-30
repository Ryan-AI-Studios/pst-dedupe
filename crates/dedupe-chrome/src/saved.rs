//! Saved search list / upsert for the queue filter bar.

use matter_core::{SavedSearch, SavedSearchInput};
use serde::{Deserialize, Serialize};

use crate::error::CommandError;
use crate::open_root::{open_matter_read, open_matter_write};

#[derive(Debug, Clone, Deserialize)]
pub struct SavedSearchesListArgs {
    pub root: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SavedSearchDto {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub scope: String,
    pub filter_json: String,
    pub keyword: Option<String>,
}

impl From<SavedSearch> for SavedSearchDto {
    fn from(s: SavedSearch) -> Self {
        Self {
            id: s.id,
            name: s.name,
            description: s.description,
            scope: s.scope,
            filter_json: s.filter_json,
            keyword: s.keyword,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SavedSearchUpsertArgs {
    pub root: String,
    pub name: String,
    pub filter_json: String,
    #[serde(default)]
    pub keyword: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
}

pub fn saved_searches_list_blocking(
    args: SavedSearchesListArgs,
) -> Result<Vec<SavedSearchDto>, CommandError> {
    let matter = open_matter_read(&args.root)?;
    let list = matter
        .list_saved_searches()
        .map_err(|e| CommandError::failed(e.to_string()))?;
    Ok(list.into_iter().map(SavedSearchDto::from).collect())
}

pub fn saved_search_upsert_blocking(
    args: SavedSearchUpsertArgs,
) -> Result<SavedSearchDto, CommandError> {
    let matter = open_matter_write(&args.root)?;
    let saved = matter
        .upsert_saved_search(SavedSearchInput {
            id: args.id,
            name: args.name,
            description: args.description,
            filter_json: args.filter_json,
            keyword: args.keyword,
            created_by: Some("chrome".into()),
        })
        .map_err(|e| CommandError::failed(e.to_string()))?;
    Ok(SavedSearchDto::from(saved))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create::create_matter_under;
    use matter_core::FilterSpec;
    use tempfile::tempdir;

    fn utf8_tmp(tmp: &tempfile::TempDir) -> camino::Utf8PathBuf {
        camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8")
    }

    #[test]
    fn saved_search_upsert_list_roundtrip() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "SavedRound").expect("create");
        let filter_json = serde_json::to_string(&FilterSpec::preset_uncoded()).expect("json");
        let saved = saved_search_upsert_blocking(SavedSearchUpsertArgs {
            root: root.to_string(),
            name: "My Unreviewed".into(),
            filter_json: filter_json.clone(),
            keyword: Some("invoice".into()),
            description: None,
            id: None,
        })
        .expect("upsert");
        assert_eq!(saved.name, "My Unreviewed");
        assert_eq!(saved.keyword.as_deref(), Some("invoice"));

        let list = saved_searches_list_blocking(SavedSearchesListArgs {
            root: root.to_string(),
        })
        .expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, saved.id);
        assert_eq!(list[0].filter_json, filter_json);
    }

    #[test]
    fn empty_name_rejected() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "SavedEmpty").expect("create");
        let filter_json = serde_json::to_string(&FilterSpec::review_corpus()).expect("json");
        let err = saved_search_upsert_blocking(SavedSearchUpsertArgs {
            root: root.to_string(),
            name: "  ".into(),
            filter_json,
            keyword: None,
            description: None,
            id: None,
        })
        .expect_err("empty name");
        assert_eq!(err.kind, "failed");
    }
}
