//! Tantivy schema for per-matter keyword FTS (track 0029 + 0054 packs).

use tantivy::schema::{
    Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, FAST, STORED, STRING, TEXT,
};

use crate::pack::{LangPack, CJK_HYBRID_TOKENIZER_ID};

/// Named fields in the matter FTS schema.
#[derive(Debug, Clone)]
pub struct FtsSchema {
    pub schema: Schema,
    pub item_id: Field,
    pub subject: Field,
    pub body: Field,
    pub path: Field,
    pub attach_names: Field,
    /// Pack used when this schema was built.
    pub pack: LangPack,
}

impl FtsSchema {
    /// Build the P0 FTS schema for `latin_default` (Tantivy `default` TEXT).
    pub fn build() -> Self {
        Self::build_for_pack(LangPack::LatinDefault)
    }

    /// Build schema for the given language pack.
    ///
    /// - `latin_default`: default TEXT (freqs + positions; `default` tokenizer)
    /// - `cjk_ngram_v1`: TextFieldIndexing with tokenizer `cjk_hybrid_v1` +
    ///   WithFreqsAndPositions for subject/body/path/attach_names
    pub fn build_for_pack(pack: LangPack) -> Self {
        let mut builder = Schema::builder();
        // Untokenized id for exact delete_term + stored for hit recovery + FAST column.
        let item_id = builder.add_text_field("item_id", STRING | STORED | FAST);

        let text_opts = match pack {
            LangPack::LatinDefault => TEXT,
            LangPack::CjkNgramV1 => {
                let indexing = TextFieldIndexing::default()
                    .set_tokenizer(CJK_HYBRID_TOKENIZER_ID)
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions);
                TextOptions::default().set_indexing_options(indexing)
            }
        };

        let subject = builder.add_text_field("subject", text_opts.clone());
        let body = builder.add_text_field("body", text_opts.clone());
        let path = builder.add_text_field("path", text_opts.clone());
        let attach_names = builder.add_text_field("attach_names", text_opts);
        let schema = builder.build();
        Self {
            schema,
            item_id,
            subject,
            body,
            path,
            attach_names,
            pack,
        }
    }

    /// Default query fields for [`tantivy::query::QueryParser`].
    pub fn default_query_fields(&self) -> Vec<Field> {
        vec![self.subject, self.body, self.path, self.attach_names]
    }
}
