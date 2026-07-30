use anyhow::{Context, Result};
use fs_err as fs;
use serde_json::{Map, Value};
use std::path::Path;

use crate::file_io::write_atomic;

pub fn read_json_value(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    // strip comments (line // incl. trailing, block /* */) and trailing commas
    // for jsonc-ish files. Real OpenCode/Pi JSONC commonly carries trailing
    // comments, block comments and trailing commas; the old full-line-only
    // stripper failed those, which then cascaded into a blanked bindings read.
    let cleaned = strip_jsonc_comments(&text);
    let cleaned = strip_trailing_commas(&cleaned);
    serde_json::from_str(&cleaned).with_context(|| format!("parse {}", path.display()))
}

pub fn write_json_value(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(value)?;
    write_atomic(path, format!("{text}\n").as_bytes())
}

/// Strip `//` line comments (full-line or trailing) and `/* */` block
/// comments from JSONC-ish text, while leaving string literals untouched.
/// `//` or `/*` inside a JSON string are preserved verbatim.
pub fn strip_jsonc_comments(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    let mut in_string = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            out.push(c as char);
            if c == b'\\' && i + 1 < bytes.len() {
                // copy the escaped char verbatim (e.g. \/, ", \n)
                out.push(bytes[i + 1] as char);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_string = true;
            out.push(c as char);
            i += 1;
            continue;
        }
        if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            // line comment: skip to end of line (keep the newline)
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            // block comment: skip to closing */
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        out.push(c as char);
        i += 1;
    }
    out
}

/// Remove trailing commas before `}` or `]`, string-aware. JSONC allows them;
/// `serde_json` does not.
pub fn strip_trailing_commas(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    let mut in_string = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            out.push(c as char);
            if c == b'\\' && i + 1 < bytes.len() {
                out.push(bytes[i + 1] as char);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_string = true;
            out.push(c as char);
            i += 1;
            continue;
        }
        if c == b',' {
            // peek ahead, skipping whitespace, to see if the next significant
            // char closes a struct/array — if so, drop the comma.
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && (bytes[j] == b'}' || bytes[j] == b']') {
                i += 1;
                continue;
            }
        }
        out.push(c as char);
        i += 1;
    }
    out
}

pub fn ensure_object<'a>(value: &'a mut Value) -> Result<&'a mut serde_json::Map<String, Value>> {
    if !value.is_object() {
        *value = Value::Object(serde_json::Map::new());
    }
    value.as_object_mut().context("expected JSON object")
}

/// Find the on-disk provider block that corresponds to a ModelHub provider.
/// Prefer the stable managed id so provider renames (and therefore slug changes)
/// can still inherit native settings; fall back to the key we are about to
/// write for legacy/unmanaged blocks.
pub fn find_existing_provider_entry<'a>(
    providers: &'a Map<String, Value>,
    provider_id: &str,
    target_key: &str,
) -> Option<(&'a str, &'a Value)> {
    providers
        .iter()
        .find(|(_, value)| {
            value
                .get("_modelhub")
                .and_then(|v| v.get("providerId"))
                .and_then(Value::as_str)
                == Some(provider_id)
        })
        .map(|(key, value)| (key.as_str(), value))
        .or_else(|| {
            providers
                .get_key_value(target_key)
                .map(|(key, value)| (key.as_str(), value))
        })
}

pub fn is_modelhub_managed_provider(value: &Value) -> bool {
    value
        .get("_modelhub")
        .and_then(|v| v.get("managed"))
        .and_then(Value::as_bool)
        == Some(true)
}

pub fn retain_unmanaged_provider_entries(providers: &mut Map<String, Value>) {
    providers.retain(|_, value| !is_modelhub_managed_provider(value));
}

/// Keys of provider blocks on disk that are NOT managed by ModelHub. Generated
/// write keys avoid these so a ModelHub provider never overwrites or takes over
/// a native/user block that merely happens to share the slug.
pub fn unmanaged_provider_keys(providers: &Map<String, Value>) -> std::collections::HashSet<String> {
    providers
        .iter()
        .filter(|(_, value)| !is_modelhub_managed_provider(value))
        .map(|(key, _)| key.clone())
        .collect()
}

pub fn set_string_path(obj: &mut serde_json::Map<String, Value>, path: &[&str], val: String) {
    if path.is_empty() {
        return;
    }
    if path.len() == 1 {
        obj.insert(path[0].to_string(), Value::String(val));
        return;
    }
    let head = path[0];
    let entry = obj
        .entry(head.to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if let Some(map) = entry.as_object_mut() {
        set_string_path(map, &path[1..], val);
    } else {
        let mut nested = serde_json::Map::new();
        set_string_path(&mut nested, &path[1..], val);
        *entry = Value::Object(nested);
    }
}

pub fn remove_path(obj: &mut serde_json::Map<String, Value>, path: &[&str]) {
    if path.is_empty() {
        return;
    }
    if path.len() == 1 {
        obj.remove(path[0]);
        return;
    }
    if let Some(Value::Object(map)) = obj.get_mut(path[0]) {
        remove_path(map, &path[1..]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn provider_match_prefers_stable_modelhub_id_before_target_key() {
        let providers = json!({
            "old-slug": {
                "_modelhub": { "providerId": "prov_1" },
                "custom": "preserve-me"
            },
            "new-slug": { "custom": "wrong-block" }
        });
        let map = providers.as_object().unwrap();

        let (key, value) = find_existing_provider_entry(map, "prov_1", "new-slug").unwrap();

        assert_eq!(key, "old-slug");
        assert_eq!(value["custom"], "preserve-me");
    }

    #[test]
    fn cleanup_removes_only_modelhub_managed_provider_blocks() {
        let mut providers = json!({
            "managed": { "_modelhub": { "managed": true } },
            "native": { "headers": { "X-Native": "keep" } }
        })
        .as_object()
        .unwrap()
        .clone();

        retain_unmanaged_provider_entries(&mut providers);

        assert!(!providers.contains_key("managed"));
        assert_eq!(providers["native"]["headers"]["X-Native"], "keep");
    }

    #[test]
    fn jsonc_strips_line_and_trailing_comments_and_trailing_commas() {
        let jsonc = r#"{
            // full-line comment
            "a": 1, // trailing comment
            /* block
               comment */ "b": "str // not a comment", /* inline block */
            "c": [1, 2, 3,],
            "d": {"x": 1,},
        }"#;

        let cleaned = strip_jsonc_comments(jsonc);
        let cleaned = strip_trailing_commas(&cleaned);
        let val: Value = serde_json::from_str(&cleaned).expect("jsonc must parse after strip");

        assert_eq!(val["a"], 1);
        assert_eq!(val["b"], "str // not a comment");
        assert_eq!(val["c"].as_array().unwrap().len(), 3);
        assert_eq!(val["d"]["x"], 1);
    }
}
