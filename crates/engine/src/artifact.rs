use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactInspection {
    pub file_name: String,
    pub file_size_bytes: u64,
    pub format: ArtifactFormat,
    pub architecture: Option<String>,
    pub parameter_count: Option<u64>,
    pub tensor_count: Option<u64>,
    pub quantization: Option<String>,
    pub context_length: Option<u64>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtifactFormat {
    GGUF,
    Safetensors,
    Parquet,
    PyTorchCheckpoint,
    JsonManifest,
    ONNX,
    Unknown,
}

impl std::fmt::Display for ArtifactFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArtifactFormat::GGUF => write!(f, "GGUF Model"),
            ArtifactFormat::Safetensors => write!(f, "Safetensors Weights"),
            ArtifactFormat::Parquet => write!(f, "Parquet Shard"),
            ArtifactFormat::PyTorchCheckpoint => write!(f, "PyTorch Checkpoint"),
            ArtifactFormat::JsonManifest => write!(f, "JSON Manifest"),
            ArtifactFormat::ONNX => write!(f, "ONNX Model"),
            ArtifactFormat::Unknown => write!(f, "Unknown / Binary"),
        }
    }
}

/// Inspect an artifact file (GGUF, Safetensors, Parquet, or JSON)
pub fn inspect_artifact(path: impl AsRef<Path>) -> Result<ArtifactInspection> {
    let path = path.as_ref();
    let file = File::open(path)
        .with_context(|| format!("Failed to open artifact for inspection: {}", path.display()))?;
    let metadata = file.metadata()?;
    let file_size_bytes = metadata.len();
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let mut reader = BufReader::new(file);

    // 1. Try GGUF
    if file_size_bytes >= 4 {
        let mut magic = [0u8; 4];
        if reader.read_exact(&mut magic).is_ok() && &magic == b"GGUF" {
            return inspect_gguf(&mut reader, &file_name, file_size_bytes);
        }
    }

    // 2. Try Parquet
    if file_size_bytes >= 12 {
        let _ = reader.seek(SeekFrom::Start(0));
        let mut magic = [0u8; 4];
        if reader.read_exact(&mut magic).is_ok() && &magic == b"PAR1" {
            let _ = reader.seek(SeekFrom::End(-4));
            let mut end_magic = [0u8; 4];
            if reader.read_exact(&mut end_magic).is_ok() && &end_magic == b"PAR1" {
                return inspect_parquet(&mut reader, &file_name, file_size_bytes);
            }
        }
    }

    // 3. Try Safetensors
    if file_size_bytes >= 8 {
        let _ = reader.seek(SeekFrom::Start(0));
        let mut header_len_bytes = [0u8; 8];
        if reader.read_exact(&mut header_len_bytes).is_ok() {
            let header_len = u64::from_le_bytes(header_len_bytes);
            if header_len > 0 && header_len < 100_000_000 && header_len + 8 <= file_size_bytes {
                let mut header_buf = vec![0u8; header_len as usize];
                if reader.read_exact(&mut header_buf).is_ok()
                    && let Ok(json_val) = serde_json::from_slice::<serde_json::Value>(&header_buf)
                {
                    return inspect_safetensors(&file_name, file_size_bytes, &json_val);
                }
            }
        }
    }

    // 4. Try ONNX
    if file_name.ends_with(".onnx") || (file_size_bytes >= 2 && looks_like_onnx(&mut reader)) {
        let _ = reader.seek(SeekFrom::Start(0));
        if let Ok(inspection) = inspect_onnx(&mut reader, &file_name, file_size_bytes) {
            return Ok(inspection);
        }
    }

    // 5. Try JSON Manifest
    if file_name.ends_with(".json") && file_size_bytes < 50_000_000 {
        let _ = reader.seek(SeekFrom::Start(0));
        let mut content = String::new();
        if reader.read_to_string(&mut content).is_ok()
            && let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&content)
        {
            let mut meta = HashMap::new();
            if let Some(ds) = json_val.get("dataset").and_then(|v| v.as_str()) {
                meta.insert("dataset".to_string(), ds.to_string());
            }
            if let Some(fc) = json_val.get("file_count").and_then(|v| v.as_u64()) {
                meta.insert("file_count".to_string(), fc.to_string());
            }
            return Ok(ArtifactInspection {
                file_name,
                file_size_bytes,
                format: ArtifactFormat::JsonManifest,
                architecture: None,
                parameter_count: None,
                tensor_count: None,
                quantization: None,
                context_length: None,
                metadata: meta,
            });
        }
    }

    // 5. Fallback
    let format = if file_name.ends_with(".pt") || file_name.ends_with(".bin") {
        ArtifactFormat::PyTorchCheckpoint
    } else {
        ArtifactFormat::Unknown
    };

    Ok(ArtifactInspection {
        file_name,
        file_size_bytes,
        format,
        architecture: None,
        parameter_count: None,
        tensor_count: None,
        quantization: None,
        context_length: None,
        metadata: HashMap::new(),
    })
}

