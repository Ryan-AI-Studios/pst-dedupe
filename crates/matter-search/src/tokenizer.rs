//! Hybrid CJK + Latin tokenizer for Tantivy (track 0054).
//!
//! - **CJK runs** (Han / Hiragana / Katakana / Hangul / compat): character bigrams
//!   with sequential positions; single leftover char → unigram.
//! - **Non-CJK runs:** lowercase; whitespace-split; email-safe tokens
//!   (`bob@example.com` kept intact — do not split on `@` / `.`).

use matter_core::is_cjk_char;
use tantivy::tokenizer::{Token, TokenStream, Tokenizer};

use crate::pack::{CJK_MAX_GRAM, CJK_MIN_GRAM};

/// Hybrid script-boundary tokenizer registered as `cjk_hybrid_v1`.
#[derive(Clone, Default)]
pub struct HybridCjkTokenizer {
    token: Token,
}

/// Precomputed token stream for [`HybridCjkTokenizer`].
pub struct HybridCjkTokenStream<'a> {
    tokens: Vec<Token>,
    cursor: usize,
    token: &'a mut Token,
}

impl Tokenizer for HybridCjkTokenizer {
    type TokenStream<'a> = HybridCjkTokenStream<'a>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> HybridCjkTokenStream<'a> {
        self.token.reset();
        let tokens = emit_hybrid_tokens(text);
        HybridCjkTokenStream {
            tokens,
            cursor: 0,
            token: &mut self.token,
        }
    }
}

impl TokenStream for HybridCjkTokenStream<'_> {
    fn advance(&mut self) -> bool {
        if self.cursor >= self.tokens.len() {
            return false;
        }
        *self.token = self.tokens[self.cursor].clone();
        self.cursor += 1;
        true
    }

    fn token(&self) -> &Token {
        self.token
    }

    fn token_mut(&mut self) -> &mut Token {
        self.token
    }
}

/// Emit hybrid tokens with sequential positions (unit-testable).
pub fn emit_hybrid_tokens(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut position: usize = 0;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (start_byte, c) = chars[i];
        if is_cjk_char(c) {
            let run_start = i;
            i += 1;
            while i < chars.len() && is_cjk_char(chars[i].1) {
                i += 1;
            }
            let run: Vec<(usize, char)> = chars[run_start..i].to_vec();
            emit_cjk_run(&run, text, &mut out, &mut position);
        } else {
            i += 1;
            while i < chars.len() && !is_cjk_char(chars[i].1) {
                i += 1;
            }
            let end_byte = if i < chars.len() {
                chars[i].0
            } else {
                text.len()
            };
            let run = &text[start_byte..end_byte];
            let tokens_before = out.len();
            emit_latin_run(run, start_byte, &mut out, &mut position);
            // Whitespace/punctuation-only non-CJK runs emit no tokens. Bump
            // position so CJK bigrams across separators are not consecutive
            // (avoids phrase false-matches for e.g. `中国 国公` vs `中国公`).
            if out.len() == tokens_before && !run.is_empty() {
                position = position.saturating_add(1);
            }
        }
    }
    out
}

fn emit_cjk_run(
    run: &[(usize, char)],
    full_text: &str,
    out: &mut Vec<Token>,
    position: &mut usize,
) {
    let n = CJK_MIN_GRAM as usize;
    let max = CJK_MAX_GRAM as usize;
    debug_assert_eq!(n, max, "P0: bigrams only");
    if run.is_empty() {
        return;
    }
    if run.len() < n {
        // Single leftover CJK char → unigram so 1-char queries work.
        let (byte_from, ch) = run[0];
        let byte_to = byte_from + ch.len_utf8();
        out.push(make_token(
            *position,
            &full_text[byte_from..byte_to],
            byte_from,
            byte_to,
        ));
        *position = position.saturating_add(1);
        return;
    }
    // Sliding character bigrams with sequential positions.
    for w in 0..=(run.len() - n) {
        let byte_from = run[w].0;
        let last = run[w + n - 1];
        let byte_to = last.0 + last.1.len_utf8();
        let gram = &full_text[byte_from..byte_to];
        out.push(make_token(*position, gram, byte_from, byte_to));
        *position = position.saturating_add(1);
    }
}

