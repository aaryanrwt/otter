use engine::{DownloadManager, ManifestStore};
use harness::{LoopbackServer, ServerProfile};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use types::{DownloadStatus, JobRecord};

fn generate_payload(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 251) as u8).collect()
}

fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

#[tokio::test]
async fn test_profile_fast_stable() {
    let payload = generate_payload(1024 * 1024); // 1 MB
    let expected_hash = compute_sha256(&payload);

    let server = LoopbackServer::start(ServerProfile::FastStable, payload)
        .await
        .expect("Server must start");

    let temp_dir = std::env::temp_dir().join(format!(
        "otter_test_harness_fast_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&temp_dir);
    let dest_file = temp_dir.join("fast.bin");

    let db_path = temp_dir.join("manifest.db");
    let manifest = Arc::new(ManifestStore::open(&db_path).unwrap());
    let manager = Arc::new(DownloadManager::new(manifest.clone()).unwrap());

    let job = JobRecord {
        id: "job_fast_stable".to_string(),
        url: server.url(),
        filename: "fast.bin".to_string(),
        destination: dest_file.to_string_lossy().to_string(),
        total_size: None,
        downloaded_bytes: 0,
        status: DownloadStatus::Queued,
        resumable: true,
        etag: None,
        last_modified: None,
        expected_sha256: Some(expected_hash.clone()),
        computed_sha256: None,
        error: None,
        created_at: 0,
        completed_at: None,
    };
    manifest.upsert_job(&job).unwrap();

    let status = manager
        .run_job(&job.id, 4, CancellationToken::new())
        .await
        .expect("Download run_job");
    assert_eq!(status, DownloadStatus::Completed);
    assert!(dest_file.exists());
    assert_eq!(std::fs::metadata(&dest_file).unwrap().len(), 1024 * 1024);

    let final_data = std::fs::read(&dest_file).unwrap();
    assert_eq!(compute_sha256(&final_data), expected_hash);

    server.stop();
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_profile_no_range() {
    let payload = generate_payload(512 * 1024); // 512 KB
    let expected_hash = compute_sha256(&payload);

    let server = LoopbackServer::start(ServerProfile::NoRange, payload)
        .await
        .expect("Server must start");

    let temp_dir = std::env::temp_dir().join(format!(
        "otter_test_harness_norange_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&temp_dir);
    let dest_file = temp_dir.join("norange.bin");

    let db_path = temp_dir.join("manifest.db");
    let manifest = Arc::new(ManifestStore::open(&db_path).unwrap());
    let manager = Arc::new(DownloadManager::new(manifest.clone()).unwrap());

    let job = JobRecord {
        id: "job_norange".to_string(),
        url: server.url(),
        filename: "norange.bin".to_string(),
        destination: dest_file.to_string_lossy().to_string(),
        total_size: None,
        downloaded_bytes: 0,
        status: DownloadStatus::Queued,
        resumable: false,
        etag: None,
        last_modified: None,
        expected_sha256: Some(expected_hash.clone()),
        computed_sha256: None,
        error: None,
        created_at: 0,
        completed_at: None,
    };
    manifest.upsert_job(&job).unwrap();

    let status = manager
        .run_job(&job.id, 1, CancellationToken::new())
        .await
        .expect("Download run_job");
    assert_eq!(status, DownloadStatus::Completed);
    assert_eq!(std::fs::metadata(&dest_file).unwrap().len(), 512 * 1024);

    let final_data = std::fs::read(&dest_file).unwrap();
    assert_eq!(compute_sha256(&final_data), expected_hash);

    server.stop();
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_profile_rate_limited_429() {
    let payload = generate_payload(1024 * 1024); // 1 MB
    let expected_hash = compute_sha256(&payload);

    // Limit server to max 2 concurrent connections; >2 triggers HTTP 429
    let server = LoopbackServer::start(
        ServerProfile::RateLimited429 {
            max_conns: 2,
            retry_after_secs: 1,
        },
        payload,
    )
    .await
    .expect("Server must start");

    let temp_dir = std::env::temp_dir().join(format!(
        "otter_test_harness_429_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&temp_dir);
    let dest_file = temp_dir.join("rate_limited.bin");

    let db_path = temp_dir.join("manifest.db");
    let manifest = Arc::new(ManifestStore::open(&db_path).unwrap());
    let manager = Arc::new(DownloadManager::new(manifest.clone()).unwrap());

    let job = JobRecord {
        id: "job_rate_limited".to_string(),
        url: server.url(),
        filename: "rate_limited.bin".to_string(),
        destination: dest_file.to_string_lossy().to_string(),
        total_size: None,
        downloaded_bytes: 0,
        status: DownloadStatus::Queued,
        resumable: true,
        etag: None,
        last_modified: None,
        expected_sha256: Some(expected_hash.clone()),
        computed_sha256: None,
        error: None,
        created_at: 0,
        completed_at: None,
    };
    manifest.upsert_job(&job).unwrap();

    // Start with 6 connections; AIMD should detect 429s and throttle down
    let status = manager
        .run_job(&job.id, 6, CancellationToken::new())
        .await
        .expect("Download run_job");
    assert_eq!(status, DownloadStatus::Completed);
    assert_eq!(std::fs::metadata(&dest_file).unwrap().len(), 1024 * 1024);

    let final_data = std::fs::read(&dest_file).unwrap();
    assert_eq!(compute_sha256(&final_data), expected_hash);

    server.stop();
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_profile_transient_5xx() {
    let payload = generate_payload(1024 * 1024); // 1 MB
    let expected_hash = compute_sha256(&payload);

    // Injects 503 error every 4th request
    let server = LoopbackServer::start(ServerProfile::Transient5xx { fail_every_n: 4 }, payload)
        .await
        .expect("Server must start");

    let temp_dir = std::env::temp_dir().join(format!(
        "otter_test_harness_5xx_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&temp_dir);
    let dest_file = temp_dir.join("transient_5xx.bin");

    let db_path = temp_dir.join("manifest.db");
    let manifest = Arc::new(ManifestStore::open(&db_path).unwrap());
    let manager = Arc::new(DownloadManager::new(manifest.clone()).unwrap());

    let job = JobRecord {
        id: "job_transient_5xx".to_string(),
        url: server.url(),
        filename: "transient_5xx.bin".to_string(),
        destination: dest_file.to_string_lossy().to_string(),
        total_size: None,
        downloaded_bytes: 0,
        status: DownloadStatus::Queued,
        resumable: true,
        etag: None,
        last_modified: None,
        expected_sha256: Some(expected_hash.clone()),
        computed_sha256: None,
        error: None,
        created_at: 0,
        completed_at: None,
    };
    manifest.upsert_job(&job).unwrap();

    let status = manager
        .run_job(&job.id, 4, CancellationToken::new())
        .await
        .expect("Download run_job");
    assert_eq!(status, DownloadStatus::Completed);
    assert_eq!(std::fs::metadata(&dest_file).unwrap().len(), 1024 * 1024);

    let final_data = std::fs::read(&dest_file).unwrap();
    assert_eq!(compute_sha256(&final_data), expected_hash);

    server.stop();
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_profile_expired_auth_and_url_refresh() {
    let payload = generate_payload(512 * 1024);
    let expected_hash = compute_sha256(&payload);

    let server = LoopbackServer::start(
        ServerProfile::ExpiredAuth {
            valid_token: "secret123".to_string(),
        },
        payload,
    )
    .await
    .expect("Server must start");

    let temp_dir = std::env::temp_dir().join(format!(
        "otter_test_harness_auth_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&temp_dir);
    let dest_file = temp_dir.join("auth.bin");

    let db_path = temp_dir.join("manifest.db");
    let manifest = Arc::new(ManifestStore::open(&db_path).unwrap());
    let manager = Arc::new(DownloadManager::new(manifest.clone()).unwrap());

    // 1. Initial attempt with expired token: probe returns 401 Unauthorized -> status WaitingAuth
    let invalid_url = format!("{}?token=expired", server.url());
    let job = JobRecord {
        id: "job_auth_refresh".to_string(),
        url: invalid_url,
        filename: "auth.bin".to_string(),
        destination: dest_file.to_string_lossy().to_string(),
        total_size: None,
        downloaded_bytes: 0,
        status: DownloadStatus::Queued,
        resumable: true,
        etag: None,
        last_modified: None,
        expected_sha256: Some(expected_hash.clone()),
        computed_sha256: None,
        error: None,
        created_at: 0,
        completed_at: None,
    };
    manifest.upsert_job(&job).unwrap();

    let fail_result = manager.run_job(&job.id, 2, CancellationToken::new()).await;
    assert!(fail_result.is_err());
    let stored_job = manifest.get_job(&job.id).unwrap().unwrap();
    assert_eq!(stored_job.status, DownloadStatus::WaitingAuth);

    // 2. Refresh URL with valid token and resume
    let refreshed_url = format!("{}?token=secret123", server.url());
    manifest.update_job_url(&job.id, &refreshed_url).unwrap();

    let success = manager
        .run_job(&job.id, 2, CancellationToken::new())
        .await
        .expect("Resumed job must succeed");
    assert_eq!(success, DownloadStatus::Completed);
    assert_eq!(std::fs::metadata(&dest_file).unwrap().len(), 512 * 1024);

    let final_data = std::fs::read(&dest_file).unwrap();
    assert_eq!(compute_sha256(&final_data), expected_hash);

    server.stop();
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_profile_chunked_unknown() {
    let payload = generate_payload(256 * 1024);
    let expected_hash = compute_sha256(&payload);

    let server = LoopbackServer::start(ServerProfile::ChunkedUnknown, payload)
        .await
        .expect("Server must start");

    let temp_dir = std::env::temp_dir().join(format!(
        "otter_test_harness_chunked_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&temp_dir);
    let dest_file = temp_dir.join("chunked.bin");

    let db_path = temp_dir.join("manifest.db");
    let manifest = Arc::new(ManifestStore::open(&db_path).unwrap());
    let manager = Arc::new(DownloadManager::new(manifest.clone()).unwrap());

    let job = JobRecord {
        id: "job_chunked".to_string(),
        url: server.url(),
        filename: "chunked.bin".to_string(),
        destination: dest_file.to_string_lossy().to_string(),
        total_size: None,
        downloaded_bytes: 0,
        status: DownloadStatus::Queued,
        resumable: false,
        etag: None,
        last_modified: None,
        expected_sha256: Some(expected_hash.clone()),
        computed_sha256: None,
        error: None,
        created_at: 0,
        completed_at: None,
    };
    manifest.upsert_job(&job).unwrap();

    let status = manager
        .run_job(&job.id, 1, CancellationToken::new())
        .await
        .expect("Download run_job");
    assert_eq!(status, DownloadStatus::Completed);
    assert_eq!(std::fs::metadata(&dest_file).unwrap().len(), 256 * 1024);

    let final_data = std::fs::read(&dest_file).unwrap();
    assert_eq!(compute_sha256(&final_data), expected_hash);

    server.stop();
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_profile_throttled_per_conn() {
    let payload = generate_payload(512 * 1024); // 512 KB
    let expected_hash = compute_sha256(&payload);

    // Throttled to 1024 KB/s
    let server = LoopbackServer::start(ServerProfile::ThrottledPerConn { kbps: 1024 }, payload)
        .await
        .expect("Server must start");

    let temp_dir = std::env::temp_dir().join(format!(
        "otter_test_harness_throttled_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&temp_dir);
    let dest_file = temp_dir.join("throttled.bin");

    let db_path = temp_dir.join("manifest.db");
    let manifest = Arc::new(ManifestStore::open(&db_path).unwrap());
    let manager = Arc::new(DownloadManager::new(manifest.clone()).unwrap());

    let job = JobRecord {
        id: "job_throttled".to_string(),
        url: server.url(),
        filename: "throttled.bin".to_string(),
        destination: dest_file.to_string_lossy().to_string(),
        total_size: None,
        downloaded_bytes: 0,
        status: DownloadStatus::Queued,
        resumable: true,
        etag: None,
        last_modified: None,
        expected_sha256: Some(expected_hash.clone()),
        computed_sha256: None,
        error: None,
        created_at: 0,
        completed_at: None,
    };
    manifest.upsert_job(&job).unwrap();

    let status = manager
        .run_job(&job.id, 4, CancellationToken::new())
        .await
        .expect("Download run_job");
    assert_eq!(status, DownloadStatus::Completed);
    assert_eq!(std::fs::metadata(&dest_file).unwrap().len(), 512 * 1024);

    let final_data = std::fs::read(&dest_file).unwrap();
    assert_eq!(compute_sha256(&final_data), expected_hash);

    server.stop();
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_profile_changed_resource_never_splice() {
    let original = generate_payload(512 * 1024);
    let mutated = generate_payload(512 * 1024)
        .into_iter()
        .map(|b| b ^ 0xFF)
        .collect::<Vec<u8>>();
    let mutated_hash = compute_sha256(&mutated);

    let server = LoopbackServer::start(
        ServerProfile::ChangedResource {
            mutated_etag: "\"mutated_etag_9999\"".to_string(),
            mutated_payload: mutated,
        },
        original,
    )
    .await
    .expect("Server must start");

    let temp_dir = std::env::temp_dir().join(format!(
        "otter_test_harness_splice_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&temp_dir);
    let dest_file = temp_dir.join("splice.bin");

    let db_path = temp_dir.join("manifest.db");
    let manifest = Arc::new(ManifestStore::open(&db_path).unwrap());
    let manager = Arc::new(DownloadManager::new(manifest.clone()).unwrap());

    // Record a stale partial job with the old ETag and partial downloaded bytes
    let job = JobRecord {
        id: "job_splice".to_string(),
        url: server.url(),
        filename: "splice.bin".to_string(),
        destination: dest_file.to_string_lossy().to_string(),
        total_size: Some(512 * 1024),
        downloaded_bytes: 128 * 1024,
        status: DownloadStatus::Paused,
        resumable: true,
        etag: Some("\"old_stale_etag_1111\"".to_string()),
        last_modified: None,
        expected_sha256: Some(mutated_hash.clone()),
        computed_sha256: None,
        error: None,
        created_at: 0,
        completed_at: None,
    };
    manifest.upsert_job(&job).unwrap();
    manifest
        .add_completed_range(&job.id, 0, 128 * 1024 - 1)
        .unwrap();

    // When running, Never-Splice detects the ETag validator mismatch and discards stale partial ranges
    let status = manager
        .run_job(&job.id, 2, CancellationToken::new())
        .await
        .expect("Download run_job");
    assert_eq!(status, DownloadStatus::Completed);
    assert_eq!(std::fs::metadata(&dest_file).unwrap().len(), 512 * 1024);

    let final_data = std::fs::read(&dest_file).unwrap();
    assert_eq!(compute_sha256(&final_data), mutated_hash);

    server.stop();
    let _ = std::fs::remove_dir_all(&temp_dir);
}
