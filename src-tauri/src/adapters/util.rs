use anyhow::{Context, Result};
use fs_err as fs;
use serde_json::Value;
use std::path::Path;

pub fn read_json_value(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    // strip simple // comments for jsonc-ish files
    let cleaned = strip_json_line_comments(&text);
    serde_json::from_str(&cleaned).with_context(|| format!("parse {}", path.display()))
}

pub fn write_json_value(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    let text = serde_json::to_string_pretty(value)?;
    fs::write(&tmp, format!("{text}\n"))?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn strip_json_line_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for line in input.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        // keep lines; naive strip trailing // outside strings is complex — only skip full-line comments
        out.push_str(line);
        out.push('\n');
    }
    out
}

pub fn ensure_object<'a>(value: &'a mut Value) -> Result<&'a mut serde_json::Map<String, Value>> {
    if !value.is_object() {
        *value = Value::Object(serde_json::Map::new());
    }
    value
        .as_object_mut()
        .context("expected JSON object")
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