fn emit_latin_run(run: &str, base_offset: usize, out: &mut Vec<Token>, position: &mut usize) {
    // Lowercase then whitespace-split; email-safe within words.
    let lower: String = run.chars().flat_map(|c| c.to_lowercase()).collect();
    // Map lower indices carefully: lowercasing can change length (İ → i̇).
    // Work on the original run for offsets; lowercase only the emitted text.
    let mut word_start: Option<usize> = None;
    let bytes = run.as_bytes();
    let mut i = 0;
    while i <= bytes.len() {
        let at_end = i == bytes.len();
        let is_space = !at_end && (bytes[i] as char).is_whitespace();
        if at_end || is_space {
            if let Some(ws) = word_start {
                let word = &run[ws..i];
                emit_latin_word(word, base_offset + ws, out, position);
                word_start = None;
            }
            if at_end {
                break;
            }
            i += 1;
            continue;
        }
        if word_start.is_none() {
            word_start = Some(i);
        }
        // Advance one char.
        let ch = run[i..].chars().next().unwrap_or(' ');
        i += ch.len_utf8();
    }
    let _ = lower; // lower applied per-token in emit_latin_word
}

fn emit_latin_word(word: &str, base_offset: usize, out: &mut Vec<Token>, position: &mut usize) {
    if word.is_empty() {
        return;
    }
    // Email / domain-like: keep intact (do not split on @ or .).
    // Trim *all* edge ASCII punctuation including trailing `.` so
    // `bob@example.com.` matches `bob@example.com`.
    if word.contains('@') {
        let (trimmed, off_from, off_to) = trim_email_edge_punct(word);
        if trimmed.is_empty() {
            return;
        }
        let text: String = trimmed.chars().flat_map(|c| c.to_lowercase()).collect();
        out.push(make_token(
            *position,
            &text,
            base_offset + off_from,
            base_offset + off_to,
        ));
        *position = position.saturating_add(1);
        return;
    }

    // Split on non-alphanumeric (SimpleTokenizer-like); keep alnum runs.
    let mut cur_start: Option<usize> = None;
    let mut byte_i = 0;
    for ch in word.chars() {
        let len = ch.len_utf8();
        if ch.is_alphanumeric() {
            if cur_start.is_none() {
                cur_start = Some(byte_i);
            }
        } else if let Some(s) = cur_start.take() {
            let piece = &word[s..byte_i];
            let text: String = piece.chars().flat_map(|c| c.to_lowercase()).collect();
            if !text.is_empty() {
                out.push(make_token(
                    *position,
                    &text,
                    base_offset + s,
                    base_offset + byte_i,
                ));
                *position = position.saturating_add(1);
            }
        }
        byte_i += len;
    }
    if let Some(s) = cur_start {
        let piece = &word[s..];
        let text: String = piece.chars().flat_map(|c| c.to_lowercase()).collect();
        if !text.is_empty() {
            out.push(make_token(
                *position,
                &text,
                base_offset + s,
                base_offset + word.len(),
            ));
            *position = position.saturating_add(1);
        }
    }
}

/// Sentence / wrapper punctuation safe to strip at email token edges.
///
/// Does **not** strip valid email-local-part characters such as `+`, `_`, `=`,
/// `#`, `%` (RFC 5322 atext). Still strips trailing `.` so `bob@example.com.`
/// indexes as `bob@example.com`.
fn is_email_sentence_edge_punct(c: char) -> bool {
    matches!(
        c,
        '.' | ','
            | ';'
            | ':'
            | '!'
            | '?'
            | '"'
            | '\''
            | '`'
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '<'
            | '>'
    )
}

/// Trim sentence-boundary punctuation from email-like tokens.
///
/// Internal `@` and `.` are preserved; only edges are stripped.
fn trim_email_edge_punct(s: &str) -> (&str, usize, usize) {
    let bytes = s.as_bytes();
    let mut start = 0;
    let mut end = bytes.len();
    while start < end {
        let c = bytes[start] as char;
        if is_email_sentence_edge_punct(c) {
            start += 1;
        } else {
            break;
        }
    }
    while end > start {
        let c = bytes[end - 1] as char;
        if is_email_sentence_edge_punct(c) {
            end -= 1;
        } else {
            break;
        }
    }
    (&s[start..end], start, end)
}

fn make_token(position: usize, text: &str, offset_from: usize, offset_to: usize) -> Token {
    Token {
        offset_from,
        offset_to,
        position,
        text: text.to_string(),
        position_length: 1,
    }
}

