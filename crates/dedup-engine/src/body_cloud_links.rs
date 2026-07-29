//! Offline body-surface detection of document-shaped SharePoint/OneDrive cloud links (0085).
//!
//! Pure scanner: no network fetch, no Attachment Table synthesis. Caps and host/path
//! allowlist follow Purview modern-attachment design inputs (not collection parity).

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;

/// Max Unicode scalar values considered per body surface (Purview-aligned default).
pub const MAX_BODY_SCAN_CHARS: usize = 100_000;
/// Max kept URL length (characters); longer candidates are skipped.
pub const MAX_URL_LEN: usize = 2_048;
/// Max document-shaped cloud links kept per message (after exact-string dedupe).
pub const MAX_LINKS_PER_MESSAGE: usize = 50;

/// Where a body cloud URL was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyCloudUrlSource {
    /// `href="…"` / `href='…'` in HTML.
    HtmlHref,
    /// Bare `https://…` token in HTML (outside an href capture).
    HtmlBare,
    /// Bare `https://…` in plain-text body.
    PlainBare,
    /// SafeLinks wrapper unwrapped to a document-shaped target.
    SafeLinksUnwrap,
}

impl BodyCloudUrlSource {
    /// Fixed CSV / ledger string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HtmlHref => "html_href",
            Self::HtmlBare => "html_bare",
            Self::PlainBare => "plain_bare",
            Self::SafeLinksUnwrap => "safelinks",
        }
    }
}

/// One kept document-shaped cloud link hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyCloudLinkHit {
    /// Full post-unescape URL including query (never stripped).
    pub url: String,
    pub source: BodyCloudUrlSource,
}

/// Result of scanning one message body.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BodyCloudScan {
    pub hits: Vec<BodyCloudLinkHit>,
    /// True when caps dropped additional document-shaped candidates (window / max-N).
    pub truncated: bool,
    pub scanned_html: bool,
    pub scanned_plain: bool,
}

/// Fixed-pattern regex (never body-derived). Returns `None` only if the constant
/// pattern is invalid at init — callers degrade to no hits (no process abort).
fn href_re() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)href\s*=\s*(?:"([^"]+)"|'([^']+)')"#).ok())
        .as_ref()
}

fn bare_url_re() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)https?://[^\s<>"'\)\]\}]+"#).ok())
        .as_ref()
}

/// Scan HTML and optional plain body for document-shaped commercial cloud links.
///
/// Order: HTML first (href then bare), then plain bare when plain is provided.
/// Full-body scan (no quote/blockquote skip). Exact-string dedupe preserves query.
pub fn scan_body_cloud_links(html: Option<&[u8]>, plain: Option<&str>) -> BodyCloudScan {
    let mut hits: Vec<BodyCloudLinkHit> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut truncated = false;
    let mut scanned_html = false;
    let mut scanned_plain = false;

    if let Some(raw) = html {
        scanned_html = true;
        let (text, windowed) = body_window_from_bytes(raw);
        if windowed {
            truncated = true;
        }
        collect_from_html(&text, &mut hits, &mut seen, &mut truncated);
    }

    if let Some(p) = plain {
        // Plain pass when present so plain-only URLs are found even with HTML.
        scanned_plain = true;
        let (text, windowed) = body_window_str(p);
        if windowed {
            truncated = true;
        }
        collect_bare(
            &text,
            BodyCloudUrlSource::PlainBare,
            &mut hits,
            &mut seen,
            &mut truncated,
        );
    }

    BodyCloudScan {
        hits,
        truncated,
        scanned_html,
        scanned_plain,
    }
}

fn body_window_from_bytes(raw: &[u8]) -> (String, bool) {
    // Prefer UTF-8; lossy keeps scan offline and linear.
    let s = String::from_utf8_lossy(raw);
    body_window_str(&s)
}

fn body_window_str(s: &str) -> (String, bool) {
    let count = s.chars().count();
    if count <= MAX_BODY_SCAN_CHARS {
        (s.to_string(), false)
    } else {
        (s.chars().take(MAX_BODY_SCAN_CHARS).collect(), true)
    }
}

