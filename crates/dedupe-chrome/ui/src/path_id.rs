//! Percent-encode / decode absolute matter roots for `/matters/:id` routes.

use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};

/// Encode path characters that are unsafe in a single URL path segment.
const PATH_SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'&')
    .add(b'\'')
    .add(b'+')
    .add(b',')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

pub fn encode_matter_id(root: &str) -> String {
    utf8_percent_encode(root, PATH_SEGMENT).to_string()
}

/// Decode a percent-encoded matter id string (not a ParamsMap value).
#[allow(dead_code)] // Used by unit tests; ParamsMap paths must not call this.
pub fn decode_matter_id(id: &str) -> Result<String, String> {
    percent_decode_str(id)
        .decode_utf8()
        .map(|s| s.into_owned())
        .map_err(|e| format!("invalid matter id encoding: {e}"))
}

/// Build `/matters/:id` from a router param.
///
/// `ParamsMap` already URL-decodes — treat `id_param` as the absolute root and
/// only encode for the href segment (never decode again).
pub fn matter_home_href_from_param(id_param: &str) -> String {
    format!("/matters/{}", encode_matter_id(id_param))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_windows_path_with_spaces_and_accents() {
        let root = r"C:\Cases\Smith v. José Müller";
        let enc = encode_matter_id(root);
        assert!(!enc.contains(' '), "spaces must be encoded");
        assert!(!enc.contains('\\'), "backslashes must be encoded");
        assert!(!enc.contains(':'), "drive colon must be encoded");
        assert!(!enc.contains('/'), "slashes must not appear raw in :id");
        let dec = decode_matter_id(&enc).expect("decode");
        assert_eq!(dec, root);
        assert!(
            enc.contains("%20") || enc.contains("%C3"),
            "non-ascii/spaces encoded"
        );
        assert!(root.contains('é') && root.contains('ü'));
    }

    #[test]
    fn roundtrip_plain_drive_path() {
        let root = r"C:\matters\demo";
        assert_eq!(decode_matter_id(&encode_matter_id(root)).unwrap(), root);
    }

    #[test]
    fn stub_back_href_reencodes_decoded_windows_param() {
        // ParamsMap unescapes before components see `:id`.
        let decoded_param = r"C:\Cases\Foo";
        let href = matter_home_href_from_param(decoded_param);
        assert!(href.starts_with("/matters/"));
        let enc = href.trim_start_matches("/matters/");
        assert!(enc.contains("%3A"), "drive colon must be %3A, got {enc}");
        assert!(enc.contains("%5C"), "backslash must be %5C, got {enc}");
        assert!(!enc.contains('\\'), "raw backslash breaks the path segment");
        assert!(!enc.contains(':'), "raw colon breaks the path segment");
        assert_eq!(decode_matter_id(enc).expect("decode"), decoded_param);
    }

    #[test]
    fn literal_percent_in_root_not_double_decoded_from_params() {
        let root = r"C:\Cases\100%20Done\%25done";
        let encoded_for_url = encode_matter_id(root);
        let params_value = decode_matter_id(&encoded_for_url).expect("router unescape");
        assert_eq!(params_value, root);
        let href = matter_home_href_from_param(&params_value);
        let enc = href.trim_start_matches("/matters/");
        assert_eq!(enc, encoded_for_url);
        let wrongly_double = decode_matter_id(&params_value).expect("lenient decode");
        assert_ne!(wrongly_double, root);
    }
}
