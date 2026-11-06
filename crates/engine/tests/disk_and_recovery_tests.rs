use engine::disk::{FileWriter, get_available_disk_space};
use engine::retry::{ErrorClassification, RetryPolicy};
use engine::segment::SegmentTable;
use engine::verify::compute_file_sha256;
use reqwest::StatusCode;
use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
use std::io::Write;
use types::ByteRange;

#[test]
fn test_disk_space_query_and_safeguard() {
    let temp_dir = std::env::temp_dir();
    let free = get_available_disk_space(&temp_dir);
    assert!(free.is_some(), "Disk space query should succeed on Windows");
    let free_bytes = free.unwrap();
    assert!(free_bytes > 0, "Free bytes must be > 0");

    // Preallocating reasonable size should succeed
    let test_file = temp_dir.join(format!("otter_disk_test_{}.bin", std::process::id()));
    let writer =
        FileWriter::open_or_create(&test_file, Some(1024 * 1024)).expect("Should preallocate 1MB");
    writer.write_at(0, b"TEST HEADER").expect("Write at 0");
    writer
        .write_at(1024 * 512, b"MID CHUNK")
        .expect("Write at 512KB");
    writer.sync_data().expect("Sync data");
    drop(writer);

    let _ = std::fs::remove_file(&test_file);
    let mut part = test_file.clone();
    part.set_file_name(format!(
        "{}.part",
        test_file.file_name().unwrap().to_string_lossy()
    ));
    let _ = std::fs::remove_file(&part);
}

#[test]
fn test_tail_starvation_work_stealing_active() {
    let total_size = 100 * 1024 * 1024;
    // 96 MB already downloaded, 4 MB remaining in range [96MB, 100MB]
    let missing = vec![ByteRange::new(96 * 1024 * 1024, total_size - 1)];
    let mut table = SegmentTable::new(total_size, &missing, 2);

    // Initial claim takes segment 0 and 1
    let claim1 = table.claim();
    assert!(claim1.is_some());
    let claim2 = table.claim();
    assert!(claim2.is_some());

    // Both initial segments are now active.
    // Worker 1 finishes its segment, so it calls claim() to steal from Worker 2.
    // Worker 2 still has ~2 MB remaining.
    // Since 2 MB >= 2 * MIN_SPLIT_HALF (1 MB), work-stealing MUST split Worker 2's segment.
    let steal_claim = table.claim();
    assert!(
        steal_claim.is_some(),
        "Work-stealing must succeed on remaining 2MB tail"
    );
    let (_stolen_id, stolen_range) = steal_claim.unwrap();
    assert_eq!(
        stolen_range.len(),
        1024 * 1024,
        "Stolen half should be exactly 1 MiB"
    );
    assert!(stolen_range.start > 96 * 1024 * 1024);
}

#[test]
fn test_error_classification_and_backoff() {
    assert_eq!(
        RetryPolicy::classify_status(StatusCode::TOO_MANY_REQUESTS),
        ErrorClassification::RateLimited
    );
    assert_eq!(
        RetryPolicy::classify_status(StatusCode::UNAUTHORIZED),
        ErrorClassification::AuthOrExpired
    );
    assert_eq!(
        RetryPolicy::classify_status(StatusCode::FORBIDDEN),
        ErrorClassification::AuthOrExpired
    );
    assert_eq!(
        RetryPolicy::classify_status(StatusCode::BAD_GATEWAY),
        ErrorClassification::TransientNetwork
    );
    assert_eq!(
        RetryPolicy::classify_status(StatusCode::SERVICE_UNAVAILABLE),
        ErrorClassification::TransientNetwork
    );

    let mut headers = HeaderMap::new();
    headers.insert(RETRY_AFTER, HeaderValue::from_static("45"));
    let dur = RetryPolicy::parse_retry_after(&headers);
    assert_eq!(dur, Some(std::time::Duration::from_secs(45)));

    let policy = RetryPolicy::default();
    let b1 = policy.calculate_backoff(1);
    let b2 = policy.calculate_backoff(2);
    assert!(b2 >= b1);
}

#[test]
fn test_sha256_buffer_streaming() {
    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join(format!("otter_sha256_test_{}.bin", std::process::id()));
    {
        let mut f = std::fs::File::create(&test_file).unwrap();
        // Write 4 MB of repeating bytes
        let chunk = vec![0xABu8; 1024 * 1024];
        for _ in 0..4 {
            f.write_all(&chunk).unwrap();
        }
    }

    let hash = compute_file_sha256(&test_file).expect("Compute sha256");
    assert_eq!(hash.len(), 64);
    let _ = std::fs::remove_file(&test_file);
}
