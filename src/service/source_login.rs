use crate::{crawler::url_analyzer::{absolute_url, analyze_url}, model::book_source::BookSource, parser::js::{eval_js_with_bindings, source_http_context, take_js_logs, take_login_effects}};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use crate::service::{book_service::take_show_browser_request, source_browser::{register_browser_request, source_panel_entry, take_browser_cancelled}};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginField {
    pub name: String,
    #[serde(default = "text_type", rename = "type")]
    pub kind: String,
    pub action: Option<String>,
    pub default: Option<Value>,
    pub placeholder: Option<String>,
    pub view_name: Option<String>,
    pub chars: Option<Vec<Value>>,
    pub style: Option<Value>,
}

fn text_type() -> String { "text".to_string() }
fn script(raw: &str) -> &str {
    let raw = raw.trim();
    raw.strip_prefix("@js:").or_else(|| raw.strip_prefix("js:"))
        .or_else(|| raw.strip_prefix("<js>").and_then(|s| s.strip_suffix("</js>"))).unwrap_or(raw)
}
fn string(value: &Value) -> String {
    match value { Value::String(value) => value.clone(), Value::Null => String::new(), other => other.to_string() }
}

fn evaluate(source: &BookSource, code: &str, values: &Map<String, Value>) -> anyhow::Result<String> {
    let bindings = HashMap::from([("result".to_string(), Value::Object(values.clone())), ("isLongClick".to_string(), Value::Bool(false))]);
    eval_js_with_bindings(code, "", &source.book_source_url, &bindings)
}