fn inspect_gguf<R: Read + Seek>(
    reader: &mut R,
    file_name: &str,
    file_size_bytes: u64,
) -> Result<ArtifactInspection> {
    let mut u32_buf = [0u8; 4];
    reader.read_exact(&mut u32_buf)?;
    let version = u32::from_le_bytes(u32_buf);

    let mut u64_buf = [0u8; 8];
    reader.read_exact(&mut u64_buf)?;
    let tensor_count = u64::from_le_bytes(u64_buf);

    reader.read_exact(&mut u64_buf)?;
    let kv_count = u64::from_le_bytes(u64_buf);

    let mut metadata = HashMap::new();
    metadata.insert("gguf_version".to_string(), version.to_string());
    metadata.insert("tensor_count".to_string(), tensor_count.to_string());
    metadata.insert("metadata_kv_count".to_string(), kv_count.to_string());

    let mut architecture = None;
    let mut quantization = None;
    let mut context_length = None;

    // Read top KV pairs safely
    for _ in 0..kv_count.min(100) {
        if let Ok(key) = read_gguf_string(reader) {
            let mut type_buf = [0u8; 4];
            if reader.read_exact(&mut type_buf).is_err() {
                break;
            }
            let val_type = u32::from_le_bytes(type_buf);
            match val_type {
                8 => {
                    // String
                    if let Ok(val) = read_gguf_string(reader) {
                        if key.ends_with("architecture") {
                            architecture = Some(val.clone());
                        }
                        if key.contains("file_type") || key.contains("quantization") {
                            quantization = Some(val.clone());
                        }
                        metadata.insert(key, val);
                    }
                }
                4 | 0 | 1 | 2 | 3 | 5 | 7 => {
                    // Small integer/bool
                    let mut b = [0u8; 4];
                    let _ = reader.read_exact(&mut b);
                    let num = u32::from_le_bytes(b);
                    if key.ends_with("context_length") {
                        context_length = Some(num as u64);
                    }
                    metadata.insert(key, num.to_string());
                }
                10 | 11 => {
                    // u64
                    let mut b = [0u8; 8];
                    let _ = reader.read_exact(&mut b);
                    let num = u64::from_le_bytes(b);
                    if key.ends_with("context_length") {
                        context_length = Some(num);
                    }
                    metadata.insert(key, num.to_string());
                }
                _ => {
                    // Unknown or array, stop metadata scan to avoid desync
                    break;
                }
            }
        } else {
            break;
        }
    }

    Ok(ArtifactInspection {
        file_name: file_name.to_string(),
        file_size_bytes,
        format: ArtifactFormat::GGUF,
        architecture,
        parameter_count: None,
        tensor_count: Some(tensor_count),
        quantization,
        context_length,
        metadata,
    })
}

fn read_gguf_string<R: Read>(reader: &mut R) -> Result<String> {
    let mut len_buf = [0u8; 8];
    reader.read_exact(&mut len_buf)?;
    let len = u64::from_le_bytes(len_buf);
    if len > 1024 * 1024 {
        bail!("GGUF string length exceeds 1MB safety limit");
    }
    let mut str_buf = vec![0u8; len as usize];
    reader.read_exact(&mut str_buf)?;
    Ok(String::from_utf8_lossy(&str_buf).to_string())
}

