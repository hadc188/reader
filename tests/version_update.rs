use reader_rust::service::update_service::{
    build_update_info, is_newer_version, GithubRelease, UpdatePreferences,
};

#[test]
fn compares_release_tags_as_semver_versions() {
    assert!(is_newer_version("v1.0.6", "v1.0.5"));
    assert!(is_newer_version("1.10.0", "v1.9.9"));
    assert!(!is_newer_version("v1.0.5", "1.0.5"));
    assert!(!is_newer_version("v1.0.4", "v1.0.5"));
}

#[test]
fn dismissed_latest_release_remains_update_without_reminder() {
    let release = GithubRelease {
        tag_name: "v1.0.6".to_string(),
        name: Some("v1.0.6".to_string()),
        html_url: "https://github.com/givenge/reader-rust/releases/tag/v1.0.6".to_string(),
        published_at: Some("2026-05-15T08:00:00Z".to_string()),
    };
    let preferences = UpdatePreferences {
        dismissed_version: Some("v1.0.6".to_string()),
    };

    let info = build_update_info(
        "v1.0.5",
        Some(release),
        Some(preferences),
        None,
        1_778_828_800,
    );

    assert!(info.update_available);
    assert!(!info.should_remind);
    assert_eq!(info.latest_version.as_deref(), Some("v1.0.6"));
    assert_eq!(info.dismissed_version.as_deref(), Some("v1.0.6"));
}
