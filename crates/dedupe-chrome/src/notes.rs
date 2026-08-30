//! `review_upsert_note` — actor chrome.

use matter_core::UpsertNoteInput;
use serde::Deserialize;

use crate::error::{map_core, CommandError};
use crate::open_root::open_matter_write;

#[derive(Debug, Clone, Deserialize)]
pub struct ReviewUpsertNoteArgs {
    pub root: String,
    pub item_id: String,
    pub body: String,
    #[serde(default)]
    pub id: Option<String>,
}

pub fn review_upsert_note_blocking(
    args: ReviewUpsertNoteArgs,
) -> Result<matter_core::ItemNote, CommandError> {
    if args.item_id.trim().is_empty() {
        return Err(CommandError::not_found("item not found: ".to_string()));
    }
    let matter = open_matter_write(&args.root)?;
    matter
        .upsert_note(UpsertNoteInput {
            id: args.id,
            item_id: args.item_id,
            body: args.body,
            highlight_id: None,
            actor: "chrome".into(),
            expected_version: None,
        })
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("empty") || msg.contains("whitespace") {
                CommandError::failed(msg)
            } else {
                map_core(e)
            }
        })
}
