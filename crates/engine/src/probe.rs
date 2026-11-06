use anyhow::{Context, Result, anyhow};
use reqwest::Client;
use reqwest::header::{
    CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG, HeaderMap,
    LAST_MODIFIED, RANGE,
};
use std::path::Path;
use types::RemoteFileInfo;

pub async fn probe_url(client: &Client, url: &str) -> Result<RemoteFileInfo> {
    let resp = client
        .get(url)
        .header(RANGE, "bytes=0-0")
        .send()
        .await
        .context("Failed to send probe request")?;

    let status = resp.status();
    let headers = resp.headers().clone();

    let etag = headers
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string());

    let last_modified = headers
        .get(LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string());

    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string());

    let accept_ranges_bytes = headers
        .get(reqwest::header::ACCEPT_RANGES)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.eq_ignore_ascii_case("bytes"))
        .unwrap_or(false);

    let filename = extract_filename(&headers, url);

    if status == reqwest::StatusCode::PARTIAL_CONTENT {
        // Status 206: Server supports byte ranges
        let total_size = parse_content_range_total(&headers);
        Ok(RemoteFileInfo {
            url: url.to_string(),
            filename,
            total_size,
            resumable: true,
            etag,
            last_modified,
            content_type,
        })
    } else if status == reqwest::StatusCode::OK {
        // Status 200: Server responded with full content or redirected
        let total_size = headers
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());

        let resumable = accept_ranges_bytes && total_size.is_some();

        Ok(RemoteFileInfo {
            url: url.to_string(),
            filename,
            total_size,
            resumable,
            etag,
            last_modified,
            content_type,
        })
    } else if status == reqwest::StatusCode::UNAUTHORIZED
        || status == reqwest::StatusCode::FORBIDDEN
    {
        Err(anyhow!(
            "Authentication required or access token expired (HTTP {})",
            status
        ))
    } else {
        Err(anyhow!(
            "Server returned unexpected status code during probe: {}",
            status
        ))
    }
}

fn parse_content_range_total(headers: &HeaderMap) -> Option<u64> {
    let header_val = headers.get(CONTENT_RANGE)?.to_str().ok()?;
    // Format: "bytes 0-0/1234567" or "bytes 0-0/*"
    let slash_pos = header_val.rfind('/')?;
    let total_str = header_val[slash_pos + 1..].trim();
    total_str.parse::<u64>().ok()
}

pub fn extract_filename(headers: &HeaderMap, url: &str) -> String {
    if let Some(cd) = headers
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        && let Some(fname) = parse_content_disposition_filename(cd)
    {
        let sanitized = sanitize_filename(&fname);
        if !sanitized.is_empty() {
            return sanitized;
        }
    }

    // Fallback to URL path
    if let Ok(parsed_url) = reqwest::Url::parse(url)
        && let Some(mut segments) = parsed_url.path_segments()
        && let Some(last) = segments.next_back()
    {
        let decoded = percent_encoding::percent_decode_str(last)
            .decode_utf8_lossy()
            .to_string();
        let sanitized = sanitize_filename(&decoded);
        if !sanitized.is_empty() {
            return sanitized;
        }
    }

    "download.bin".to_string()
}

fn parse_content_disposition_filename(cd: &str) -> Option<String> {
    // 1. filename*=UTF-8''... (RFC 5987 / 6266)
    if let Some(idx) = cd.to_lowercase().find("filename*=") {
        let rest = &cd[idx + 10..];
        let val = rest
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches('"');
        if let Some(tick_pos) = val.rfind("''") {
            let encoded = &val[tick_pos + 2..];
            if let Ok(decoded) = percent_encoding::percent_decode_str(encoded).decode_utf8() {
                return Some(decoded.to_string());
            }
        }
    }

    // 2. filename="..."
    if let Some(idx) = cd.to_lowercase().find("filename=") {
        let rest = &cd[idx + 9..];
        let val = rest
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches('"');
        if !val.is_empty() {
            return Some(val.to_string());
        }
    }

    None
}

pub fn sanitize_filename(name: &str) -> String {
    let reserved_names = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];

    let base_name = Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(name);

    let mut sanitized = String::with_capacity(base_name.len());
    for ch in base_name.chars() {
        if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
            sanitized.push('_');
        } else {
            sanitized.push(ch);
        }
    }

    let trimmed = sanitized.trim().trim_matches('.');
    let upper = trimmed.to_ascii_uppercase();
    if reserved_names.contains(&upper.as_str()) {
        format!("{}_file", trimmed)
    } else if trimmed.is_empty() {
        "download.bin".to_string()
    } else {
        trimmed.to_string()
    }
}
