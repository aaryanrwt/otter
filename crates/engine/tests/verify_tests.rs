use engine::verify::{compute_file_sha256, verify_file_integrity};
use std::io::Write;

#[test]
fn test_streaming_sha256_verification() {
    let temp_dir = std::env::temp_dir().join(format!("otter_test_verify_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let file_path = temp_dir.join("test_file.bin");

    // Write deterministic content: 540KB of patterned bytes
    let pattern = b"OTTER Reliable Download Engine Verification Test 2026\n";
    {
        let mut file = std::fs::File::create(&file_path).unwrap();
        for _ in 0..10_000 {
            file.write_all(pattern).unwrap();
        }
    }

    // Compute hash
    let hash = compute_file_sha256(&file_path).expect("Hash computation failed");
    assert_eq!(hash.len(), 64, "SHA-256 hex string must be 64 characters");

    // Exact verification succeeds
    let result = verify_file_integrity(&file_path, &hash).expect("Verification call failed");
    assert!(result, "Expected verification to succeed on identical hash");

    // Case-insensitive verification succeeds
    let upper_hash = hash.to_uppercase();
    let result_upper =
        verify_file_integrity(&file_path, &upper_hash).expect("Verification call failed");
    assert!(
        result_upper,
        "Expected verification to succeed with uppercase hash"
    );

    // Corrupted hash fails
    let bad_hash = "0".repeat(64);
    let result_bad =
        verify_file_integrity(&file_path, &bad_hash).expect("Verification call failed");
    assert!(
        !result_bad,
        "Expected verification to fail on mismatched hash"
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}
