//! Mixed-script tokenizer: CJK character n-grams + space-delimited word tokens.

use std::collections::BTreeSet;

/// Unicode ranges used for CJK run detection (documented standard blocks).
///
/// | Block | Range |
/// |---|---|
/// | CJK Unified Ideographs | U+4E00–U+9FFF |
/// | CJK Unified Ideographs Extension A | U+3400–U+4DBF |
/// | Hiragana | U+3040–U+309F |
/// | Katakana | U+30A0–U+30FF |
/// | Hangul Syllables | U+AC00–U+D7AF |
/// | CJK Compatibility Ideographs | U+F900–U+FAFF |
pub fn is_cjk_char(c: char) -> bool {
    matches!(
        c,
        '\u{4E00}'..='\u{9FFF}'
            | '\u{3400}'..='\u{4DBF}'
            | '\u{3040}'..='\u{309F}'
            | '\u{30A0}'..='\u{30FF}'
            | '\u{AC00}'..='\u{D7AF}'
            | '\u{F900}'..='\u{FAFF}'
    )
}

/// Lightweight prep: lowercase (simple Unicode lower), collapse whitespace.
///
/// Zero-width chars (ZWSP, ZWNJ, ZWJ, BOM, word joiner) are stripped.
pub fn prep_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_space = false;
    for c in text.chars() {
        if matches!(
            c,
            '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}' | '\u{2060}'
        ) {
            continue;
        }
        let lower = c.to_lowercase().next().unwrap_or(c);
        if lower.is_whitespace() {
            if !prev_space && !out.is_empty() {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(lower);
            prev_space = false;
        }
    }
    // trim trailing space
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// True when the word token is composed entirely of ASCII digits (and optional
/// common numeric separators already removed by word split).
fn is_pure_digit_word(w: &str) -> bool {
    !w.is_empty() && w.chars().all(|c| c.is_ascii_digit())
}

/// Split a non-CJK run into word tokens on whitespace and simple punctuation.
fn split_words(run: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    for c in run.chars() {
        if c.is_whitespace() || is_word_separator(c) {
            if !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
}

fn is_word_separator(c: char) -> bool {
    // Simple punctuation / symbol separators (not CJK).
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
            | '/'
            | '\\'
            | '|'
            | '+'
            | '='
            | '*'
            | '&'
            | '%'
            | '$'
            | '#'
            | '@'
            | '~'
            | '^'
            | '_'
            | '-'
            | '—'
            | '–'
            | '…'
            | '“'
            | '”'
            | '‘'
            | '’'
            | '«'
            | '»'
            | '、'
            | '。'
            | '，'
            | '；'
            | '：'
            | '！'
            | '？'
            | '（'
            | '）'
            | '【'
            | '】'
            | '『'
            | '』'
            | '「'
            | '」'
    ) || c.is_ascii_punctuation()
}

/// Emit mixed-script **tokens** (word tokens for Latin runs; CJK char n-grams
/// for CJK runs). These feed shingle construction (see `shingle`).
pub fn tokenize(prep: &str, cjk_char_n: usize, ignore_numbers: bool) -> Vec<String> {
    if prep.is_empty() || cjk_char_n == 0 {
        return Vec::new();
    }
    let mut tokens = Vec::new();
    let chars: Vec<char> = prep.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if is_cjk_char(chars[i]) {
            let start = i;
            while i < chars.len() && is_cjk_char(chars[i]) {
                i += 1;
            }
            let run = &chars[start..i];
            if run.len() >= cjk_char_n {
                for w in 0..=(run.len() - cjk_char_n) {
                    let gram: String = run[w..w + cjk_char_n].iter().collect();
                    tokens.push(gram);
                }
            } else if !run.is_empty() {
                // Short CJK run: emit the whole run as one token so short docs
                // are not silently empty (still may fail min_chars earlier).
                let gram: String = run.iter().collect();
                tokens.push(gram);
            }
        } else {
            let start = i;
            while i < chars.len() && !is_cjk_char(chars[i]) {
                i += 1;
            }
            let run: String = chars[start..i].iter().collect();
            for w in split_words(&run) {
                if ignore_numbers && is_pure_digit_word(&w) {
                    continue;
                }
                if !w.is_empty() {
                    tokens.push(w);
                }
            }
        }
    }
    tokens
}

/// Build the unique shingle **set** for Jaccard (BTreeSet for determinism).
///
/// - CJK n-gram tokens are already shingles (emitted as-is).
/// - Word tokens form overlapping *k*-shingles joined with U+001F.
///
/// Mixed documents: CJK tokens and Latin word-shingles share one set.
pub fn build_shingles(tokens: &[String], shingle_k: usize, cjk_char_n: usize) -> BTreeSet<String> {
    let _ = cjk_char_n; // CJK tokens already n-gram shingles from tokenize
    let mut set = BTreeSet::new();
    if tokens.is_empty() {
        return set;
    }

    // Partition consecutive runs: CJK-origin tokens (all chars CJK) vs word tokens.
    // CJK n-grams are shingles directly; word tokens need k-shingles.
    let mut word_buf: Vec<&str> = Vec::new();
    let flush_words = |buf: &mut Vec<&str>, set: &mut BTreeSet<String>| {
        if buf.is_empty() {
            return;
        }
        if shingle_k == 0 {
            buf.clear();
            return;
        }
        // Spec §3.4.2: space-delimited path emits only consecutive word
        // *k*-shingles. Runs with fewer than `shingle_k` words contribute
        // **zero** shingles (no 1-word fallback). Empty set → skipped later.
        if buf.len() >= shingle_k {
            for i in 0..=(buf.len() - shingle_k) {
                let shingle = buf[i..i + shingle_k].join("\u{1f}");
                set.insert(shingle);
            }
        }
        buf.clear();
    };

    for t in tokens {
        if t.chars().all(is_cjk_char) {
            flush_words(&mut word_buf, &mut set);
            set.insert(t.clone());
        } else {
            word_buf.push(t.as_str());
        }
    }
    flush_words(&mut word_buf, &mut set);
    set
}

/// Full pipeline: prep → tokenize → shingle set.
pub fn text_to_shingles(
    text: &str,
    shingle_k: usize,
    cjk_char_n: usize,
    ignore_numbers: bool,
) -> (String, BTreeSet<String>, usize) {
    let prepared = prep_text(text);
    let tokens = tokenize(&prepared, cjk_char_n, ignore_numbers);
    let token_count = tokens.len();
    let shingles = build_shingles(&tokens, shingle_k, cjk_char_n);
    (prepared, shingles, token_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prep_lowercases_and_collapses_ws() {
        assert_eq!(prep_text("  Hello   WORLD\t"), "hello world");
    }

    #[test]
    fn latin_word_shingles_k5() {
        let text = "the quick brown fox jumps over the lazy dog again more";
        let (_, shingles, _) = text_to_shingles(text, 5, 2, true);
        assert!(!shingles.is_empty());
        // first 5-shingle
        assert!(shingles.iter().any(|s| s.contains("quick")));
    }

    #[test]
    fn cjk_bigrams_non_empty() {
        // Chinese without spaces
        let text = "这是一份重要的合同文件内容用于测试近似重复检测功能是否正常工作";
        let (_, shingles, token_count) = text_to_shingles(text, 5, 2, true);
        assert!(token_count > 0, "expected CJK tokens");
        assert!(!shingles.is_empty(), "expected non-empty CJK shingles");
        // each shingle should be 2 chars for long enough text
        assert!(shingles.iter().any(|s| s.chars().count() == 2));
    }

    #[test]
    fn ignore_numbers_drops_digit_words_not_cjk() {
        let tokens = tokenize("order 12345 shipped", 2, true);
        assert!(!tokens.iter().any(|t| t == "12345"));
        assert!(tokens.iter().any(|t| t == "order"));
    }

    #[test]
    fn mixed_script_union() {
        // Consecutive Latin words form k-shingles; CJK runs add char n-grams.
        // Isolated single Latin words between CJK contribute zero Latin shingles
        // when k>1 (spec §3.4.2 — no 1-word fallback).
        let text = "hello world 合同 文件 extra words";
        let (_, shingles, _) = text_to_shingles(text, 2, 2, true);
        assert!(
            shingles.iter().any(|s| s.chars().all(is_cjk_char)),
            "expected CJK n-gram shingles"
        );
        assert!(
            shingles
                .iter()
                .any(|s| s.contains("hello") && s.contains("world")),
            "expected Latin k=2 shingle hello/world: {shingles:?}"
        );
        assert!(
            shingles
                .iter()
                .any(|s| s.contains("extra") && s.contains("words")),
            "expected Latin k=2 shingle extra/words: {shingles:?}"
        );
    }

    #[test]
    fn latin_fewer_than_k_words_yields_empty_shingles() {
        // Long enough for min_chars, but only 4 word tokens with default k=5.
        let long_four = format!(
            "{} {} {} {}",
            "a".repeat(25),
            "b".repeat(25),
            "c".repeat(25),
            "d".repeat(25)
        );
        assert!(long_four.len() >= 80);
        let tokens = tokenize(&prep_text(&long_four), 2, true);
        assert_eq!(tokens.len(), 4, "expected exactly 4 word tokens");
        let shingles = build_shingles(&tokens, 5, 2);
        assert!(
            shingles.is_empty(),
            "minhash_shingle_v1 must not emit 1-word fallbacks for <k Latin runs: {shingles:?}"
        );
    }

    #[test]
    fn latin_exactly_k_words_emits_one_shingle() {
        let words: Vec<String> = (0..5).map(|i| format!("word{i}")).collect();
        let text = words.join(" ");
        let (_, shingles, _) = text_to_shingles(&text, 5, 2, true);
        assert_eq!(shingles.len(), 1);
        assert!(shingles.iter().next().unwrap().contains("word0"));
    }
}
