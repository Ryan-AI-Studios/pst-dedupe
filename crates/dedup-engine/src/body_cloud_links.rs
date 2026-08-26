//! Offline body-surface detection of document-shaped SharePoint/OneDrive cloud links
//! (0085 commercial; 0088 US GCC High / DoD sovereign hosts).
//!
//! Pure scanner: no network fetch, no Attachment Table synthesis. Caps and host/path
//! allowlist follow Purview modern-attachment design inputs (not collection parity).

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;

/// Max Unicode scalar values considered per body surface (Purview-aligned default).
pub const MAX_BODY_SCAN_CHARS: usize = 100_000;
/// Max kept URL length (characters). Longer document-shaped candidates are not
/// kept hits; they set `truncated` and stash a 2048-char prefix for the honesty marker.
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
    /// True when document-shaped candidates were actually dropped
    /// (window tail / max-links / url-len). Independent of [`Self::window_capped`].
    pub truncated: bool,
    /// True when HTML or plain exceeded the 100k scan window.
    pub window_capped: bool,
    /// Document-shaped candidate(s) existed in the un-windowed tail.
    pub window_dropped: bool,
    /// Additional in-window document-shaped candidates past the 50-link cap.
    pub max_links_exceeded: bool,
    /// A document-shaped URL (or SafeLinks nested target) exceeded 2048 chars.
    pub url_truncated: bool,
    /// First 2048-char prefix of the first over-length document-shaped candidate.
    /// Marker payload only — not a kept hit.
    pub overlength_prefix: Option<String>,
    pub scanned_html: bool,
    pub scanned_plain: bool,
}

struct ScanAccum {
    hits: Vec<BodyCloudLinkHit>,
    seen: HashSet<String>,
    truncated: bool,
    window_capped: bool,
    window_dropped: bool,
    max_links_exceeded: bool,
    url_truncated: bool,
    overlength_prefix: Option<String>,
    scanned_html: bool,
    scanned_plain: bool,
}

impl ScanAccum {
    fn new() -> Self {
        Self {
            hits: Vec::new(),
            seen: HashSet::new(),
            truncated: false,
            window_capped: false,
            window_dropped: false,
            max_links_exceeded: false,
            url_truncated: false,
            overlength_prefix: None,
            scanned_html: false,
            scanned_plain: false,
        }
    }

    fn note_overlength(&mut self, url: &str) {
        self.url_truncated = true;
        self.truncated = true;
        if self.overlength_prefix.is_none() {
            self.overlength_prefix = Some(url.chars().take(MAX_URL_LEN).collect());
        }
    }

    fn note_max_links(&mut self) {
        self.max_links_exceeded = true;
        self.truncated = true;
    }

    fn note_window_drop(&mut self) {
        self.window_dropped = true;
        self.truncated = true;
    }

    fn note_unseen_in(&mut self, text: &str, as_window_tail: bool) {
        let probe = probe_unseen_document_candidates(text, &self.seen);
        if !probe.found {
            return;
        }
        if as_window_tail {
            self.note_window_drop();
        } else {
            self.note_max_links();
        }
        if let Some(url) = probe.overlength_url.as_deref() {
            self.note_overlength(url);
        }
    }