fn inspect_safetensors(
    file_name: &str,
    file_size_bytes: u64,
    json: &serde_json::Value,
) -> Result<ArtifactInspection> {
    let mut tensor_count = 0u64;
    let mut metadata = HashMap::new();
    let mut architecture = None;

    if let Some(obj) = json.as_object() {
        for (k, v) in obj {
            if k == "__metadata__" {
                if let Some(meta_obj) = v.as_object() {
                    for (mk, mv) in meta_obj {
                        if let Some(str_val) = mv.as_str() {
                            if mk.contains("architecture")
                                || (mk.contains("model_type") && architecture.is_none())
                            {
                                architecture = Some(str_val.to_string());
                            }
                            metadata.insert(mk.clone(), str_val.to_string());
                        }
                    }
                }
            } else {
                tensor_count += 1;
            }
        }
    }

    metadata.insert("tensor_count".to_string(), tensor_count.to_string());

    Ok(ArtifactInspection {
        file_name: file_name.to_string(),
        file_size_bytes,
        format: ArtifactFormat::Safetensors,
        architecture,
        parameter_count: None,
        tensor_count: Some(tensor_count),
        quantization: None,
        context_length: None,
        metadata,
    })
}

fn inspect_parquet<R: Read + Seek>(
    reader: &mut R,
    file_name: &str,
    file_size_bytes: u64,
) -> Result<ArtifactInspection> {
    let _ = reader.seek(SeekFrom::End(-8));
    let mut len_buf = [0u8; 4];
    let _ = reader.read_exact(&mut len_buf);
    let meta_len = u32::from_le_bytes(len_buf);

    let mut metadata = HashMap::new();
    metadata.insert("magic_header".to_string(), "PAR1".to_string());
    metadata.insert("magic_footer".to_string(), "PAR1".to_string());
    metadata.insert("metadata_bytes".to_string(), meta_len.to_string());

    Ok(ArtifactInspection {
        file_name: file_name.to_string(),
        file_size_bytes,
        format: ArtifactFormat::Parquet,
        architecture: None,
        parameter_count: None,
        tensor_count: None,
        quantization: None,
        context_length: None,
        metadata,
    })
}