fn collect_from_html(
    text: &str,
    hits: &mut Vec<BodyCloudLinkHit>,
    seen: &mut HashSet<String>,
    truncated: &mut bool,
) {
    // Track href spans so bare-pass can skip overlapping text (avoid double-count same href).
    let mut href_ranges: Vec<(usize, usize)> = Vec::new();

    if let Some(re) = href_re() {
        for cap in re.captures_iter(text) {
            let m = cap.get(1).or_else(|| cap.get(2));
            let Some(m) = m else { continue };
            href_ranges.push((m.start(), m.end()));
            // Href values are exact attribute text — do not strip trailing punctuation
            // (query fidelity). Only HTML-unescape + trim.
            try_keep_candidate(
                m.as_str(),
                BodyCloudUrlSource::HtmlHref,
                hits,
                seen,
                truncated,
                false,
            );
            if hits.len() >= MAX_LINKS_PER_MESSAGE {
                if more_document_candidates_beyond(text, seen) {
                    *truncated = true;
                }
                return;
            }
        }
    }

    if let Some(re) = bare_url_re() {
        for m in re.find_iter(text) {
            let start = m.start();
            let end = m.end();
            if href_ranges.iter().any(|&(hs, he)| start >= hs && end <= he) {
                continue;
            }
            // Bare tokens may include trailing sentence punctuation outside the URL.
            try_keep_candidate(
                m.as_str(),
                BodyCloudUrlSource::HtmlBare,
                hits,
                seen,
                truncated,
                true,
            );
            if hits.len() >= MAX_LINKS_PER_MESSAGE {
                if more_document_candidates_beyond(text, seen) {
                    *truncated = true;
                }
                return;
            }
        }
    }
}

fn collect_bare(
    text: &str,
    source: BodyCloudUrlSource,
    hits: &mut Vec<BodyCloudLinkHit>,
    seen: &mut HashSet<String>,
    truncated: &mut bool,
) {
    let Some(re) = bare_url_re() else {
        return;
    };
    for m in re.find_iter(text) {
        try_keep_candidate(m.as_str(), source, hits, seen, truncated, true);
        if hits.len() >= MAX_LINKS_PER_MESSAGE {
            let rest = &text[m.end()..];
            for m2 in re.find_iter(rest) {
                let cand = normalize_candidate(m2.as_str(), true);
                if cand.chars().count() > MAX_URL_LEN || cand.is_empty() {
                    continue;
                }
                if seen.contains(&cand) {
                    continue;
                }
                if classify_url(&cand).is_some() {
                    *truncated = true;
                    break;
                }
            }
            return;
        }
    }
}

/// Cheap remaining-candidate probe after cap (HTML path).
fn more_document_candidates_beyond(text: &str, seen: &HashSet<String>) -> bool {
    if let Some(re) = bare_url_re() {
        for m in re.find_iter(text) {
            let cand = normalize_candidate(m.as_str(), true);
            if cand.chars().count() > MAX_URL_LEN || cand.is_empty() {
                continue;
            }
            if seen.contains(&cand) {
                continue;
            }
            if classify_url(&cand).is_some() {
                return true;
            }
        }
    }
    if let Some(re) = href_re() {
        for cap in re.captures_iter(text) {
            let m = cap.get(1).or_else(|| cap.get(2));
            let Some(m) = m else { continue };
            let cand = normalize_candidate(m.as_str(), false);
            if cand.chars().count() > MAX_URL_LEN || cand.is_empty() {
                continue;
            }
            if seen.contains(&cand) {
                continue;
            }
            if classify_url(&cand).is_some() {
                return true;
            }
        }
    }
    false
}

