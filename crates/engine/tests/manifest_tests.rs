use engine::manifest::ManifestStore;
use types::{ByteRange, DownloadStatus, JobRecord};

#[test]
fn test_manifest_job_crud_and_persistence() {
    let temp_dir = std::env::temp_dir().join(format!("otter_test_manifest_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let _ = std::fs::create_dir_all(&temp_dir);
    let db_path = temp_dir.join("test_manifest.db");

    let store = ManifestStore::open(&db_path).expect("Failed to open test manifest store");

    let job = JobRecord {
        id: "job_test_001".to_string(),
        url: "https://example.com/data.bin".to_string(),
        destination: temp_dir.join("data.bin").to_string_lossy().to_string(),
        filename: "data.bin".to_string(),
        total_size: Some(1048576),
        downloaded_bytes: 524288,
        status: DownloadStatus::Paused,
        resumable: true,
        etag: Some("\"xyz-123\"".to_string()),
        last_modified: None,
        expected_sha256: Some("abc123hash".to_string()),
        computed_sha256: None,
        error: None,
        created_at: 1000,
        completed_at: None,
    };

    store.upsert_job(&job).expect("Failed to insert job");

    // Retrieve and verify
    let loaded = store.get_job("job_test_001").expect("Failed to get job");
    assert!(loaded.is_some());
    let loaded = loaded.unwrap();
    assert_eq!(loaded.id, "job_test_001");
    assert_eq!(loaded.total_size, Some(1048576));
    assert_eq!(loaded.downloaded_bytes, 524288);
    assert_eq!(loaded.status, DownloadStatus::Paused);
    assert_eq!(loaded.etag, Some("\"xyz-123\"".to_string()));

    // Insert and update completed ranges
    store
        .add_completed_range("job_test_001", 0, 524287)
        .expect("Failed to add range 1");
    store
        .add_completed_range("job_test_001", 524288, 786431)
        .expect("Failed to add range 2");

    let loaded_ranges = store
        .get_completed_ranges("job_test_001")
        .expect("Failed to load ranges");
    // Adjacent ranges 0..524287 and 524288..786431 are merged into a single continuous range
    assert_eq!(loaded_ranges.len(), 1);
    assert_eq!(loaded_ranges[0], ByteRange::new(0, 786431));

    // Check missing range calculation
    let missing = store
        .get_missing_ranges("job_test_001", 1048576)
        .expect("Failed to calculate missing ranges");
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0], ByteRange::new(786432, 1048575));

    // Update job status to Completed
    store
        .update_status("job_test_001", DownloadStatus::Completed, None)
        .expect("Failed to update status");
    let updated = store
        .get_job("job_test_001")
        .expect("Failed to get job")
        .unwrap();
    assert_eq!(updated.status, DownloadStatus::Completed);

    let _ = std::fs::remove_dir_all(&temp_dir);
}
