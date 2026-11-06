use engine::artifact::{
    ArtifactFormat, create_reproducibility_manifest, inspect_artifact,
    verify_reproducibility_manifest,
};
use engine::doctor::DoctorEngine;
use engine::ollama::parse_model_tag;
use std::io::Write;

#[test]
fn test_ollama_tag_parsing() {
    let (name, tag) = parse_model_tag("deepseek-r1:14b");
    assert_eq!(name, "deepseek-r1");
    assert_eq!(tag, "14b");

    let (name2, tag2) = parse_model_tag("library/gemma:latest");
    assert_eq!(name2, "gemma");
    assert_eq!(tag2, "latest");

    let (name3, tag3) = parse_model_tag("llama3.2");
    assert_eq!(name3, "llama3.2");
    assert_eq!(tag3, "latest");
}

fn make_test_temp_dir(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "otter_test_{}_{}",
        tag,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&p);
    p
}

#[test]
fn test_parquet_inspection_validation() {
    let dir = make_test_temp_dir("parquet");
    let parquet_path = dir.join("test_shard.parquet");

    // Write a mock valid Parquet file: PAR1 (4 bytes) + dummy body + metadata len (4 bytes) + PAR1 (4 bytes)
    let mut file = std::fs::File::create(&parquet_path).unwrap();
    file.write_all(b"PAR1").unwrap();
    file.write_all(&[0u8; 128]).unwrap();
    file.write_all(&128u32.to_le_bytes()).unwrap();
    file.write_all(b"PAR1").unwrap();
    drop(file);

    let inspection = inspect_artifact(&parquet_path).unwrap();
    assert_eq!(inspection.format, ArtifactFormat::Parquet);
    assert_eq!(inspection.file_size_bytes, 4 + 128 + 4 + 4);
    assert_eq!(inspection.metadata.get("magic_header").unwrap(), "PAR1");
    assert_eq!(inspection.metadata.get("magic_footer").unwrap(), "PAR1");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_safetensors_inspection_validation() {
    let dir = make_test_temp_dir("safetensors");
    let st_path = dir.join("model.safetensors");

    let json_header = serde_json::json!({
        "__metadata__": {
            "architecture": "LlamaForCausalLM",
            "model_type": "llama"
        },
        "model.layers.0.weight": {
            "dtype": "F16",
            "shape": [4096, 4096],
            "data_offsets": [0, 33554432]
        },
        "model.layers.1.weight": {
            "dtype": "F16",
            "shape": [4096, 4096],
            "data_offsets": [33554432, 67108864]
        }
    });

    let header_bytes = serde_json::to_vec(&json_header).unwrap();
    let header_len = header_bytes.len() as u64;

    let mut file = std::fs::File::create(&st_path).unwrap();
    file.write_all(&header_len.to_le_bytes()).unwrap();
    file.write_all(&header_bytes).unwrap();
    file.write_all(&[0u8; 1024]).unwrap(); // Dummy tensor payload
    drop(file);

    let inspection = inspect_artifact(&st_path).unwrap();
    assert_eq!(inspection.format, ArtifactFormat::Safetensors);
    assert_eq!(inspection.tensor_count, Some(2));
    assert_eq!(inspection.architecture.as_deref(), Some("LlamaForCausalLM"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_reproducibility_manifest_lifecycle() {
    let dir = make_test_temp_dir("manifest");
    let sub = dir.join("shards");
    std::fs::create_dir_all(&sub).unwrap();

    let f1 = sub.join("shard_0.bin");
    let f2 = sub.join("shard_1.bin");
    std::fs::write(&f1, b"sample_shard_zero_content").unwrap();
    std::fs::write(&f2, b"sample_shard_one_content").unwrap();

    let manifest = create_reproducibility_manifest(&dir).unwrap();
    assert_eq!(manifest.total_files, 2);
    assert_eq!(manifest.files.len(), 2);

    let manifest_path = dir.join("manifest.json");
    let manifest_bytes = serde_json::to_vec_pretty(&manifest).unwrap();
    std::fs::write(&manifest_path, manifest_bytes).unwrap();

    let report = verify_reproducibility_manifest(&manifest_path, &dir).unwrap();
    assert!(report.passed);
    assert_eq!(report.matching_files, 2);
    assert!(report.missing_files.is_empty());
    assert!(report.mismatch_files.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn test_doctor_engine_diagnostics() {
    let engine = DoctorEngine::new().unwrap();
    let report = engine.run_diagnostics(None, false).await;
    assert!(!report.checks.is_empty());
    assert!(report.checks.iter().any(|c| c.name == "OTTER Version"));
    assert!(
        report
            .checks
            .iter()
            .any(|c| c.name.contains("Disk Free Space"))
    );
}