    fn into_scan(self) -> BodyCloudScan {
        let truncated =
            self.truncated || self.window_dropped || self.max_links_exceeded || self.url_truncated;
        BodyCloudScan {
            hits: self.hits,
            truncated,
            window_capped: self.window_capped,
            window_dropped: self.window_dropped,
            max_links_exceeded: self.max_links_exceeded,
            url_truncated: self.url_truncated,
            overlength_prefix: self.overlength_prefix,
            scanned_html: self.scanned_html,
            scanned_plain: self.scanned_plain,
        }
    }
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

/// Scan HTML and optional plain body for document-shaped cloud links
/// (commercial + US GCC High / DoD allowlist).
///
/// Order: HTML first (href then bare), then plain bare when plain is provided.
/// Full-body scan (no quote/blockquote skip). Exact-string dedupe preserves query.
pub fn scan_body_cloud_links(html: Option<&[u8]>, plain: Option<&str>) -> BodyCloudScan {
    let mut acc = ScanAccum::new();

    if let Some(raw) = html {
        acc.scanned_html = true;
        let s = String::from_utf8_lossy(raw);
        let (text, windowed) = body_window_str(&s);
        if windowed {
            acc.window_capped = true;
        }
        collect_from_html(&text, windowed.then_some(s.as_ref()), windowed, &mut acc);
        if windowed {
            let tail = char_tail(s.as_ref(), MAX_BODY_SCAN_CHARS);
            acc.note_unseen_in(tail, true);
        }
    }

    if let Some(p) = plain {
        // Plain pass when present so plain-only URLs are found even with HTML.
        acc.scanned_plain = true;
        let (text, windowed) = body_window_str(p);
        if windowed {
            acc.window_capped = true;
        }
        collect_bare(
            &text,
            BodyCloudUrlSource::PlainBare,
            windowed.then_some(p),
            windowed,
            &mut acc,
        );
        if windowed {
            let tail = char_tail(p, MAX_BODY_SCAN_CHARS);
            acc.note_unseen_in(tail, true);
        }
    }

    acc.into_scan()
}

fn body_window_str(s: &str) -> (String, bool) {
    let count = s.chars().count();
    if count <= MAX_BODY_SCAN_CHARS {
        (s.to_string(), false)
    } else {
        (s.chars().take(MAX_BODY_SCAN_CHARS).collect(), true)
    }
}

fn char_tail(s: &str, skip_chars: usize) -> &str {
    match s.char_indices().nth(skip_chars) {
        Some((i, _)) => &s[i..],
        None => "",
    }
}

/// Inverse of the bare-URL character class `[^\s<>"'\)\]\}]`.
fn is_url_continue_char(c: char) -> bool {
    if c.is_whitespace() {
        return false;
    }
    !matches!(c, '<' | '>' | '"' | '\'' | ')' | ']' | '}')
}

fn full_bare_url_from(original: &str, start: usize) -> Option<&str> {
    let rest = original.get(start..)?;
    let re = bare_url_re()?;
    let m = re.find(rest)?;
    if m.start() != 0 {
        return None;
    }
    Some(m.as_str())
}

/// Reject a bare match cut by the 100k window; if the full original URL is
/// document-shaped, record a window drop (not a kept hit).
fn handle_window_edge_bare(m_start: usize, original: &str, acc: &mut ScanAccum) -> bool {
    let Some(next) = original.chars().nth(MAX_BODY_SCAN_CHARS) else {
        return false;
    };
    if !is_url_continue_char(next) {
        return false;
    }
    if let Some(full) = full_bare_url_from(original, m_start) {
        if let Some((final_url, _, overlength)) = classify_url(full) {
            // Cut prefix is never a kept hit; only unique unseen URLs count as drops.
            if acc.seen.contains(&final_url) {
                return true;
            }
            acc.note_window_drop();
            if overlength {
                acc.note_overlength(&final_url);
            }
        }
    }
    true
}

fn collect_from_html(text: &str, original: Option<&str>, windowed: bool, acc: &mut ScanAccum) {
    // Track href spans so bare-pass can skip overlapping text (avoid double-count same href).
    let mut href_ranges: Vec<(usize, usize)> = Vec::new();

    if let Some(re) = href_re() {
        for cap in re.captures_iter(text) {
            let m = cap.get(1).or_else(|| cap.get(2));
            let Some(m) = m else { continue };
            href_ranges.push((m.start(), m.end()));
            // Href values are exact attribute text — do not strip trailing punctuation
            // (query fidelity). Only HTML-unescape + trim.
            try_keep_candidate(m.as_str(), BodyCloudUrlSource::HtmlHref, false, acc);
            if acc.hits.len() >= MAX_LINKS_PER_MESSAGE {
                acc.note_unseen_in(text, false);
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
            if windowed && end == text.len() {
                if let Some(orig) = original {
                    if handle_window_edge_bare(start, orig, acc) {
                        continue;
                    }
                }
            }
            try_keep_candidate(m.as_str(), BodyCloudUrlSource::HtmlBare, true, acc);
            if acc.hits.len() >= MAX_LINKS_PER_MESSAGE {
                acc.note_unseen_in(text, false);
                return;
            }
        }
    }
}

fn collect_bare(
    text: &str,
    source: BodyCloudUrlSource,
    original: Option<&str>,
    windowed: bool,
    acc: &mut ScanAccum,
) {
    let Some(re) = bare_url_re() else {
        return;
    };
    for m in re.find_iter(text) {
        if windowed && m.end() == text.len() {
            if let Some(orig) = original {
                if handle_window_edge_bare(m.start(), orig, acc) {
                    continue;
                }
            }
        }
        try_keep_candidate(m.as_str(), source, true, acc);
        if acc.hits.len() >= MAX_LINKS_PER_MESSAGE {
            acc.note_unseen_in(text, false);
            return;
        }
    }
}

struct UnseenProbe {
    found: bool,
    /// First unseen over-length classified URL (full); prefix is taken by `note_overlength`.
    overlength_url: Option<String>,
}

/// Remaining-candidate probe (window tail or past the 50-link cap).
/// Over-length document-shaped URLs count as candidates and surface prefix metadata.
fn probe_unseen_document_candidates(text: &str, seen: &HashSet<String>) -> UnseenProbe {
    let mut probe = UnseenProbe {
        found: false,
        overlength_url: None,
    };
    let mut consider = |cand: String| {
        if cand.is_empty() || seen.contains(&cand) {
            return;
        }
        let Some((final_url, _, overlength)) = classify_url(&cand) else {
            return;
        };
        if seen.contains(&final_url) {
            return;
        }
        probe.found = true;
        if overlength && probe.overlength_url.is_none() {
            probe.overlength_url = Some(final_url);
        }
    };
    if let Some(re) = bare_url_re() {
        for m in re.find_iter(text) {
            consider(normalize_candidate(m.as_str(), true));
        }
    }
    if let Some(re) = href_re() {
        for cap in re.captures_iter(text) {
            let m = cap.get(1).or_else(|| cap.get(2));
            let Some(m) = m else { continue };
            consider(normalize_candidate(m.as_str(), false));
        }
    }
    probe
}

fn try_keep_candidate(
    raw: &str,
    source: BodyCloudUrlSource,
    strip_trailing_punct: bool,
    acc: &mut ScanAccum,
) {
    let cand = normalize_candidate(raw, strip_trailing_punct);
    if cand.is_empty() {
        return;
    }
    let Some((final_url, final_source, overlength)) = classify_url(&cand) else {
        return;
    };
    // Prefer unwrap source when SafeLinks produced the kept URL.
    let src = if final_source == BodyCloudUrlSource::SafeLinksUnwrap {
        BodyCloudUrlSource::SafeLinksUnwrap
    } else {
        source
    };
    if overlength {
        acc.note_overlength(&final_url);
        return;
    }
    if acc.seen.contains(&final_url) {
        return;
    }
    if acc.hits.len() >= MAX_LINKS_PER_MESSAGE {
        acc.note_max_links();
        return;
    }
    if acc.seen.insert(final_url.clone()) {
        acc.hits.push(BodyCloudLinkHit {
            url: final_url,
            source: src,
        });
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

/// Classify a normalized absolute URL.
/// Returns (ledger_url, source_hint, overlength). Over-length document-shaped
/// URLs are still classified so callers can set `truncated` instead of dropping silently.
fn classify_url(url: &str) -> Option<(String, BodyCloudUrlSource, bool)> {
    let lower = url.to_ascii_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return None;
    }

    let host = extract_host(&lower)?;
    if is_safelinks_host(host) {
        let target = unwrap_safelinks_url(url)?;
        // SafeLinks nested target is a full URL value — never strip punctuation.
        let target_norm = normalize_candidate(&target, false);
        if target_norm.is_empty() {
            return None;
        }
        // Re-test nested target against document-shaped allowlist (not SafeLinks again).
        let target_lower = target_norm.to_ascii_lowercase();
        let thost = extract_host(&target_lower)?;
        if is_safelinks_host(thost) {
            return None;
        }
        if is_document_shaped_cloud(&target_lower, thost) {
            let overlength = target_norm.chars().count() > MAX_URL_LEN;
            return Some((target_norm, BodyCloudUrlSource::SafeLinksUnwrap, overlength));
        }
        return None;
    }

    if is_document_shaped_cloud(&lower, host) {
        let overlength = url.chars().count() > MAX_URL_LEN;
        Some((url.to_string(), BodyCloudUrlSource::HtmlHref, overlength))
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

/// Commercial + GCC High/DoD SafeLinks wrapper hosts.
fn is_safelinks_host(host: &str) -> bool {
    host_matches_dns_suffix(host, "safelinks.protection.outlook.com")
        || host_matches_dns_suffix(host, "safelinks.protection.office365.us")
}

/// DNS suffixes for document-shaped cloud hosts (exact or one-or-more labels under).
///
/// Learn citations (access 2026-08-24): GCC High `*.sharepoint.us` + `admin.onedrive.us`
/// (updated 2026-07-01); DoD `*.sharepoint-mil.us` + `*.dps.mil` (updated 2026-06-30).
/// 21Vianet (`*.sharepoint.cn`) and `.microsoft` TLD content hosts are out of P0.
///
/// Keep in sync with `pst-reader` `CLOUD_POINTER_HOST_SUFFIXES` /
/// `CLOUD_POINTER_HOST_EXACT` (0088: no shared crate / no reader→engine dep).
const ALLOWED_CLOUD_HOST_SUFFIXES: &[&str] = &[
    // Commercial (0085)
    "sharepoint.com",
    "sharepoint-df.com",
    "onedrive.live.com",
    "1drv.ms",
    // US GCC High / DoD (0088)
    "sharepoint.us",
    "sharepoint-mil.us",
    "dps.mil",
];

/// Exact hosts that are not covered by a broad suffix (admin-only sync endpoint).
const ALLOWED_CLOUD_HOST_EXACT: &[&str] = &["admin.onedrive.us"];

/// True when `host` equals `suffix` or is a DNS subdomain of it
/// (`contoso.sharepoint.us` yes; `notsharepoint.us` no).
fn host_matches_dns_suffix(host: &str, suffix: &str) -> bool {
    if host == suffix {
        return true;
    }
    let Some(rest) = host.strip_suffix(suffix) else {
        return false;
    };
    rest.ends_with('.')
}

/// Allowed cloud document hosts (commercial + US GCC High / DoD).
fn is_allowed_cloud_host(host: &str) -> bool {
    if ALLOWED_CLOUD_HOST_EXACT.contains(&host) {
        return true;
    }
    ALLOWED_CLOUD_HOST_SUFFIXES
        .iter()
        .any(|&suffix| host_matches_dns_suffix(host, suffix))
}

/// Document-shaped path/query markers on the cloud allowlist.
fn is_document_shaped_cloud(url_lower: &str, host: &str) -> bool {
    if !is_allowed_cloud_host(host) {
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
        assert!(!scan.window_capped);

        let scan2 = scan_body_cloud_links(Some(b""), Some(""));
        assert!(scan2.hits.is_empty());
        assert!(scan2.scanned_html);
        assert!(scan2.scanned_plain);
        assert!(!scan2.truncated);
        assert!(!scan2.window_capped);
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
        assert!(scan.max_links_exceeded);
        assert!(!scan.window_capped);
    }

    #[test]
    fn url_longer_than_2048_truncated_not_kept() {
        let long_q = "a".repeat(3000);
        let url = format!("https://contoso.sharepoint.com/:x:/s/L/f.xlsx?d={long_q}");
        assert!(url.chars().count() > MAX_URL_LEN);
        let scan = scan_body_cloud_links(Some(&html_with(&url)), None);
        assert!(
            scan.hits.is_empty(),
            "over-length URL must not be a kept hit"
        );
        assert!(scan.truncated);
        assert!(scan.url_truncated);
        assert!(!scan.window_capped);
        let prefix = scan.overlength_prefix.expect("prefix for marker");
        assert_eq!(prefix.chars().count(), MAX_URL_LEN);
        assert!(prefix.starts_with("https://contoso.sharepoint.com/:x:/s/L/f.xlsx?d="));
    }

    #[test]
    fn safelinks_nested_overlength_truncated_not_kept() {
        let long_q = "a".repeat(3000);
        let target = format!("https://contoso.sharepoint.com/:x:/s/L/f.xlsx?d={long_q}");
        assert!(target.chars().count() > MAX_URL_LEN);
        let encoded = percent_encode_for_test(&target);
        let wrapper =
            format!("https://nam06.safelinks.protection.outlook.com/?url={encoded}&data=foo");
        let scan = scan_body_cloud_links(Some(&html_with(&wrapper)), None);
        assert!(scan.hits.is_empty());
        assert!(scan.truncated);
        assert!(scan.url_truncated);
        let prefix = scan.overlength_prefix.expect("nested-target prefix");
        assert_eq!(prefix.chars().count(), MAX_URL_LEN);
        assert!(prefix.starts_with("https://contoso.sharepoint.com/:x:/s/L/f.xlsx?d="));
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
    fn body_window_150k_zero_candidates_not_truncated() {
        let body = "x".repeat(150_000);
        let scan = scan_body_cloud_links(Some(body.as_bytes()), None);
        assert!(scan.hits.is_empty());
        assert!(scan.window_capped);
        assert!(
            !scan.truncated,
            "window-only zero-candidate must not set truncated"
        );
        assert!(!scan.window_dropped);
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
            "tail document-shaped candidate must set truncated"
        );
        assert!(scan.window_capped);
        assert!(scan.window_dropped);
        assert!(
            scan.hits.is_empty(),
            "URL past 100k window must not be collected"
        );
    }

    #[test]
    fn body_window_url_inside_window_still_hits() {
        let url = "https://contoso.sharepoint.com/:x:/s/Legal/early.xlsx?d=in";
        let mut body = format!(r#"<a href="{url}">early</a>"#);
        body.push_str(&"y".repeat(MAX_BODY_SCAN_CHARS)); // padding after URL; no further candidates
        let scan = scan_body_cloud_links(Some(body.as_bytes()), None);
        assert_eq!(scan.hits.len(), 1);
        assert_eq!(scan.hits[0].url, url);
        assert!(scan.window_capped);
        assert!(
            !scan.truncated,
            "padding past window with no further candidates must not set truncated"
        );
    }

    #[test]
    fn body_window_bare_url_cut_at_boundary_not_kept() {
        // Window ends mid-URL: `…/book.xls` cut from `…/book.xlsx?d=…`.
        let head = "https://contoso.sharepoint.com/:x:/s/L/book.xls";
        let tail = "x?d=pastwindow";
        let pad_len = MAX_BODY_SCAN_CHARS - head.chars().count();
        let mut body = " ".repeat(pad_len);
        body.push_str(head);
        body.push_str(tail);
        let scan = scan_body_cloud_links(None, Some(&body));
        assert!(scan.window_capped);
        assert!(
            scan.hits.is_empty(),
            "cut .xls prefix must not be a kept hit: {:?}",
            scan.hits
        );
        assert!(scan.truncated, "full straddling URL is document-shaped");
        assert!(scan.window_dropped);
    }

    #[test]
    fn body_window_tail_overlength_sets_url_truncated_and_prefix() {
        let long_q = "a".repeat(3000);
        let url = format!("https://contoso.sharepoint.com/:x:/s/L/late.xlsx?d={long_q}");
        assert!(url.chars().count() > MAX_URL_LEN);
        let mut body = "x".repeat(MAX_BODY_SCAN_CHARS);
        body.push_str(&format!(r#" <a href="{url}">late</a>"#));
        let scan = scan_body_cloud_links(Some(body.as_bytes()), None);
        assert!(
            scan.hits.is_empty(),
            "over-length tail must not be a kept hit"
        );
        assert!(scan.window_capped);
        assert!(scan.window_dropped);
        assert!(scan.url_truncated);
        assert!(scan.truncated);
        let prefix = scan.overlength_prefix.expect("prefix from tail probe");
        assert_eq!(prefix.chars().count(), MAX_URL_LEN);
        assert!(prefix.starts_with("https://contoso.sharepoint.com/:x:/s/L/late.xlsx?d="));
    }

    #[test]
    fn max_links_plus_overlength_sets_both_flags_and_prefix() {
        let mut html = String::from("<html><body>");
        for i in 0..MAX_LINKS_PER_MESSAGE {
            html.push_str(&format!(
                r#"<a href="https://contoso.sharepoint.com/:x:/s/L/f{i}.xlsx?d={i}">x</a>"#
            ));
        }
        let long_q = "a".repeat(3000);
        let long_url = format!("https://contoso.sharepoint.com/:x:/s/L/over.xlsx?d={long_q}");
        html.push_str(&format!(r#"<a href="{long_url}">x</a>"#));
        html.push_str("</body></html>");
        let scan = scan_body_cloud_links(Some(html.as_bytes()), None);
        assert_eq!(scan.hits.len(), MAX_LINKS_PER_MESSAGE);
        assert!(scan.max_links_exceeded);
        assert!(scan.url_truncated);
        assert!(scan.truncated);
        assert!(!scan.window_capped);
        let prefix = scan.overlength_prefix.expect("prefix from post-cap probe");
        assert_eq!(prefix.chars().count(), MAX_URL_LEN);
        assert!(prefix.starts_with("https://contoso.sharepoint.com/:x:/s/L/over.xlsx?d="));
        assert!(
            scan.hits
                .iter()
                .all(|h| h.url.chars().count() <= MAX_URL_LEN),
            "over-length URL must not occupy a kept-hit slot"
        );
    }

    #[test]
    fn body_window_duplicate_cut_url_not_dropped() {
        let url = "https://contoso.sharepoint.com/:x:/s/L/book.xlsx?d=1";
        let head = "https://contoso.sharepoint.com/:x:/s/L/book.xls";
        let tail = "x?d=1";
        assert_eq!(format!("{head}{tail}"), url);
        let lead = format!("{url} ");
        let pad_len = MAX_BODY_SCAN_CHARS - lead.chars().count() - head.chars().count();
        let mut body = lead;
        body.push_str(&" ".repeat(pad_len));
        body.push_str(head);
        body.push_str(tail);
        let scan = scan_body_cloud_links(None, Some(&body));
        assert_eq!(scan.hits.len(), 1);
        assert_eq!(scan.hits[0].url, url);
        assert!(scan.window_capped);
        assert!(
            !scan.truncated,
            "duplicate cut URL must not set truncated: {:?}",
            scan
        );
        assert!(!scan.window_dropped);
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

    // --- 0088 US GCC High / DoD sovereign hosts ---

    #[test]
    fn gcc_high_my_sharepoint_us_action_tokens() {
        let cases = [
            "https://contoso-my.sharepoint.us/:w:/r/personal/user/Documents/memo.docx",
            "https://contoso-my.sharepoint.us/:x:/r/personal/user/Documents/book.xlsx?d=wabc",
        ];
        for url in cases {
            let scan = scan_body_cloud_links(Some(&html_with(url)), None);
            assert_eq!(scan.hits.len(), 1, "expected GCC High -my hit: {url}");
            assert_eq!(scan.hits[0].url, url);
        }
    }

    #[test]
    fn dod_my_sharepoint_mil_us_action_tokens() {
        let cases = [
            "https://contoso-my.sharepoint-mil.us/:w:/r/personal/user/Documents/memo.docx",
            "https://contoso-my.sharepoint-mil.us/:x:/r/personal/user/Documents/book.xlsx",
        ];
        for url in cases {
            let scan = scan_body_cloud_links(Some(&html_with(url)), None);
            assert_eq!(scan.hits.len(), 1, "expected DoD -my hit: {url}");
            assert_eq!(scan.hits[0].url, url);
        }
    }

    #[test]
    fn dps_mil_document_shaped_hit() {
        let url = "https://tenant.dps.mil/:x:/r/sites/Legal/Shared%20Documents/book.xlsx?d=1";
        let scan = scan_body_cloud_links(Some(&html_with(url)), None);
        assert_eq!(scan.hits.len(), 1, "dps.mil document-shaped must hit");
        assert_eq!(scan.hits[0].url, url);
    }

    #[test]
    fn gcc_high_safelinks_unwrap_to_sharepoint_us() {
        let target = "https://contoso.sharepoint.us/:x:/s/Finance/book.xlsx?d=wabc123&csf=1";
        let encoded = percent_encode_for_test(target);
        let wrapper =
            format!("https://nam.safelinks.protection.office365.us/?url={encoded}&data=foo");
        let scan = scan_body_cloud_links(Some(&html_with(&wrapper)), None);
        assert_eq!(scan.hits.len(), 1, "hits={:?}", scan.hits);
        assert_eq!(scan.hits[0].url, target);
        assert_eq!(scan.hits[0].source, BodyCloudUrlSource::SafeLinksUnwrap);
        assert!(scan.hits[0].url.contains("?d=wabc123"));
        assert!(scan.hits[0].url.contains("&csf=1"));
    }

    #[test]
    fn sovereign_bare_roots_and_folder_f_excluded() {
        let misses = [
            "https://contoso.sharepoint.us/sites/HR",
            "https://contoso-my.sharepoint.us/personal/user/Documents",
            "https://contoso.sharepoint.us/:f:/s/Legal/folderShare",
            "https://contoso.sharepoint-mil.us/sites/HR",
            "https://contoso-my.sharepoint-mil.us/:f:/r/personal/user/Documents/folder",
            "https://tenant.dps.mil/sites/HR",
            "https://tenant.dps.mil/:f:/s/Legal/folderShare",
        ];
        for url in misses {
            let scan = scan_body_cloud_links(Some(&html_with(url)), None);
            assert!(
                scan.hits.is_empty(),
                "bare root / :f: must miss for sovereign: {url} → {:?}",
                scan.hits
            );
        }
    }

    #[test]
    fn host_suffix_rejects_lookalike() {
        assert!(!is_allowed_cloud_host("notsharepoint.com"));
        assert!(!is_allowed_cloud_host("notsharepoint.us"));
        assert!(!is_allowed_cloud_host("evilsharepoint-mil.us"));
        assert!(!is_allowed_cloud_host("notdps.mil"));
        assert!(is_allowed_cloud_host("contoso.sharepoint.us"));
        assert!(is_allowed_cloud_host("contoso-my.sharepoint-mil.us"));
        assert!(is_allowed_cloud_host("admin.onedrive.us"));
        assert!(!is_allowed_cloud_host("other.onedrive.us"));
        // 21Vianet out of P0
        assert!(!is_allowed_cloud_host("contoso.sharepoint.cn"));
    }

    #[test]
    fn admin_onedrive_us_host_allowed_but_needs_document_shape() {
        // Harmless include: host matches, but bare admin path is not document-shaped.
        let bare = "https://admin.onedrive.us/";
        let scan = scan_body_cloud_links(Some(&html_with(bare)), None);
        assert!(
            scan.hits.is_empty(),
            "admin.onedrive.us alone must not produce body-cloud rows"
        );
        assert!(is_allowed_cloud_host("admin.onedrive.us"));
    }
}
