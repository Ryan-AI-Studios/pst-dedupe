//! `production-profile list|show|upsert|delete` commands (track **0060**).

use matter_core::ProductionProfileInput;
use serde_json::{json, Value};

use crate::error::{CliError, Result};
use crate::json_io::{emit_json, ok_envelope};
use crate::matter_cmd::{open_matter, open_matter_read, resolve_matter_root};
use crate::paths::resolve_cli_path;

pub fn production_profile_list(path: &std::path::Path, json: bool) -> Result<()> {
    let root = resolve_matter_root(path)?;
    let matter = open_matter_read(&root)?;
    let list = matter.list_production_profiles().map_err(CliError::from)?;
    if json {
        let rows: Vec<_> = list
            .iter()
            .map(|p| {
                json!({
                    "id": p.id,
                    "slug": p.slug,
                    "label": p.label,
                    "jurisdiction_tag": p.jurisdiction_tag,
                    "is_builtin": p.is_builtin,
                    "qc_pack_id": p.body.qc.pack_id,
                })
            })
            .collect();
        emit_json(
            true,
            &ok_envelope(json!({ "profiles": rows, "count": rows.len() })),
        )?;
    } else {
        println!("Production profiles (templates — not legal compliance advice):\n");
        for p in &list {
            let tag = if p.is_builtin { "builtin" } else { "user" };
            let jur = p.jurisdiction_tag.as_deref().unwrap_or("-");
            println!(
                "[{tag}] {}  {}  ({})  qc={}",
                p.slug, p.label, jur, p.body.qc.pack_id
            );
        }
    }
    Ok(())
}

pub fn production_profile_show(path: &std::path::Path, slug: &str, json: bool) -> Result<()> {
    if slug.trim().is_empty() {
        return Err(CliError::Usage(
            "production profile slug must not be empty".into(),
        ));
    }
    let root = resolve_matter_root(path)?;
    let matter = open_matter_read(&root)?;
    let p = matter
        .get_production_profile(slug)
        .map_err(CliError::from)?;
    let body = serde_json::to_value(&p.body).map_err(CliError::from)?;
    if json {
        emit_json(
            true,
            &ok_envelope(json!({
                "id": p.id,
                "slug": p.slug,
                "label": p.label,
                "jurisdiction_tag": p.jurisdiction_tag,
                "is_builtin": p.is_builtin,
                "body": body,
            })),
        )?;
    } else {
        println!("id:              {}", p.id);
        println!("slug:            {}", p.slug);
        println!("label:           {}", p.label);
        println!(
            "jurisdiction:    {}",
            p.jurisdiction_tag.as_deref().unwrap_or("-")
        );
        println!("builtin:         {}", p.is_builtin);
        println!("qc.pack_id:      {}", p.body.qc.pack_id);
        println!("bates.prefix:    {}", p.body.bates.prefix);
        println!("bates.pad_width: {}", p.body.bates.pad_width);
        println!("load dialect:    {}", p.body.load_file.dialect);
        println!("note: Bates start is job-time only (never stored in profile).");
        println!("note: Built-in templates are technical packaging presets, not legal advice.");
        println!("body:");
        println!("{}", serde_json::to_string_pretty(&body)?);
    }
    Ok(())
}

pub fn production_profile_upsert(
    path: &std::path::Path,
    file: &std::path::Path,
    json: bool,
) -> Result<()> {
    let root = resolve_matter_root(path)?;
    let file_path = resolve_cli_path(file)?;
    let text = std::fs::read_to_string(file_path.as_std_path())
        .map_err(|e| CliError::Usage(format!("cannot read profile file {file_path}: {e}")))?;
    let doc = parse_production_profile_document(&text)?;
    let matter = open_matter(&root)?;
    let profile = matter
        .upsert_production_profile(ProductionProfileInput {
            id: doc.id,
            slug: doc.slug,
            label: doc.label,
            jurisdiction_tag: doc.jurisdiction_tag,
            body_json: doc.body_json,
        })
        .map_err(CliError::from)?;
    if json {
        emit_json(
            true,
            &ok_envelope(json!({
                "id": profile.id,
                "slug": profile.slug,
                "label": profile.label,
                "is_builtin": profile.is_builtin,
            })),
        )?;
    } else {
        println!(
            "upserted production profile id={} slug={}",
            profile.id, profile.slug
        );
    }
    Ok(())
}

pub fn production_profile_delete(path: &std::path::Path, slug: &str, json: bool) -> Result<()> {
    if slug.trim().is_empty() {
        return Err(CliError::Usage(
            "production profile slug/id must not be empty".into(),
        ));
    }
    let root = resolve_matter_root(path)?;
    let matter = open_matter(&root)?;
    matter
        .delete_production_profile(slug)
        .map_err(CliError::from)?;
    if json {
        emit_json(true, &ok_envelope(json!({ "deleted": slug })))?;
    } else {
        println!("deleted production profile {slug}");
    }
    Ok(())
}

/// Fields extracted from a production-profile upsert document.
struct UpsertDoc {
    slug: String,
    label: String,
    jurisdiction_tag: Option<String>,
    body_json: String,
    id: Option<String>,
}

/// Parse upsert document:
/// `{ "slug": "…", "label": "…", "jurisdiction_tag": "…", "body": {…}, "id": "…" }`
/// or body at top-level with slug/label siblings.
fn parse_production_profile_document(text: &str) -> Result<UpsertDoc> {
    let root: Value = serde_json::from_str(text)
        .map_err(|e| CliError::Usage(format!("invalid production profile JSON: {e}")))?;
    let obj = root
        .as_object()
        .ok_or_else(|| CliError::Usage("production profile JSON must be an object".into()))?;

    let slug = obj
        .get("slug")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            CliError::Usage("production profile upsert requires top-level \"slug\"".into())
        })?;
    let label = obj
        .get("label")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            CliError::Usage("production profile upsert requires top-level \"label\"".into())
        })?;
    let jurisdiction_tag = obj
        .get("jurisdiction_tag")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let id = obj
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let body_json = if let Some(body) = obj.get("body") {
        serde_json::to_string(body)?
    } else if obj.contains_key("version") && obj.contains_key("load_file") {
        // Bare body at top level (without slug/label nested inside body).
        let mut bare = obj.clone();
        bare.remove("slug");
        bare.remove("label");
        bare.remove("jurisdiction_tag");
        bare.remove("id");
        serde_json::to_string(&Value::Object(bare))?
    } else {
        return Err(CliError::Usage(
            "production profile upsert requires \"body\" object or top-level versioned body".into(),
        ));
    };

    Ok(UpsertDoc {
        slug,
        label,
        jurisdiction_tag,
        body_json,
        id,
    })
}