// ---------------------------------------------------------------------------
// Reproducibility Manifest Generator and Verifier
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReproducibilityManifest {
    pub manifest_version: String,
    pub otter_version: String,
    pub created_at_utc: String,
    pub root_name: String,
    pub total_files: usize,
    pub total_size_bytes: u64,
    pub files: Vec<ManifestFileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestFileEntry {
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestVerificationReport {
    pub total_files_evaluated: usize,
    pub matching_files: usize,
    pub missing_files: Vec<String>,
    pub mismatch_files: Vec<String>,
    pub passed: bool,
}

pub fn create_reproducibility_manifest(
    root_path: impl AsRef<Path>,
) -> Result<ReproducibilityManifest> {
    let root = root_path.as_ref();
    if !root.exists() {
        bail!("Path does not exist: {}", root.display());
    }

    let root_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("artifact")
        .to_string();
    let mut files = Vec::new();
    let mut total_size_bytes = 0u64;

    if root.is_file() {
        let size = root.metadata()?.len();
        let hash = crate::verify::compute_file_sha256(root)?;
        let inspection = inspect_artifact(root).ok();
        let fmt = inspection
            .map(|i| i.format.to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        files.push(ManifestFileEntry {
            relative_path: root_name.clone(),
            size_bytes: size,
            sha256: hash,
            format: fmt,
        });
        total_size_bytes = size;
    } else {
        collect_dir_entries(root, root, &mut files, &mut total_size_bytes)?;
    }

    Ok(ReproducibilityManifest {
        manifest_version: "1.0.0".to_string(),
        otter_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at_utc: chrono::Utc::now().to_rfc3339(),
        root_name,
        total_files: files.len(),
        total_size_bytes,
        files,
    })
}

fn collect_dir_entries(
    dir: &Path,
    base: &Path,
    out: &mut Vec<ManifestFileEntry>,
    total_bytes: &mut u64,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let rel = path
                .strip_prefix(base)?
                .to_string_lossy()
                .replace('\\', "/");
            let size = entry.metadata()?.len();
            let hash = crate::verify::compute_file_sha256(&path)?;
            let inspection = inspect_artifact(&path).ok();
            let fmt = inspection
                .map(|i| i.format.to_string())
                .unwrap_or_else(|| "Unknown".to_string());

            *total_bytes += size;
            out.push(ManifestFileEntry {
                relative_path: rel,
                size_bytes: size,
                sha256: hash,
                format: fmt,
            });
        } else if path.is_dir() {
            collect_dir_entries(&path, base, out, total_bytes)?;
        }
    }
    Ok(())
}

pub fn verify_reproducibility_manifest(
    manifest_path: impl AsRef<Path>,
    target_dir: impl AsRef<Path>,
) -> Result<ManifestVerificationReport> {
    let manifest_bytes = std::fs::read(manifest_path.as_ref())?;
    let manifest: ReproducibilityManifest = serde_json::from_slice(&manifest_bytes)
        .context("Failed to parse reproducibility manifest JSON")?;

    let base = target_dir.as_ref();
    let mut matching_files = 0;
    let mut missing_files = Vec::new();
    let mut mismatch_files = Vec::new();

    for entry in &manifest.files {
        let file_path = base.join(&entry.relative_path);
        if !file_path.exists() {
            missing_files.push(entry.relative_path.clone());
            continue;
        }

        let computed = crate::verify::compute_file_sha256(&file_path)?;
        if computed.trim().eq_ignore_ascii_case(entry.sha256.trim()) {
            matching_files += 1;
        } else {
            mismatch_files.push(entry.relative_path.clone());
        }
    }

    let passed = missing_files.is_empty() && mismatch_files.is_empty();

    Ok(ManifestVerificationReport {
        total_files_evaluated: manifest.files.len(),
        matching_files,
        missing_files,
        mismatch_files,
        passed,
    })
}

fn looks_like_onnx<R: Read + Seek>(reader: &mut R) -> bool {
    let mut header = [0u8; 2];
    if reader.read_exact(&mut header).is_ok() {
        // Tag 0x08 = field 1 (ir_version), wire type 0 (varint).
        // Common ONNX IR versions range from 1 to 20.
        if header[0] == 0x08 && header[1] >= 1 && header[1] <= 20 {
            return true;
        }
        // Tag 0x3a = field 7 (graph), wire type 2 (length-delimited).
        if header[0] == 0x3a {
            return true;
        }
    }
    false
}

fn read_varint<R: Read>(reader: &mut R) -> Result<u64> {
    let mut result = 0u64;
    let mut shift = 0;
    loop {
        let mut byte = [0u8; 1];
        if reader.read_exact(&mut byte).is_err() {
            bail!("Unexpected EOF while reading varint");
        }
        let b = byte[0];
        result |= ((b & 0x7F) as u64) << shift;
        if (b & 0x80) == 0 {
            break;
        }
        shift += 7;
        if shift >= 64 {
            bail!("Protobuf varint overflow");
        }
    }
    Ok(result)
}

fn read_string_data<R: Read>(reader: &mut R, len: usize) -> Result<String> {
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).to_string())
}

fn skip_field_data<R: Read + Seek>(reader: &mut R, wire_type: u8) -> Result<()> {
    match wire_type {
        0 => {
            let _ = read_varint(reader)?;
        }
        1 => {
            reader.seek(SeekFrom::Current(8))?;
        }
        2 => {
            let len = read_varint(reader)?;
            reader.seek(SeekFrom::Current(len as i64))?;
        }
        5 => {
            reader.seek(SeekFrom::Current(4))?;
        }
        other => {
            bail!("Unsupported protobuf wire type: {}", other);
        }
    }
    Ok(())
}

