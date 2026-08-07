use reader_rust::model::ai_proxy::{
    ai_proxy_timeout, build_ai_proxy_url, format_ai_proxy_upstream_error,
    validate_ai_proxy_image_url,
};

#[test]
fn ai_proxy_url_allows_only_openai_compatible_paths() {
    let url =
        build_ai_proxy_url("https://api.example.test/", "/v1/chat/completions", false).unwrap();
    assert_eq!(url.as_str(), "https://api.example.test/v1/chat/completions");

    let speech_url =
        build_ai_proxy_url("https://api.example.test/", "/v1/audio/speech", false).unwrap();
    assert_eq!(
        speech_url.as_str(),
        "https://api.example.test/v1/audio/speech"
    );

    let err = build_ai_proxy_url("https://api.example.test/", "/v1/models", false).unwrap_err();
    assert!(err.contains("unsupported proxy path"));
}

#[test]
fn ai_proxy_url_can_use_full_model_endpoint_without_appending_path() {
    let url = build_ai_proxy_url(
        "https://gateway.example.test/custom/chat?deployment=reader",
        "/v1/chat/completions",
        true,
    )
    .unwrap();

    assert_eq!(
        url.as_str(),
        "https://gateway.example.test/custom/chat?deployment=reader"
    );
}

#[test]
fn ai_proxy_url_rejects_non_http_targets() {
    let err = build_ai_proxy_url("file:///tmp/secret", "/v1/chat/completions", false).unwrap_err();
    assert!(err.contains("http"));

    let image_err = validate_ai_proxy_image_url("data:image/png;base64,abc").unwrap_err();
    assert!(image_err.contains("http"));
}

#[test]
fn ai_proxy_uses_model_sized_timeout() {
    assert!(ai_proxy_timeout().as_secs() >= 60);
}

#[test]
fn ai_proxy_formats_upstream_method_errors() {
    let message = format_ai_proxy_upstream_error(
        405,
        "<html><body><h1>Method Not Allowed</h1></body></html>",
    );

    assert!(message.contains("模型服务返回 405"));
    assert!(message.contains("Method Not Allowed"));
}