/// Rewrite a user query so consecutive CJK character runs become Tantivy phrases.
///
/// - Leaves existing quoted regions untouched.
/// - Latin segments / boolean operators left as-is.
/// - Example: `合同会社 Acme` → `"合同会社" Acme`
pub fn rewrite_cjk_query_phrases(query: &str) -> String {
    let mut out = String::with_capacity(query.len() + 8);
    let mut chars = query.chars().peekable();
    let mut in_quotes = false;
    while let Some(c) = chars.next() {
        if c == '"' {
            in_quotes = !in_quotes;
            out.push(c);
            continue;
        }
        if !in_quotes && is_cjk_char(c) {
            let mut run = String::new();
            run.push(c);
            while let Some(&n) = chars.peek() {
                if is_cjk_char(n) {
                    if let Some(ch) = chars.next() {
                        run.push(ch);
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            out.push('"');
            out.push_str(&run);
            out.push('"');
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(s: &str) -> Vec<String> {
        emit_hybrid_tokens(s).into_iter().map(|t| t.text).collect()
    }

    #[test]
    fn cjk_bigrams_sequential() {
        let toks = emit_hybrid_tokens("株式会社");
        let words: Vec<_> = toks.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(words, vec!["株式", "式会", "会社"]);
        assert_eq!(toks[0].position, 0);
        assert_eq!(toks[1].position, 1);
        assert_eq!(toks[2].position, 2);
    }

    #[test]
    fn email_intact_under_hybrid() {
        let words = texts("contact bob@example.com please");
        assert!(
            words.iter().any(|w| w == "bob@example.com"),
            "email must stay intact, got {words:?}"
        );
        assert!(words.iter().any(|w| w == "contact"));
        assert!(words.iter().any(|w| w == "please"));
    }

    #[test]
    fn email_trailing_period_trimmed() {
        let words = texts("contact bob@example.com. please");
        assert!(
            words.iter().any(|w| w == "bob@example.com"),
            "trailing period must be stripped from email, got {words:?}"
        );
        assert!(
            !words.iter().any(|w| w == "bob@example.com."),
            "must not keep trailing period token, got {words:?}"
        );
    }

    #[test]
    fn email_plus_addressing_preserved() {
        let words = texts("mail +tag@example.com end");
        assert!(
            words.iter().any(|w| w == "+tag@example.com"),
            "plus-address local-part must be preserved, got {words:?}"
        );
    }

    #[test]
    fn email_parenthetical_wrappers_stripped() {
        let words = texts("see (bob@example.com) today");
        assert!(
            words.iter().any(|w| w == "bob@example.com"),
            "parentheses wrappers must strip, got {words:?}"
        );
    }

    #[test]
    fn cjk_separator_opens_position_gap() {
        // Whitespace between CJK runs must not leave consecutive positions
        // (otherwise phrase "中国公" false-matches "中国 国公").
        let toks = emit_hybrid_tokens("中国 国公");
        let words: Vec<_> = toks.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(words, vec!["中国", "国公"], "got {words:?}");
        assert_eq!(toks[0].position, 0);
        assert!(
            toks[1].position > toks[0].position + 1,
            "expected position gap after separator, positions {} and {}",
            toks[0].position,
            toks[1].position
        );
    }

    #[test]
    fn cjk_contiguous_no_extra_gap() {
        let toks = emit_hybrid_tokens("中国公");
        let words: Vec<_> = toks.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(words, vec!["中国", "国公"]);
        assert_eq!(toks[0].position, 0);
        assert_eq!(toks[1].position, 1);
    }

    #[test]
    fn latin_lowercased() {
        let words = texts("Hello World");
        assert_eq!(words, vec!["hello", "world"]);
    }

    #[test]
    fn single_cjk_unigram() {
        let words = texts("中");
        assert_eq!(words, vec!["中"]);
    }

    #[test]
    fn rewrite_phrases() {
        assert_eq!(rewrite_cjk_query_phrases("合同会社"), "\"合同会社\"");
        assert_eq!(
            rewrite_cjk_query_phrases("合同会社 Acme"),
            "\"合同会社\" Acme"
        );
        // Existing quotes preserved (CJK inside stays as-is).
        assert_eq!(
            rewrite_cjk_query_phrases("\"合同会社\" AND bob"),
            "\"合同会社\" AND bob"
        );
    }
}