/// Native ONNX Protobuf Model Inspection (AI-INSPECT-04)
pub fn inspect_onnx<R: Read + Seek>(
    reader: &mut R,
    file_name: &str,
    file_size_bytes: u64,
) -> Result<ArtifactInspection> {
    let mut metadata = HashMap::new();
    let mut graph_name: Option<String> = None;
    let mut tensor_count: u64 = 0;
    let mut node_count: u64 = 0;

    let total_len = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(0))?;

    while reader.stream_position()? < total_len {
        let tag = match read_varint(reader) {
            Ok(t) => t,
            Err(_) => break,
        };
        let field_num = (tag >> 3) as u32;
        let wire_type = (tag & 0x07) as u8;

        match field_num {
            1 if wire_type == 0 => {
                // ir_version
                let ir_ver = read_varint(reader)?;
                metadata.insert("ir_version".to_string(), ir_ver.to_string());
            }
            2 if wire_type == 2 => {
                // producer_name
                let len = read_varint(reader)? as usize;
                let s = read_string_data(reader, len)?;
                metadata.insert("producer_name".to_string(), s);
            }
            3 if wire_type == 2 => {
                // producer_version
                let len = read_varint(reader)? as usize;
                let s = read_string_data(reader, len)?;
                metadata.insert("producer_version".to_string(), s);
            }
            4 if wire_type == 2 => {
                // domain
                let len = read_varint(reader)? as usize;
                let s = read_string_data(reader, len)?;
                metadata.insert("domain".to_string(), s);
            }
            5 if wire_type == 0 => {
                // model_version
                let v = read_varint(reader)?;
                metadata.insert("model_version".to_string(), v.to_string());
            }
            6 if wire_type == 2 => {
                // doc_string
                let len = read_varint(reader)? as usize;
                let _ = read_string_data(reader, len)?;
            }
            7 if wire_type == 2 => {
                // graph (GraphProto)
                let graph_len = read_varint(reader)?;
                let start_pos = reader.stream_position()?;
                let end_pos = start_pos + graph_len;

                while reader.stream_position()? < end_pos {
                    let g_tag = match read_varint(reader) {
                        Ok(t) => t,
                        Err(_) => break,
                    };
                    let g_field = (g_tag >> 3) as u32;
                    let g_wire = (g_tag & 0x07) as u8;

                    match g_field {
                        1 if g_wire == 2 => {
                            // node (NodeProto)
                            node_count += 1;
                            skip_field_data(reader, g_wire)?;
                        }
                        2 if g_wire == 2 => {
                            // graph name
                            let len = read_varint(reader)? as usize;
                            graph_name = Some(read_string_data(reader, len)?);
                        }
                        5 if g_wire == 2 => {
                            // initializer (TensorProto)
                            tensor_count += 1;
                            skip_field_data(reader, g_wire)?;
                        }
                        _ => {
                            skip_field_data(reader, g_wire)?;
                        }
                    }
                }
                reader.seek(SeekFrom::Start(end_pos))?;
            }
            8 if wire_type == 2 => {
                // opset_import (OperatorSetIdProto)
                let opset_len = read_varint(reader)?;
                let start_pos = reader.stream_position()?;
                let end_pos = start_pos + opset_len;
                let mut domain = String::new();
                let mut version = 0u64;

                while reader.stream_position()? < end_pos {
                    let o_tag = match read_varint(reader) {
                        Ok(t) => t,
                        Err(_) => break,
                    };
                    let o_field = (o_tag >> 3) as u32;
                    let o_wire = (o_tag & 0x07) as u8;

                    match o_field {
                        1 if o_wire == 2 => {
                            let len = read_varint(reader)? as usize;
                            domain = read_string_data(reader, len)?;
                        }
                        2 if o_wire == 0 => {
                            version = read_varint(reader)?;
                        }
                        _ => {
                            skip_field_data(reader, o_wire)?;
                        }
                    }
                }
                reader.seek(SeekFrom::Start(end_pos))?;
                let opset_label = if domain.is_empty() {
                    format!("ai.onnx:v{}", version)
                } else {
                    format!("{}:v{}", domain, version)
                };
                metadata.insert("opset".to_string(), opset_label);
            }
            14 if wire_type == 2 => {
                // metadata_props (StringStringEntryProto)
                let entry_len = read_varint(reader)?;
                let start_pos = reader.stream_position()?;
                let end_pos = start_pos + entry_len;
                let mut key = String::new();
                let mut val = String::new();

                while reader.stream_position()? < end_pos {
                    let e_tag = match read_varint(reader) {
                        Ok(t) => t,
                        Err(_) => break,
                    };
                    let e_field = (e_tag >> 3) as u32;
                    let e_wire = (e_tag & 0x07) as u8;

                    match e_field {
                        1 if e_wire == 2 => {
                            let len = read_varint(reader)? as usize;
                            key = read_string_data(reader, len)?;
                        }
                        2 if e_wire == 2 => {
                            let len = read_varint(reader)? as usize;
                            val = read_string_data(reader, len)?;
                        }
                        _ => {
                            skip_field_data(reader, e_wire)?;
                        }
                    }
                }
                reader.seek(SeekFrom::Start(end_pos))?;
                if !key.is_empty() {
                    metadata.insert(format!("prop.{}", key), val);
                }
            }
            _ => {
                skip_field_data(reader, wire_type)?;
            }
        }
    }

    if node_count > 0 {
        metadata.insert("node_count".to_string(), node_count.to_string());
    }

    Ok(ArtifactInspection {
        file_name: file_name.to_string(),
        file_size_bytes,
        format: ArtifactFormat::ONNX,
        architecture: graph_name.or_else(|| Some("ONNX Graph".to_string())),
        parameter_count: None,
        tensor_count: Some(tensor_count),
        quantization: None,
        context_length: None,
        metadata,
    })
}