fn try_keep_candidate(
    raw: &str,
    source: BodyCloudUrlSource,
    hits: &mut Vec<BodyCloudLinkHit>,
    seen: &mut HashSet<String>,
    truncated: &mut bool,
    strip_trailing_punct: bool,
) {
    if hits.len() >= MAX_LINKS_PER_MESSAGE {
        let cand = normalize_candidate(raw, strip_trailing_punct);
        if !cand.is_empty()
            && cand.chars().count() <= MAX_URL_LEN
            && !seen.contains(&cand)
            && classify_url(&cand).is_some()
        {
            *truncated = true;
        }
        return;
    }

    let cand = normalize_candidate(raw, strip_trailing_punct);
    if cand.is_empty() || cand.chars().count() > MAX_URL_LEN {
        return;
    }
    if seen.contains(&cand) {
        return;
    }

    if let Some((final_url, final_source)) = classify_url(&cand) {
        // Prefer unwrap source when SafeLinks produced the kept URL.
        let src = if final_source == BodyCloudUrlSource::SafeLinksUnwrap {
            BodyCloudUrlSource::SafeLinksUnwrap
        } else {
            source
        };
        if seen.insert(final_url.clone()) {
            if hits.len() >= MAX_LINKS_PER_MESSAGE {
                *truncated = true;
                return;
            }
            hits.push(BodyCloudLinkHit {
                url: final_url,
                source: src,
            });
        }
    }
}

/// HTML-unescape + trim. Optionally strip trailing **sentence** punctuation that is
/// outside the URL (bare-text tokens only). **Never** strips query content:
/// `?`, `:`, `=`, `&`, `%` are never removed as trailing chars.
fn normalize_candidate(raw: &str, strip_trailing_punct: bool) -> String {
    let mut s = html_unescape(raw.trim());
    if strip_trailing_punct {
        // Sentence punctuation only — not query/path delimiters (`?`, `:`, `/`).
        while let Some(c) = s.chars().last() {
            if matches!(c, '.' | ',' | ';' | '!' | ')' | ']' | '}' | '"' | '\'') {
                s.pop();
            } else {
                break;
            }
        }
    }
    s
}

