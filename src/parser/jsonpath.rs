use serde_json::Value;

pub fn jsonpath_query(value: &Value, rule: &str) -> Vec<Value> {
    if let Some(rendered) = render_embedded_paths(value, rule) {
        return vec![Value::String(rendered)];
    }
    if let Ok(res) = jsonpath_lib::select(value, rule) {
        let mut out = Vec::new();
        for item in res {
            match item {
                Value::Array(items) => {
                    out.extend(items.iter().cloned());
                }
                other => out.push(other.clone()),
            }
        }
        out
    } else {
        vec![]
    }
}

pub fn jsonpath_first_string(value: &Value, rule: &str) -> Option<String> {
    if let Some(rendered) = render_embedded_paths(value, rule) {
        return Some(rendered);
    }
    let res = jsonpath_query(value, rule);
    res.first().and_then(|v| value_to_string(v))
}

pub fn value_to_string(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Array(items) => Some(
            items
                .iter()
                .filter_map(value_to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        Value::Object(_) => Some(v.to_string()),
    }
}

fn render_embedded_paths(value: &Value, rule: &str) -> Option<String> {
    if !rule.contains("{$") {
        return None;
    }
    let re = regex::Regex::new(r"\{\s*(\$[^}]+)\}").unwrap();
    let mut replaced_any = false;
    let rendered = re
        .replace_all(rule, |captures: &regex::Captures| {
            replaced_any = true;
            let path = captures.get(1).map(|m| m.as_str()).unwrap_or_default();
            jsonpath_first_string(value, path).unwrap_or_default()
        })
        .into_owned();
    if replaced_any {
        Some(rendered)
    } else {
        Some(String::new())
    }
}
