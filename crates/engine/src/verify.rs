use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

pub fn compute_file_sha256(path: impl AsRef<Path>) -> Result<String> {
    let file = File::open(path.as_ref()).with_context(|| {
        format!(
            "Failed to open file for SHA-256: {}",
            path.as_ref().display()
        )
    })?;
    let mut reader = BufReader::with_capacity(2 * 1024 * 1024, file); // 2 MiB buffer
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024]; // 1 MiB chunk read buffer

    loop {
        let n = reader
            .read(&mut buf)
            .context("Error reading file during SHA-256 hashing")?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    let result = hasher.finalize();
    Ok(hex::encode(result))
}

pub fn verify_file_integrity(path: impl AsRef<Path>, expected_hex: &str) -> Result<bool> {
    let computed = compute_file_sha256(path)?;
    let clean_expected = expected_hex.trim().to_lowercase();
    let clean_computed = computed.trim().to_lowercase();
    Ok(clean_expected == clean_computed)
}
