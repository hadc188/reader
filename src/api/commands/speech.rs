use std::time::Duration;

use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;
use serde_json::{json, Value};
use url::Url;

use crate::api::AppState;
use crate::error::error::AppError;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechAudioRequest {
    pub api_format: Option<String>,
    pub base_url: String,
    pub proxy_url: Option<String>,
    pub api_key: Option<String>,
    pub input: String,
    pub model: String,
    pub voice: String,
    pub format: Option<String>,
    pub speed: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpeechApiKind {
    OpenAi,
    Fish,
    ElevenLabs,
    Azure,
}

#[tauri::command]
pub async fn request_speech_audio(
    state: tauri::State<'_, AppState>,
    req: SpeechAudioRequest,
) -> Result<tauri::ipc::Response, AppError> {
    let input = req.input.trim();
    if input.is_empty() {
        return Err(AppError::BadRequest("朗读文本不能为空".to_string()));
    }

    let kind = resolve_api_kind(req.api_format.as_deref(), &req.base_url)?;
    let format = req.format.as_deref().unwrap_or("mp3");
    validate_audio_format(kind, format)?;
    let endpoint = build_speech_endpoint(&req.base_url, kind, &req.voice, format)?;
    let proxy_client = build_proxy_client(req.proxy_url.as_deref())?;
    let client = proxy_client
        .as_ref()
        .unwrap_or_else(|| state.book_service.http_client());
    let mut request = client
        .post(endpoint)
        .timeout(Duration::from_secs(120));

    if let Some(api_key) = req
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
    {
        request = match kind {
            SpeechApiKind::OpenAi | SpeechApiKind::Fish => request.bearer_auth(api_key),
            SpeechApiKind::ElevenLabs => request.header("xi-api-key", api_key),
            SpeechApiKind::Azure => request.header("Ocp-Apim-Subscription-Key", api_key),
        };
    }

    request = match kind {
        SpeechApiKind::OpenAi => request.json(&json!({
            "model": req.model,
            "input": input,
            "voice": req.voice,
            "response_format": format,
            "speed": req.speed,
        })),
        SpeechApiKind::Fish => {
            let backend = match req.model.trim() {
                "" | "qwen-tts" => "s2.1-pro",
                model => model,
            };
            request = request.header("model", backend);

            let mut body = json!({
                "text": input,
                "format": format,
            });
            if let Some(speed) = req.speed {
                body["prosody"] = json!({ "speed": speed });
            }
            let voice = req.voice.trim();
            if !voice.is_empty() && voice != "vivian" {
                body["reference_id"] = Value::String(voice.to_string());
            }
            request.json(&body)
        }
        SpeechApiKind::ElevenLabs => {
            let mut body = json!({ "text": input });
            let model = req.model.trim();
            if !model.is_empty() {
                body["model_id"] = Value::String(model.to_string());
            }
            if let Some(speed) = req.speed {
                body["voice_settings"] = json!({ "speed": speed.clamp(0.7, 1.2) });
            }
            request.json(&body)
        }
        SpeechApiKind::Azure => {
            let output_format = azure_output_format(format).ok_or_else(|| {
                AppError::BadRequest(format!("所选接口格式不支持 {format} 音频格式"))
            })?;
            let language = match req.model.trim() {
                "" => "zh-CN",
                language => language,
            };
            let voice = req.voice.trim();
            if voice.is_empty() {
                return Err(AppError::BadRequest("请填写语音音色".to_string()));
            }
            request
                .header(CONTENT_TYPE, "application/ssml+xml")
                .header("X-Microsoft-OutputFormat", output_format)
                .body(build_azure_ssml(input, language, voice, req.speed))
        }
    };

    let response = request
        .send()
        .await
        .map_err(|error| {
            let hint = if error.is_connect() || error.is_timeout() {
                "，请检查网络或 HTTP 代理设置"
            } else {
                ""
            };
            AppError::BadRequest(format!("语音服务连接失败: {error}{hint}"))
        })?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| AppError::BadRequest(format!("读取语音响应失败: {error}")))?;

    if !status.is_success() {
        return Err(AppError::BadRequest(format!(
            "语音请求失败 ({status}): {}",
            summarize_error_body(&bytes),
        )));
    }
    if bytes.is_empty() {
        return Err(AppError::BadRequest("语音服务返回了空音频".to_string()));
    }

    Ok(tauri::ipc::Response::new(bytes.to_vec()))
}

fn resolve_api_kind(api_format: Option<&str>, base_url: &str) -> Result<SpeechApiKind, AppError> {
    match api_format.map(str::trim).filter(|value| !value.is_empty()) {
        Some("openai") => Ok(SpeechApiKind::OpenAi),
        Some("fish") => Ok(SpeechApiKind::Fish),
        Some("elevenlabs") => Ok(SpeechApiKind::ElevenLabs),
        Some("azure") => Ok(SpeechApiKind::Azure),
        Some(_) => Err(AppError::BadRequest("不支持的语音接口格式".to_string())),
        None => detect_legacy_api_kind(base_url),
    }
}

