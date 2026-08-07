use reader_rust::model::ai_book::{
    AiBookCharacter, AiBookLocation, AiBookMap, AiBookMemory, AiBookNote, AiBookRelationship,
};
use reader_rust::service::ai_book_service::AiBookService;
use reader_rust::storage::db;

fn temp_storage_dir(name: &str) -> String {
    let path = std::env::temp_dir().join(format!(
        "reader-rust-ai-book-test-{}-{}",
        name,
        std::process::id()
    ));
    if path.exists() {
        std::fs::remove_dir_all(&path).unwrap();
    }
    std::fs::create_dir_all(&path).unwrap();
    path.to_string_lossy().to_string()
}

async fn create_service(name: &str) -> (AiBookService, String) {
    let storage_dir = temp_storage_dir(name);
    let database_url = format!(
        "sqlite:{}?mode=rwc",
        std::path::Path::new(&storage_dir)
            .join("reader.db")
            .display()
    );
    let pool = db::init_pool(&database_url).await.unwrap();
    (AiBookService::new(pool, &storage_dir), storage_dir)
}

#[tokio::test]
async fn ai_book_memory_round_trips_and_isolated_by_user() {
    let (service, storage_dir) = create_service("round-trip").await;
    let memory = AiBookMemory {
        book_url: "https://example.test/book/1".to_string(),
        book_name: Some("山海旧事".to_string()),
        enabled: true,
        processed_chapter_index: Some(7),
        summary: "主角抵达北境，首次听闻旧神传说。".to_string(),
        worldview: vec![AiBookNote {
            title: "旧神信仰".to_string(),
            content: "北境仍保留旧神祭仪，但真伪未知。".to_string(),
            category: Some("历史传说".to_string()),
            confidence: Some("推断".to_string()),
            importance: Some("high".to_string()),
        }],
        characters: vec![AiBookCharacter {
            name: "林舟".to_string(),
            aliases: vec!["阿舟".to_string()],
            status: "抵达北境".to_string(),
            importance: Some("high".to_string()),
            ..AiBookCharacter::default()
        }],
        relationships: vec![AiBookRelationship {
            source: "林舟".to_string(),
            target: "沈月".to_string(),
            relation: "同伴".to_string(),
            importance: Some("medium".to_string()),
            ..AiBookRelationship::default()
        }],
        locations: vec![AiBookLocation {
            name: "北境边城".to_string(),
            kind: Some("城市".to_string()),
            parent_name: Some("北境".to_string()),
            description: "北境边缘的要塞城市。".to_string(),
            importance: Some("high".to_string()),
            ..AiBookLocation::default()
        }],
        map: Some(AiBookMap {
            image_url: Some("/assets/alice/ai-maps/map.png".to_string()),
            prompt: Some("ink fantasy map of northern border".to_string()),
            updated_at: Some(1_700_000_000),
            source_chapter_index: Some(7),
            fallback: Some("relationship-graph".to_string()),
            fallback_reason: Some("图片模型未配置".to_string()),
        }),
        ..AiBookMemory::default()
    };

    service
        .save_for_book("alice", "https://example.test/book/1", memory.clone())
        .await
        .unwrap();

    let saved = service
        .get("alice", "https://example.test/book/1")
        .await
        .unwrap()
        .expect("alice should have memory");
    assert_eq!(saved.book_name.as_deref(), Some("山海旧事"));
    assert_eq!(saved.processed_chapter_index, Some(7));
    assert_eq!(saved.worldview[0].title, "旧神信仰");
    assert_eq!(saved.worldview[0].category.as_deref(), Some("历史传说"));
    assert_eq!(saved.worldview[0].importance.as_deref(), Some("high"));
    assert_eq!(saved.characters[0].importance.as_deref(), Some("high"));
    assert_eq!(saved.relationships[0].importance.as_deref(), Some("medium"));
    assert_eq!(saved.locations[0].parent_name.as_deref(), Some("北境"));
    assert_eq!(saved.locations[0].importance.as_deref(), Some("high"));
    assert_eq!(
        saved.map,
        Some(AiBookMap {
            image_url: Some("/assets/alice/ai-maps/map.png".to_string()),
            prompt: Some("ink fantasy map of northern border".to_string()),
            updated_at: Some(1_700_000_000),
            source_chapter_index: Some(7),
            fallback: Some("relationship-graph".to_string()),
            fallback_reason: Some("图片模型未配置".to_string()),
        })
    );

    let bob = service
        .get("bob", "https://example.test/book/1")
        .await
        .unwrap();
    assert!(bob.is_none(), "memory must be isolated by user namespace");

    assert!(service
        .delete("alice", "https://example.test/book/1")
        .await
        .unwrap());
    assert!(service
        .get("alice", "https://example.test/book/1")
        .await
        .unwrap()
        .is_none());

    let _ = std::fs::remove_dir_all(storage_dir);
}

#[tokio::test]
async fn ai_book_memory_rejects_mismatched_book_url_on_save() {
    let (service, storage_dir) = create_service("mismatch").await;

    let mut memory = AiBookMemory::default();
    memory.book_url = "https://example.test/book/1".to_string();

    let err = service
        .save_for_book("alice", "https://example.test/book/2", memory)
        .await
        .expect_err("mismatched bookUrl should fail");

    assert!(err.to_string().contains("bookUrl mismatch"));
    let _ = std::fs::remove_dir_all(storage_dir);
}