fn html_unescape(s: &str) -> String {
    // Minimal entity set sufficient for href/query targets.
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        if let Some(semi) = rest.find(';') {
            let entity = &rest[..=semi];
            let decoded = match entity {
                "&amp;" => Some('&'),
                "&lt;" => Some('<'),
                "&gt;" => Some('>'),
                "&quot;" => Some('"'),
                "&apos;" | "&#39;" => Some('\''),
                "&#38;" => Some('&'),
                _ => None,
            };
            if let Some(ch) = decoded {
                out.push(ch);
                rest = &rest[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = &rest[1..];
    }
    out.push_str(rest);
    out
}

/// Classify a normalized absolute URL. Returns (ledger_url, source_hint).
fn classify_url(url: &str) -> Option<(String, BodyCloudUrlSource)> {
    let lower = url.to_ascii_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return None;
    }

    let host = extract_host(&lower)?;
    if is_safelinks_host(host) {
        let target = unwrap_safelinks_url(url)?;
        // SafeLinks nested target is a full URL value — never strip punctuation.
        let target_norm = normalize_candidate(&target, false);
        if target_norm.is_empty() || target_norm.chars().count() > MAX_URL_LEN {
            return None;
        }
        // Re-test nested target against document-shaped allowlist (not SafeLinks again).
        let target_lower = target_norm.to_ascii_lowercase();
        let thost = extract_host(&target_lower)?;
        if is_safelinks_host(thost) {
            return None;
        }
        if is_document_shaped_cloud(&target_lower, thost) {
            return Some((target_norm, BodyCloudUrlSource::SafeLinksUnwrap));
        }
        return None;
    }

    if is_document_shaped_cloud(&lower, host) {
        Some((url.to_string(), BodyCloudUrlSource::HtmlHref))
    } else {
        None
    }
}

fn extract_host(url_lower: &str) -> Option<&str> {
    let after_scheme = url_lower
        .strip_prefix("https://")
        .or_else(|| url_lower.strip_prefix("http://"))?;
    let host_port = after_scheme.split(['/', '?', '#']).next().unwrap_or("");
    if host_port.is_empty() {
        return None;
    }
    // Drop userinfo if present.
    let host_port = host_port.rsplit('@').next().unwrap_or(host_port);
    let host = host_port.split(':').next().unwrap_or(host_port);
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

fn is_safelinks_host(host: &str) -> bool {
    host == "safelinks.protection.outlook.com"
        || host.ends_with(".safelinks.protection.outlook.com")
}

fn is_commercial_cloud_host(host: &str) -> bool {
    host == "sharepoint.com"
        || host.ends_with(".sharepoint.com")
        || host == "sharepoint-df.com"
        || host.ends_with(".sharepoint-df.com")
        || host == "onedrive.live.com"
        || host.ends_with(".onedrive.live.com")
        || host == "1drv.ms"
        || host.ends_with(".1drv.ms")
}

/// Document-shaped path/query markers (commercial allowlist).
fn is_document_shaped_cloud(url_lower: &str, host: &str) -> bool {
    if !is_commercial_cloud_host(host) {
        return false;
    }

    // OneDrive short links are document-shaped by nature of the shortener.
    if host == "1drv.ms" || host.ends_with(".1drv.ms") {
        return true;
    }

    let path_and_query = path_and_query(url_lower);
    // Action tokens and extensions apply to the **path only** — never search the query
    // string (avoids `?foo=:x:` / wiki false positives).
    let path_only = path_and_query.split('?').next().unwrap_or(path_and_query);

    // Folder share token — exclude by default (Purview-aligned).
    if path_only.contains(":f:") {
        return false;
    }

    // Action tokens (Word / Excel / PowerPoint / PDF-binary / catch-all file).
    const ACTION_TOKENS: &[&str] = &[":w:", ":x:", ":p:", ":b:", ":u:"];
    if ACTION_TOKENS.iter().any(|t| path_only.contains(t)) {
        return true;
    }

    // Extension markers on the path (before query).
    const EXTS: &[&str] = &[
        ".docx", ".doc", ".xlsx", ".xls", ".xlsm", ".pptx", ".ppt", ".pdf", ".csv",
    ];
    for ext in EXTS {
        if path_only.ends_with(ext) {
            return true;
        }
        // Also allow /file.xlsx/ forms.
        if path_only.contains(&format!("{ext}/")) {
            return true;
        }
    }

    // Document library item / download-style paths with query implying a document.
    if path_only.contains("/_layouts/15/doc.aspx")
        || path_only.contains("/_layouts/15/download.aspx")
        || path_only.ends_with("/download.aspx")
        || path_only.contains("/download.aspx")
        || path_only.contains("/doc.aspx")
    {
        return query_implies_document(path_and_query);
    }

    false
}

fn path_and_query(url_lower: &str) -> &str {
    if let Some(rest) = url_lower.strip_prefix("https://") {
        if let Some(i) = rest.find('/') {
            return &rest[i..];
        }
        return "";
    }
    if let Some(rest) = url_lower.strip_prefix("http://") {
        if let Some(i) = rest.find('/') {
            return &rest[i..];
        }
        return "";
    }
    url_lower
}

fn query_implies_document(path_and_query: &str) -> bool {
    let q = path_and_query.split('?').nth(1).unwrap_or("");
    if q.is_empty() {
        return false;
    }
    // Exact query-key match (not substring): `userid=` must not satisfy `id=`.
    const KEYS: &[&str] = &["sourcedoc", "uniqueid", "file", "id", "documentid"];
    for pair in q.split('&') {
        let key_raw = pair.split('=').next().unwrap_or("");
        if key_raw.is_empty() {
            continue;
        }
        // Percent-decode common `%3d` already split; keys are usually plain.
        let key = percent_decode(key_raw).to_ascii_lowercase();
        if KEYS.contains(&key.as_str()) {
            return true;
        }
    }
    false
}

/// Extract and decode SafeLinks `url=` query parameter.
fn unwrap_safelinks_url(wrapper: &str) -> Option<String> {
    let q_start = wrapper.find('?')?;
    let query = &wrapper[q_start + 1..];
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next().unwrap_or("");
        let val = parts.next().unwrap_or("");
        if key.eq_ignore_ascii_case("url") && !val.is_empty() {
            let decoded = percent_decode(val);
            let unescaped = html_unescape(&decoded);
            if unescaped.starts_with("http://") || unescaped.starts_with("https://") {
                return Some(unescaped);
            }
            return None;
        }
    }
    None
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html_with(url: &str) -> Vec<u8> {
        format!(r#"<html><body><a href="{url}">doc</a></body></html>"#).into_bytes()
    }

    #[test]
    fn hit_action_tokens_including_excel() {
        let cases = [
            ("https://contoso.sharepoint.com/:w:/s/Legal/worddoc", ":w:"),
            (
                "https://contoso.sharepoint.com/:x:/r/sites/Finance/Shared%20Documents/book.xlsx",
                ":x:",
            ),
            ("https://contoso.sharepoint.com/:p:/g/slides", ":p:"),
            (
                "https://contoso.sharepoint.com/:b:/s/Legal/report.pdf",
                ":b:",
            ),
            ("https://contoso.sharepoint.com/:u:/g/file.zip", ":u:"),
        ];
        for (url, token) in cases {
            let scan = scan_body_cloud_links(Some(&html_with(url)), None);
            assert_eq!(scan.hits.len(), 1, "expected hit for {token}: {url}");
            assert_eq!(scan.hits[0].url, url);
            assert_eq!(scan.hits[0].source, BodyCloudUrlSource::HtmlHref);
        }
    }

    #[test]
    fn hit_xlsx_extension_path() {
        let url = "https://contoso.sharepoint.com/sites/Legal/Shared%20Documents/Q4.xlsx";
        let scan = scan_body_cloud_links(Some(&html_with(url)), None);
        assert_eq!(scan.hits.len(), 1);
        assert_eq!(scan.hits[0].url, url);
    }

    #[test]
    fn hit_1drv_ms() {
        let url = "https://1drv.ms/x/s!AmxYzExample";
        let scan = scan_body_cloud_links(Some(&html_with(url)), None);
        assert_eq!(scan.hits.len(), 1);
        assert_eq!(scan.hits[0].url, url);
    }

    #[test]
    fn safelinks_unwrap_preserves_query() {
        let target = "https://contoso.sharepoint.com/:x:/s/Finance/book.xlsx?d=wabc123&csf=1";
        let encoded = percent_encode_for_test(target);
        let wrapper =
            format!("https://nam06.safelinks.protection.outlook.com/?url={encoded}&data=foo");
        let scan = scan_body_cloud_links(Some(&html_with(&wrapper)), None);
        assert_eq!(scan.hits.len(), 1, "hits={:?}", scan.hits);
        assert_eq!(scan.hits[0].url, target);
        assert_eq!(scan.hits[0].source, BodyCloudUrlSource::SafeLinksUnwrap);
        assert!(scan.hits[0].url.contains("?d=wabc123"));
        assert!(scan.hits[0].url.contains("&csf=1"));
    }

    fn percent_encode_for_test(s: &str) -> String {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    (b as char).to_string()
                }
                _ => format!("%{b:02X}"),
            })
            .collect()
    }

    #[test]
    fn miss_bare_hr_site() {
        let url = "https://contoso.sharepoint.com/sites/HR";
        let scan = scan_body_cloud_links(Some(&html_with(url)), None);
        assert!(
            scan.hits.is_empty(),
            "intranet root must miss: {:?}",
            scan.hits
        );
    }

    #[test]
    fn miss_non_cloud_https() {
        let url = "https://example.com/docs/report.xlsx";
        let scan = scan_body_cloud_links(Some(&html_with(url)), None);
        assert!(scan.hits.is_empty());
    }

    #[test]
    fn miss_folder_f_token() {
        let url = "https://contoso.sharepoint.com/:f:/s/Legal/folderShare";
        let scan = scan_body_cloud_links(Some(&html_with(url)), None);
        assert!(
            scan.hits.is_empty(),
            ":f: folder shares must be excluded: {:?}",
            scan.hits
        );
    }

    #[test]
    fn two_queries_two_rows() {
        let u1 = "https://contoso.sharepoint.com/:x:/s/Legal/a.xlsx?d=1";
        let u2 = "https://contoso.sharepoint.com/:x:/s/Legal/a.xlsx?d=2";
        let html = format!(r#"<html><a href="{u1}">one</a><a href="{u2}">two</a></html>"#);
        let scan = scan_body_cloud_links(Some(html.as_bytes()), None);
        assert_eq!(scan.hits.len(), 2);
        assert_eq!(scan.hits[0].url, u1);
        assert_eq!(scan.hits[1].url, u2);
    }

    #[test]
    fn exact_dedupe_same_url() {
        let u = "https://contoso.sharepoint.com/:w:/s/Legal/memo.docx";
        let html = format!(r#"<a href="{u}">a</a><a href="{u}">b</a>"#);
        let scan = scan_body_cloud_links(Some(html.as_bytes()), None);
        assert_eq!(scan.hits.len(), 1);
    }

    #[test]
    fn empty_body() {
        let scan = scan_body_cloud_links(None, None);
        assert!(scan.hits.is_empty());
        assert!(!scan.scanned_html);
        assert!(!scan.scanned_plain);
        assert!(!scan.truncated);

        let scan2 = scan_body_cloud_links(Some(b""), Some(""));
        assert!(scan2.hits.is_empty());
        assert!(scan2.scanned_html);
        assert!(scan2.scanned_plain);
    }

    #[test]
    fn plain_bare_hit() {
        let plain = "See https://contoso.sharepoint.com/:x:/s/Fin/book.xlsx?d=zz for numbers.";
        let scan = scan_body_cloud_links(None, Some(plain));
        assert_eq!(scan.hits.len(), 1);
        assert_eq!(scan.hits[0].source, BodyCloudUrlSource::PlainBare);
        assert!(scan.hits[0].url.contains("?d=zz"));
    }

    #[test]
    fn query_preserve_on_href() {
        let url =
            "https://contoso.sharepoint.com/sites/L/Shared%20Documents/r.xlsx?d=wXYZ&csf=1&e=abc";
        let scan = scan_body_cloud_links(Some(&html_with(url)), None);
        assert_eq!(scan.hits.len(), 1);
        assert_eq!(scan.hits[0].url, url);
    }

    #[test]
    fn max_links_cap_and_truncated() {
        let mut html = String::from("<html><body>");
        for i in 0..60 {
            html.push_str(&format!(
                r#"<a href="https://contoso.sharepoint.com/:x:/s/L/f{i}.xlsx?d={i}">x</a>"#
            ));
        }
        html.push_str("</body></html>");
        let scan = scan_body_cloud_links(Some(html.as_bytes()), None);
        assert_eq!(scan.hits.len(), MAX_LINKS_PER_MESSAGE);
        assert!(scan.truncated, "must flag truncation past 50");
    }

    #[test]
    fn url_longer_than_2048_skipped() {
        let long_q = "a".repeat(3000);
        let url = format!("https://contoso.sharepoint.com/:x:/s/L/f.xlsx?d={long_q}");
        assert!(url.chars().count() > MAX_URL_LEN);
        let scan = scan_body_cloud_links(Some(&html_with(&url)), None);
        assert!(scan.hits.is_empty());
    }

    #[test]
    fn source_as_str_locked() {
        assert_eq!(BodyCloudUrlSource::HtmlHref.as_str(), "html_href");
        assert_eq!(BodyCloudUrlSource::HtmlBare.as_str(), "html_bare");
        assert_eq!(BodyCloudUrlSource::PlainBare.as_str(), "plain_bare");
        assert_eq!(BodyCloudUrlSource::SafeLinksUnwrap.as_str(), "safelinks");
    }

    #[test]
    fn onedrive_live_with_extension() {
        let url = "https://onedrive.live.com/view.aspx?resid=ABC&id=doc.pdf";
        // onedrive.live.com needs document-shaped marker — extension in path or action.
        // This path has .pdf in query not path — should miss unless we also check query.
        // Spec: path ends with extensions. Query-only id=doc.pdf is weak; require path marker.
        // Use path with extension:
        let url2 = "https://onedrive.live.com/personal/user/Documents/report.pdf";
        let scan = scan_body_cloud_links(Some(&html_with(url2)), None);
        assert_eq!(
            scan.hits.len(),
            1,
            "onedrive path+ext; first url miss ok: {url}"
        );
    }

    #[test]
    fn sharepoint_df_host() {
        let url = "https://contoso.sharepoint-df.com/:w:/s/Legal/memo.docx";
        let scan = scan_body_cloud_links(Some(&html_with(url)), None);
        assert_eq!(scan.hits.len(), 1);
    }

    #[test]
    fn body_window_100k_truncates_and_misses_past_window_url() {
        // Prefix fills the scan window; document-shaped URL sits past the window.
        let mut body = "x".repeat(MAX_BODY_SCAN_CHARS);
        body.push_str(
            r#" <a href="https://contoso.sharepoint.com/:x:/s/Legal/late.xlsx?d=past">late</a>"#,
        );
        let scan = scan_body_cloud_links(Some(body.as_bytes()), None);
        assert!(
            scan.truncated,
            "window overflow must set truncated even when no hit kept"
        );
        assert!(
            scan.hits.is_empty(),
            "URL past 100k window must not be collected"
        );
    }

    #[test]
    fn body_window_url_inside_window_still_hits() {
        let url = "https://contoso.sharepoint.com/:x:/s/Legal/early.xlsx?d=in";
        let mut body = format!(r#"<a href="{url}">early</a>"#);
        body.push_str(&"y".repeat(MAX_BODY_SCAN_CHARS)); // padding after URL still truncates flag
        let scan = scan_body_cloud_links(Some(body.as_bytes()), None);
        assert_eq!(scan.hits.len(), 1);
        assert_eq!(scan.hits[0].url, url);
        assert!(scan.truncated, "body longer than window sets truncated");
    }

    #[test]
    fn doc_aspx_with_document_query_hits() {
        let url = "https://contoso.sharepoint.com/sites/Legal/_layouts/15/Doc.aspx?sourcedoc=%7Babc%7D&file=memo.docx";
        let scan = scan_body_cloud_links(Some(&html_with(url)), None);
        assert_eq!(scan.hits.len(), 1, "Doc.aspx + sourcedoc must hit");
        assert_eq!(scan.hits[0].url, url);
    }

    #[test]
    fn download_aspx_without_document_query_misses() {
        let url = "https://contoso.sharepoint.com/sites/Legal/_layouts/15/download.aspx";
        let scan = scan_body_cloud_links(Some(&html_with(url)), None);
        assert!(
            scan.hits.is_empty(),
            "download.aspx without document-ish query must miss"
        );
    }

    #[test]
    fn download_aspx_with_id_query_hits() {
        let url =
            "https://contoso.sharepoint.com/sites/Legal/_layouts/15/download.aspx?UniqueId=deadbeef";
        let scan = scan_body_cloud_links(Some(&html_with(url)), None);
        assert_eq!(scan.hits.len(), 1);
        assert!(scan.hits[0].url.to_ascii_lowercase().contains("uniqueid="));
    }

    #[test]
    fn action_token_in_query_only_is_miss() {
        // Proportionality: document action tokens must be in path, not query text.
        let url = "https://contoso.sharepoint.com/sites/HR?foo=:x:";
        let scan = scan_body_cloud_links(Some(&html_with(url)), None);
        assert!(
            scan.hits.is_empty(),
            "query-only :x: must not hit: {:?}",
            scan.hits
        );
    }

    #[test]
    fn userid_query_does_not_imply_document() {
        let url = "https://contoso.sharepoint.com/sites/Legal/_layouts/15/download.aspx?userid=42";
        let scan = scan_body_cloud_links(Some(&html_with(url)), None);
        assert!(
            scan.hits.is_empty(),
            "userid= must not match id= key: {:?}",
            scan.hits
        );
    }

    #[test]
    fn href_preserves_trailing_query_punctuation() {
        // As-sent share URLs may end with `?` or include odd query endings — never strip.
        let url = "https://contoso.sharepoint.com/:x:/s/Legal/a.xlsx?d=abc?";
        let scan = scan_body_cloud_links(Some(&html_with(url)), None);
        assert_eq!(scan.hits.len(), 1);
        assert_eq!(
            scan.hits[0].url, url,
            "href must preserve full query including trailing ?"
        );
    }

    #[test]
    fn bare_strips_sentence_period_not_query() {
        let plain = "See https://contoso.sharepoint.com/:x:/s/Legal/a.xlsx?d=1.";
        let scan = scan_body_cloud_links(None, Some(plain));
        assert_eq!(scan.hits.len(), 1);
        assert_eq!(
            scan.hits[0].url,
            "https://contoso.sharepoint.com/:x:/s/Legal/a.xlsx?d=1"
        );
    }
}