fn detect_legacy_api_kind(base_url: &str) -> Result<SpeechApiKind, AppError> {
    let url = parse_base_url(base_url)?;
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let path = url.path().trim_end_matches('/').to_ascii_lowercase();
    if host == "fish.audio" || host.ends_with(".fish.audio") || path.ends_with("/v1/tts") {
        Ok(SpeechApiKind::Fish)
    } else if host == "elevenlabs.io"
        || host.ends_with(".elevenlabs.io")
        || path.contains("/v1/text-to-speech")
    {
        Ok(SpeechApiKind::ElevenLabs)
    } else if host.ends_with(".azure.com")
        || host.ends_with(".speech.microsoft.com")
        || path.ends_with("/cognitiveservices/v1")
    {
        Ok(SpeechApiKind::Azure)
    } else {
        Ok(SpeechApiKind::OpenAi)
    }
}

fn validate_audio_format(kind: SpeechApiKind, format: &str) -> Result<(), AppError> {
    let supported = match kind {
        SpeechApiKind::OpenAi => matches!(format, "mp3" | "wav" | "opus" | "flac" | "pcm"),
        SpeechApiKind::Fish => matches!(format, "mp3" | "wav" | "opus" | "pcm"),
        SpeechApiKind::ElevenLabs => matches!(format, "mp3" | "opus"),
        SpeechApiKind::Azure => matches!(format, "mp3" | "wav" | "opus" | "pcm"),
    };
    if supported {
        Ok(())
    } else {
        Err(AppError::BadRequest(format!(
            "所选接口格式不支持 {format} 音频格式"
        )))
    }
}

fn build_speech_endpoint(
    base_url: &str,
    kind: SpeechApiKind,
    voice: &str,
    format: &str,
) -> Result<Url, AppError> {
    let mut url = parse_base_url(base_url)?;
    let path = url.path().trim_end_matches('/');
    let lower = path.to_ascii_lowercase();
    let endpoint_path = match kind {
        SpeechApiKind::Fish if lower.ends_with("/v1/tts") => path.to_string(),
        SpeechApiKind::Fish if lower.ends_with("/v1") => format!("{path}/tts"),
        SpeechApiKind::Fish => format!("{path}/v1/tts"),
        SpeechApiKind::OpenAi if lower.ends_with("/v1/audio/speech") => path.to_string(),
        SpeechApiKind::OpenAi if lower.ends_with("/v1") => format!("{path}/audio/speech"),
        SpeechApiKind::OpenAi => format!("{path}/v1/audio/speech"),
        SpeechApiKind::ElevenLabs if lower.contains("/v1/text-to-speech/") => {
            path.to_string()
        }
        SpeechApiKind::ElevenLabs => {
            let voice = voice.trim();
            if voice.is_empty() {
                return Err(AppError::BadRequest("请填写语音音色".to_string()));
            }
            let voice = urlencoding::encode(voice);
            if lower.ends_with("/v1/text-to-speech") {
                format!("{path}/{voice}")
            } else if lower.ends_with("/v1") {
                format!("{path}/text-to-speech/{voice}")
            } else {
                format!("{path}/v1/text-to-speech/{voice}")
            }
        }
        SpeechApiKind::Azure if lower.ends_with("/cognitiveservices/v1") => path.to_string(),
        SpeechApiKind::Azure => format!("{path}/cognitiveservices/v1"),
    };
    url.set_path(&endpoint_path);
    url.set_query(None);
    url.set_fragment(None);
    if kind == SpeechApiKind::ElevenLabs {
        let output_format = if format == "opus" {
            "opus_48000_128"
        } else {
            "mp3_44100_128"
        };
        url.query_pairs_mut()
            .append_pair("output_format", output_format);
    }
    Ok(url)
}