/// Gson LENIENT 风格的宽容 JSON 预处理：给不带引号的对象 key 补引号。
/// legado 端用 Gson（默认 LENIENT）解析 loginUi，接受 `name: "xxx"` 这类
/// 裸 key（部分源这么写）；serde_json 是严格 JSON，需要先修复。
/// 逐字符扫描并跟踪字符串/转义状态，只在「{ 或 , 之后、冒号之前」的位置补引号。
fn quote_bare_keys(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let mut out = String::with_capacity(raw.len() + 32);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            // 原样复制整个字符串字面量（含转义）
            out.push(c);
            i += 1;
            while i < chars.len() {
                out.push(chars[i]);
                if chars[i] == '\\' && i + 1 < chars.len() {
                    out.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                if chars[i] == '"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if c == '{' || c == ',' {
            out.push(c);
            i += 1;
            while i < chars.len() && chars[i].is_whitespace() {
                out.push(chars[i]);
                i += 1;
            }
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$') {
                i += 1;
            }
            if start < i {
                let mut j = i;
                while j < chars.len() && chars[j] == ' ' {
                    j += 1;
                }
                if j < chars.len() && chars[j] == ':' {
                    out.push('"');
                    out.extend(&chars[start..i]);
                    out.push('"');
                } else {
                    // 不是 key 位置，原样保留
                    out.extend(&chars[start..i]);
                }
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn parse_lenient_rows(json: &str) -> anyhow::Result<Vec<LoginField>> {
    match serde_json::from_str(json) {
        Ok(rows) => Ok(rows),
        Err(strict_err) => match serde_json::from_str(&quote_bare_keys(json)) {
            Ok(rows) => Ok(rows),
            Err(_) => Err(anyhow::anyhow!("登录表单格式错误: {strict_err}")),
        },
    }
}

/// 可编辑控件：参与 `values`（登录信息 map），名字是取值键。
fn takes_value(kind: &str) -> bool {
    matches!(kind, "text" | "password" | "toggle" | "select")
}

/// 认识的控件类型。`label` 是 legado 之外由部分分支客户端（含本项目前端）
/// 支持的纯文字说明控件：不渲染输入，也不进 `values`。
fn is_supported(kind: &str) -> bool {
    takes_value(kind) || matches!(kind, "button" | "label")
}

fn parse_rows(source: &BookSource, values: &Map<String, Value>) -> anyhow::Result<Vec<LoginField>> {
    let Some(raw) = source.login_ui.as_deref().filter(|s| !s.trim().is_empty()) else { return Ok(Vec::new()) };
    let raw = raw.trim();
    let json = if raw.starts_with("@js:") || raw.starts_with("js:") || raw.starts_with("<js>") {
        evaluate(source, &format!("{}\n{}", script(source.login_url.as_deref().unwrap_or("")), script(raw)), values)?
    } else { raw.to_string() };
    let mut rows = parse_lenient_rows(&json)?;

    // legado 的 `rowUiBuilder` 是对 `RowUi.Type` 的 `when`，**没有 else 分支**：
    // 不认识的类型既不报错也不渲染，其余控件照常显示。这里早先对未知类型直接
    // `Err`，于是一个只在部分分支客户端（如带 `label` 的增强版）里合法的书源
    // 会让整个登录面板打不开 —— 报「不支持的登录控件: label」。
    // 因此改为：认识的照常，不认识的跳过并记日志，绝不整页失败。
    rows.retain(|row| {
        let kind = row.kind.trim().to_ascii_lowercase();
        if is_supported(&kind) {
            return true;
        }
        // 用 `tracing` 而不是 JS 日志：`take_js_logs()` 在本次求值结束后就被取走
        // 当 toast 列表，这里的提示会混进用户可见的消息里。
        tracing::warn!(
            target: "reader::login",
            "跳过不支持的登录控件类型: {}（{}）",
            row.kind,
            row.name
        );
        false
    });
    for row in &mut rows {
        row.kind = row.kind.trim().to_ascii_lowercase();
        // 展示控件的文字可以直接写在 `name` 里（书源常见写法），
        // 因此只有可编辑控件要求 name 非空（它是取值键）。
        anyhow::ensure!(
            !takes_value(&row.kind) || !row.name.trim().is_empty(),
            "登录表单字段名称不能为空"
        );
    }
    Ok(rows)
}

/// `viewName` 的显示文本：`'字面量'` 直接取引号内文本，其余按 JS 表达式求值
/// （legado `SourceLoginDialog.evalUiJs`，脚本前拼 `loginUrl`，`result` 绑定表单值）。
///
/// 求值失败不报错：legado 在 `.onError` 里把文字显示成 `err`，但整块面板照常
/// 可用；这里回退成 `None`，前端用 `name` 兜底。
fn eval_view_name(source: &BookSource, values: &Map<String, Value>, row: &LoginField) -> Option<String> {
    let raw = row.view_name.as_deref()?.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(literal) = crate::parser::js::literal_view_name(raw) {
        return Some(literal);
    }
    let code = format!("{}\n{}", script(source.login_url.as_deref().unwrap_or("")), script(raw));
    match evaluate(source, &code, values) {
        Ok(text) => {
            let text = text.trim().to_string();
            (!text.is_empty() && !text.eq_ignore_ascii_case("null")).then_some(text)
        }
        Err(err) => {
            // 每点一次按钮都会重算一遍，日志要能看出是哪个控件出的问题。
            tracing::warn!(target: "reader::login", "登录控件 viewName 求值失败（{}）: {err}", row.name);
            None
        }
    }
}

fn fields(source: &BookSource, values: &Map<String, Value>) -> anyhow::Result<Vec<LoginField>> {
    let mut rows = parse_rows(source, values)?;
    // 动态显示名：本源的「状态栏 / 切换网址」按钮把 `viewName` 写成
    // `'🌐 当前：'+(source.getVariable()||source.getKey())`，不求值就会把整段
    // JS 源码当按钮文字显示出来。
    for row in &mut rows {
        if let Some(text) = eval_view_name(source, values, row) {
            row.view_name = Some(text);
        }
    }
    Ok(rows)
}

/// Actions are resolved from the saved source's current form, so the frontend
/// sends a row index and values rather than an arbitrary script.
pub fn run(source: &BookSource, action: Option<&str>, supplied_values: Option<Map<String, Value>>) -> anyhow::Result<Value> {
    take_js_logs();
    take_login_effects();
    take_show_browser_request();
    take_browser_cancelled();
    let context = source_http_context(&source.book_source_url)?;
    let session = context.session;
    let mut values = supplied_values.unwrap_or_else(|| session.get("loginForm").or_else(|| session.get("loginInfo"))
        .and_then(|raw| serde_json::from_str::<Map<String, Value>>(&raw).ok()).unwrap_or_default());
    for value in values.values_mut() { *value = json!(string(value)); }
    let is_form = source.login_ui.as_deref().is_some_and(|s| !s.trim().is_empty());
    let mut rows = fields(source, &values)?;
    for row in &rows {
        if takes_value(&row.kind) { values.entry(row.name.clone()).or_insert_with(|| json!(default_value(row))); }
    }
    let mut open_url = None;
    if let Some(action) = action {
        anyhow::ensure!(is_form, "当前书源未配置登录表单");
        let code = if action == "login" {
            "if(typeof login !== 'function') throw new Error('书源未定义登录操作'); login();".to_string()
        } else {
            let index = action.strip_prefix("button:").and_then(|v| v.parse::<usize>().ok()).ok_or_else(|| anyhow::anyhow!("无效登录操作"))?;
            let row = rows.get(index).ok_or_else(|| anyhow::anyhow!("登录表单已变化，请重新打开"))?;
            row.action.clone().unwrap_or_default()
        };
        session.put("loginInfo", Some(&Value::Object(values.clone()).to_string()))?;
        if code.starts_with("https://") || code.starts_with("http://") {
            open_url = Some(code);
        } else if !code.trim().is_empty() {
            if let Err(err) = evaluate(source, &format!("{}\n{}", script(source.login_url.as_deref().unwrap_or("")), script(&code)), &values) {
                take_show_browser_request();
                if !crate::service::source_browser::browser_was_cancelled() {
                    return Err(err);
                }
            }
        }
        let effects = take_login_effects();
        apply_effects(&rows, &mut values, effects);
        rows = fields(source, &values)?;
    }
    apply_effects(&rows, &mut values, take_login_effects());
    for row in &mut rows {
        if row.kind == "button" {
            if let Some(value) = values.get(&row.name) { row.view_name = Some(string(value)); }
        } else if takes_value(&row.kind) {
            values.entry(row.name.clone()).or_insert_with(|| json!(default_value(row)));
        }
    }
    if is_form { session.put("loginForm", Some(&Value::Object(values.clone()).to_string()))?; }
    let url = if is_form { None } else if let Some(raw) = source.login_url.as_deref().filter(|s| !s.trim().is_empty()) {
        // A bare program must never be appended to the source domain as a path.
        let start = raw.trim();
        anyhow::ensure!(!["function ", "var ", "let ", "const "].iter().any(|prefix| start.starts_with(prefix)), "此书源包含登录脚本，但未配置登录表单");
        let spec = analyze_url(raw, "", 1, &source.book_source_url, source)?;
        let url = absolute_url(&source.book_source_url, &spec.url);
        let parsed = url::Url::parse(&url)?;
        anyhow::ensure!(["http", "https"].contains(&parsed.scheme()), "登录页地址必须是网页地址");
        Some(url)
    } else { None };
    let mut messages = take_js_logs().into_iter().filter(|line| line.starts_with("[toast]"))
        .map(|line| line.split_once(']').map(|(_, text)| text.trim().to_string()).unwrap_or(line)).collect::<Vec<_>>();
    let cancelled = take_browser_cancelled();
    if cancelled { messages = vec!["已取消".to_string()]; }
    let browser = take_show_browser_request().map(|request| {
        register_browser_request(request, source_panel_entry(&source.book_source_url)?)
    }).transpose()?;
    // `mode: "web"` 的语义是「登录页是一个真实网页」。这种页面必须用**独立窗口**
    // 打开，不能塞进 iframe：上游常带 `X-Frame-Options: SAMEORIGIN` / CSP
    // `frame-ancestors`，或用前端安全 SDK 检测「自己是否被 iframe 嵌入」——
    // 命中就整页替换成「页面渲染安全策略拦截」。独立窗口里 `window.top === window`，
    // 检测不触发，与手机端 legado 直接 `loadUrl` 一致。
    //
    // `externalWindow` 让前端走 `open_external_window`（窗口关闭时自动回收
    // Cookie 写回书源 jar）；`url` 仍照旧返回，便于调试与兼容旧前端。
    let is_web = !is_form && url.is_some();
    Ok(json!({
        "mode": if is_form {"form"} else if is_web {"web"} else {"cookie"},
        "fields":rows, "values":values, "url":url, "openUrl":open_url,
        "messages":messages, "actionCompleted":action.is_some() && !cancelled,
        "showBrowser":browser, "cancelled":cancelled,
        "externalWindow":is_web,
    }))
}

fn default_value(row: &LoginField) -> String {
    row.default.as_ref().filter(|v| !v.is_null())
        .or_else(|| if matches!(row.kind.as_str(), "toggle" | "select") {
            row.chars.as_ref().and_then(|chars| chars.iter().find(|v| !v.is_null()))
        } else { None }).map(string).unwrap_or_default()
}

fn apply_effects(rows: &[LoginField], values: &mut Map<String, Value>, effects: crate::parser::js::LoginEffects) {
    if effects.reset {
        values.clear();
        // `label` 不是取值键：`java.upLoginData(null)` 的重置只回填可编辑控件，
        // 否则展示文字会混进登录信息 map 一起写进 `source.putLoginInfo`。
        for row in rows.iter().filter(|row| takes_value(&row.kind)) {
            values.insert(row.name.clone(), json!(default_value(row)));
        }
    }
    for (name, value) in effects.values {
        let text = if value.is_null() {
            rows.iter().find(|row| row.name == name).map(default_value).unwrap_or_default()
        } else { string(&value) };
        values.insert(name, json!(text));
    }
}