#[cfg(test)]
mod onnx_tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_native_onnx_inspection() {
        // Construct synthetic ONNX ModelProto bytes:
        // field 1 (ir_version = 8): tag 0x08, val 0x08
        // field 2 (producer_name = "pytorch"): tag 0x12, len 7, "pytorch"
        // field 8 (opset): tag 0x42, len 4: field 2 (version = 17): tag 0x10, val 17
        // field 7 (graph): tag 0x3A, len 16:
        //   field 2 (name = "resnet50"): tag 0x12, len 8, "resnet50"
        //   field 5 (initializer): tag 0x2A, len 2, 0x00, 0x00
        //   field 5 (initializer): tag 0x2A, len 1, 0x00

        let mut bytes = Vec::new();
        // ir_version = 8
        bytes.extend_from_slice(&[0x08, 0x08]);

        // producer_name = "pytorch"
        bytes.push(0x12);
        bytes.push(7);
        bytes.extend_from_slice(b"pytorch");

        // opset import: domain="", version=17
        bytes.push(0x42);
        bytes.push(2); // len
        bytes.push(0x10); // field 2, wire 0
        bytes.push(17); // version 17

        // graph:
        let mut graph_bytes = Vec::new();
        // graph name = "resnet50"
        graph_bytes.push(0x12);
        graph_bytes.push(8);
        graph_bytes.extend_from_slice(b"resnet50");
        // initializer 1
        graph_bytes.push(0x2A);
        graph_bytes.push(2);
        graph_bytes.extend_from_slice(&[0x01, 0x02]);
        // initializer 2
        graph_bytes.push(0x2A);
        graph_bytes.push(1);
        graph_bytes.extend_from_slice(&[0x03]);

        // field 7 tag (7 << 3 | 2 = 0x3A)
        bytes.push(0x3A);
        bytes.push(graph_bytes.len() as u8);
        bytes.extend_from_slice(&graph_bytes);

        let mut cursor = Cursor::new(&bytes);
        let inspection = inspect_onnx(&mut cursor, "model.onnx", bytes.len() as u64)
            .expect("inspect_onnx must succeed");

        assert_eq!(inspection.format, ArtifactFormat::ONNX);
        assert_eq!(inspection.architecture.as_deref(), Some("resnet50"));
        assert_eq!(inspection.tensor_count, Some(2));
        assert_eq!(
            inspection.metadata.get("ir_version").map(|s| s.as_str()),
            Some("8")
        );
        assert_eq!(
            inspection.metadata.get("producer_name").map(|s| s.as_str()),
            Some("pytorch")
        );
        assert_eq!(
            inspection.metadata.get("opset").map(|s| s.as_str()),
            Some("ai.onnx:v17")
        );
    }
}
