//! `review_document_body` — capped CAS text/html for Native and Text panes.

use serde::{Deserialize, Serialize};

use crate::error::{map_core, CommandError};
use crate::html_strip::html_to_review_text;
use crate::open_root::open_matter_read;

pub const BODY_DISPLAY_CAP_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize)]
pub struct ReviewDocumentBodyArgs {
    pub root: String,
    pub item_id: String,
    pub pane: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReviewDocumentBodyResponse {
    pub item_id: String,
    pub pane: String,
    pub text: String,
    pub truncated: bool,
    pub empty: bool,
    pub digest: Option<String>,
}

pub fn review_document_body_blocking(
    args: ReviewDocumentBodyArgs,
) -> Result<ReviewDocumentBodyResponse, CommandError> {
    if args.item_id.trim().is_empty() {
        return Err(CommandError::not_found("item not found: ".to_string()));
    }
    let pane = args.pane.trim();
    if pane != "native" && pane != "text" {
        return Err(CommandError::failed("pane must be \"native\" or \"text\""));
    }
    let matter = open_matter_read(&args.root)?;
    let item = matter.get_item(&args.item_id).map_err(map_core)?;

    let (digest, empty_copy, strip_html) = if pane == "text" {
        (item.text_sha256.clone(), "No extracted text", false)
    } else if let Some(html) = item.html_sha256.clone() {
        (Some(html), "No native/extracted body", true)
    } else {
        (item.text_sha256.clone(), "No native/extracted body", false)
    };

    let Some(digest) = digest else {
        return Ok(ReviewDocumentBodyResponse {
            item_id: args.item_id,
            pane: pane.to_string(),
            text: empty_copy.into(),
            truncated: false,
            empty: true,
            digest: None,
        });
    };

    let (raw, truncated) = read_capped_text(&matter, &digest)?;
    let text = if strip_html {
        html_to_review_text(&raw)
    } else {
        raw
    };
    let text = cap_string(text);
    Ok(ReviewDocumentBodyResponse {
        item_id: args.item_id,
        pane: pane.to_string(),
        text,
        truncated,
        empty: false,
        digest: Some(digest),
    })
}

fn read_capped_text(
    matter: &matter_core::Matter,
    digest: &str,
) -> Result<(String, bool), CommandError> {
    let len = matter.cas_len(digest).map_err(map_core)?;
    let truncated = len > BODY_DISPLAY_CAP_BYTES as u64;
    let bytes = matter
        .read_cas_prefix(digest, BODY_DISPLAY_CAP_BYTES)
        .map_err(map_core)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok((text, truncated))
}

fn cap_string(text: String) -> String {
    if text.len() <= BODY_DISPLAY_CAP_BYTES {
        return text;
    }
    let mut end = BODY_DISPLAY_CAP_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create::create_matter_under;
    use matter_core::{
        item_role, item_status, ItemInput, ItemUpdate, Matter, DEFAULT_REVIEW_SET_NAME,
    };
    use tempfile::tempdir;

    fn utf8_tmp(tmp: &tempfile::TempDir) -> camino::Utf8PathBuf {
        camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8")
    }

    fn seed_item_with_cas(
        root: &camino::Utf8Path,
        text: Option<&[u8]>,
        html: Option<&[u8]>,
        subject: &str,
    ) {
        let matter = Matter::open(root).expect("open");
        let family = matter.insert_family("").expect("family");
        let text_sha = text.map(|b| matter.put_bytes(b).expect("text cas"));
        let html_sha = html.map(|b| matter.put_bytes(b).expect("html cas"));
        matter
            .insert_item(ItemInput {
                id: Some("itm_0000".into()),
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::PARENT.into()),
                family_id: Some(family.id.clone()),
                subject: Some(subject.into()),
                text_sha256: text_sha,
                html_sha256: html_sha,
                ..Default::default()
            })
            .expect("parent");
        matter
            .insert_item(ItemInput {
                id: Some("itm_0001".into()),
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::ATTACHMENT.into()),
                family_id: Some(family.id.clone()),
                parent_item_id: Some("itm_0000".into()),
                subject: Some("A".into()),
                ..Default::default()
            })
            .expect("c1");
        matter
            .insert_item(ItemInput {
                id: Some("itm_0002".into()),
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::ATTACHMENT.into()),
                family_id: Some(family.id.clone()),
                parent_item_id: Some("itm_0000".into()),
                subject: Some("B".into()),
                ..Default::default()
            })
            .expect("c2");
        let set = matter
            .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
            .expect("set");
        for (i, id) in ["itm_0000", "itm_0001", "itm_0002"].iter().enumerate() {
            matter
                .update_item(
                    id,
                    ItemUpdate {
                        in_review: Some(Some(1)),
                        review_set_id: Some(Some(set.id.clone())),
                        review_order: Some(Some(i as i64)),
                        ..Default::default()
                    },
                )
                .expect("promote");
        }
        matter.seed_default_codes().expect("seed");
    }

    #[test]
    fn text_pane_exact_not_truncated() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "BodyText").expect("create");
        seed_item_with_cas(
            &root,
            Some(b"Hello review body"),
            None,
            "Subject must not appear",
        );
        let body = review_document_body_blocking(ReviewDocumentBodyArgs {
            root: root.to_string(),
            item_id: "itm_0000".into(),
            pane: "text".into(),
        })
        .expect("body");
        assert_eq!(body.text, "Hello review body");
        assert!(!body.truncated);
        assert!(!body.empty);
        assert!(body.digest.is_some());
    }

    #[test]
    fn native_html_whitespace_not_helloworld() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "BodyHtml").expect("create");
        seed_item_with_cas(
            &root,
            None,
            Some(b"<p>Hello</p><p>World</p>"),
            "Subject as body would fail",
        );
        let body = review_document_body_blocking(ReviewDocumentBodyArgs {
            root: root.to_string(),
            item_id: "itm_0000".into(),
            pane: "native".into(),
        })
        .expect("body");
        assert!(body.text.contains("Hello"), "{:?}", body.text);
        assert!(body.text.contains("World"), "{:?}", body.text);
        assert!(
            !body.text.contains("HelloWorld"),
            "must not concatenate: {:?}",
            body.text
        );
        let hello_pos = body.text.find("Hello").expect("Hello");
        let world_pos = body.text.find("World").expect("World");
        let between = &body.text[hello_pos + 5..world_pos];
        assert!(
            between.chars().any(|c| c.is_whitespace()),
            "expected whitespace between Hello and World, got {between:?}"
        );
        assert!(!body.text.contains("<p"), "{:?}", body.text);
        assert!(!body.truncated);
    }

    #[test]
    fn truncated_over_two_mib() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "BodyCap").expect("create");
        let blob = vec![b'x'; BODY_DISPLAY_CAP_BYTES + 1];
        seed_item_with_cas(&root, Some(&blob), None, "cap");
        let body = review_document_body_blocking(ReviewDocumentBodyArgs {
            root: root.to_string(),
            item_id: "itm_0000".into(),
            pane: "text".into(),
        })
        .expect("body");
        assert!(body.truncated);
        assert!(body.text.len() <= BODY_DISPLAY_CAP_BYTES);
        assert!(body.text.chars().count() <= BODY_DISPLAY_CAP_BYTES);
        assert!(!body.empty);
    }

    #[test]
    fn missing_digest_empty_not_subject() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "BodyEmpty").expect("create");
        seed_item_with_cas(&root, None, None, "Do not use this subject as body");
        let text = review_document_body_blocking(ReviewDocumentBodyArgs {
            root: root.to_string(),
            item_id: "itm_0000".into(),
            pane: "text".into(),
        })
        .expect("text");
        assert!(text.empty);
        assert_eq!(text.text, "No extracted text");
        assert!(!text.text.contains("Do not use this subject"));
        let native = review_document_body_blocking(ReviewDocumentBodyArgs {
            root: root.to_string(),
            item_id: "itm_0000".into(),
            pane: "native".into(),
        })
        .expect("native");
        assert!(native.empty);
        assert_eq!(native.text, "No native/extracted body");
        assert!(!native.text.contains("Do not use this subject"));
    }
}
