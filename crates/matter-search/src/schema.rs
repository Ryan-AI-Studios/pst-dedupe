//! Tantivy schema for per-matter keyword FTS (track 0029).

use tantivy::schema::{Field, Schema, STORED, STRING, TEXT};

/// Named fields in the matter FTS schema.
#[derive(Debug, Clone)]
pub struct FtsSchema {
    pub schema: Schema,
    pub item_id: Field,
    pub subject: Field,
    pub body: Field,
    pub path: Field,
    pub attach_names: Field,
}

impl FtsSchema {
    /// Build the P0 FTS schema.
    ///
    /// - `item_id`: **STRING | STORED** (untokenized) for exact `delete_term`
    /// - `subject`, `body`, `path`, `attach_names`: **TEXT** (tokenized + positions for phrases)
    /// - Full body is **not** STORED (re-read from CAS in the viewer)
    pub fn build() -> Self {
        let mut builder = Schema::builder();
        // Untokenized id for exact delete_term + stored for hit recovery.
        let item_id = builder.add_text_field("item_id", STRING | STORED);
        // Default TEXT includes freqs + positions (phrase queries).
        let subject = builder.add_text_field("subject", TEXT);
        let body = builder.add_text_field("body", TEXT);
        let path = builder.add_text_field("path", TEXT);
        let attach_names = builder.add_text_field("attach_names", TEXT);
        let schema = builder.build();
        Self {
            schema,
            item_id,
            subject,
            body,
            path,
            attach_names,
        }
    }

    /// Default query fields for [`tantivy::query::QueryParser`].
    pub fn default_query_fields(&self) -> Vec<Field> {
        vec![self.subject, self.body, self.path, self.attach_names]
    }
}