fn parse_base_url(base_url: &str) -> Result<Url, AppError> {
    let url = Url::parse(base_url.trim())
        .map_err(|_| AppError::BadRequest("语音服务地址无效".to_string()))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(AppError::BadRequest(
            "语音服务地址仅支持 HTTP 或 HTTPS".to_string(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::BadRequest(
            "语音服务地址不能包含账号或密码".to_string(),
        ));
    }
    Ok(url)
}

fn build_proxy_client(proxy_url: Option<&str>) -> Result<Option<reqwest::Client>, AppError> {
    let Some(proxy_url) = proxy_url.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let proxy = Url::parse(proxy_url)
        .map_err(|_| AppError::BadRequest("HTTP 代理地址无效".to_string()))?;
    if !matches!(proxy.scheme(), "http" | "https") || proxy.host_str().is_none() {
        return Err(AppError::BadRequest(
            "HTTP 代理地址仅支持 HTTP 或 HTTPS".to_string(),
        ));
    }
    let proxy = reqwest::Proxy::all(proxy_url)
        .map_err(|error| AppError::BadRequest(format!("HTTP 代理配置无效: {error}")))?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .cookie_store(true)
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120.0.0.0 Safari/537.36")
        .proxy(proxy)
        .build()
        .map_err(|error| AppError::BadRequest(format!("创建 HTTP 代理连接失败: {error}")))?;
    Ok(Some(client))
}

fn azure_output_format(format: &str) -> Option<&'static str> {
    match format {
        "mp3" => Some("audio-24khz-96kbitrate-mono-mp3"),
        "wav" => Some("riff-24khz-16bit-mono-pcm"),
        "opus" => Some("ogg-24khz-16bit-mono-opus"),
        "pcm" => Some("raw-24khz-16bit-mono-pcm"),
        _ => None,
    }
}

fn build_azure_ssml(input: &str, language: &str, voice: &str, speed: Option<f64>) -> String {
    let rate = (((speed.unwrap_or(1.0) - 1.0) * 100.0).round() as i32).clamp(-50, 200);
    let rate_prefix = if rate >= 0 { "+" } else { "" };
    format!(
        "<speak version=\"1.0\" xml:lang=\"{}\"><voice name=\"{}\"><prosody rate=\"{rate_prefix}{rate}%\">{}</prosody></voice></speak>",
        escape_xml(language),
        escape_xml(voice),
        escape_xml(input),
    )
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn summarize_error_body(bytes: &[u8]) -> String {
    if let Ok(value) = serde_json::from_slice::<Value>(bytes) {
        for candidate in [
            value.pointer("/error/message"),
            value.get("detail"),
            value.get("message"),
            value.get("error"),
        ] {
            if let Some(candidate) = candidate {
                let text = candidate
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| candidate.to_string());
                if !text.trim().is_empty() {
                    return truncate_message(&text);
                }
            }
        }
    }

    let text = String::from_utf8_lossy(bytes);
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        "服务未返回错误详情".to_string()
    } else {
        truncate_message(&collapsed)
    }
}

fn truncate_message(message: &str) -> String {
    const MAX_CHARS: usize = 300;
    let mut chars = message.chars();
    let truncated = chars.by_ref().take(MAX_CHARS).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_each_supported_endpoint() {
        let fish = build_speech_endpoint(
            "https://api.fish.audio",
            SpeechApiKind::Fish,
            "voice",
            "mp3",
        )
        .unwrap();
        assert_eq!(fish.as_str(), "https://api.fish.audio/v1/tts");

        let openai = build_speech_endpoint(
            "https://api.openai.com/v1",
            SpeechApiKind::OpenAi,
            "alloy",
            "mp3",
        )
        .unwrap();
        assert_eq!(openai.as_str(), "https://api.openai.com/v1/audio/speech");

        let eleven = build_speech_endpoint(
            "https://api.elevenlabs.io",
            SpeechApiKind::ElevenLabs,
            "voice/id",
            "opus",
        )
        .unwrap();
        assert_eq!(
            eleven.as_str(),
            "https://api.elevenlabs.io/v1/text-to-speech/voice%2Fid?output_format=opus_48000_128"
        );

        let azure = build_speech_endpoint(
            "https://reader.cognitiveservices.azure.com",
            SpeechApiKind::Azure,
            "zh-CN-XiaoxiaoNeural",
            "mp3",
        )
        .unwrap();
        assert_eq!(
            azure.as_str(),
            "https://reader.cognitiveservices.azure.com/cognitiveservices/v1"
        );
    }

    #[test]
    fn explicit_format_does_not_depend_on_the_host() {
        assert_eq!(
            resolve_api_kind(Some("fish"), "https://proxy.example.com").unwrap(),
            SpeechApiKind::Fish,
        );
        assert_eq!(
            resolve_api_kind(Some("openai"), "https://api.fish.audio").unwrap(),
            SpeechApiKind::OpenAi,
        );
    }

    #[test]
    fn escapes_azure_ssml_values() {
        let ssml = build_azure_ssml("甲<&乙", "zh-CN", "voice\"name", Some(1.2));
        assert!(ssml.contains("甲&lt;&amp;乙"));
        assert!(ssml.contains("voice&quot;name"));
        assert!(ssml.contains("rate=\"+20%\""));
    }

    #[test]
    fn validates_optional_http_proxy() {
        assert!(build_proxy_client(None).unwrap().is_none());
        assert!(build_proxy_client(Some("http://127.0.0.1:7890"))
            .unwrap()
            .is_some());
        assert!(build_proxy_client(Some("socks5://127.0.0.1:7890")).is_err());
    }
}
