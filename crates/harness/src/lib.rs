//! Automated 9-Profile Loopback Test Server Harness (WBS-M0-01)
//!
//! Provides deterministic loopback test servers implementing the 9 behavioral profiles:
//! 1. srv_fast_stable
//! 2. srv_throttled_per_conn
//! 3. srv_rate_limited_429
//! 4. srv_transient_5xx
//! 5. srv_no_range
//! 6. srv_chunked_unknown
//! 7. srv_expired_auth
//! 8. srv_changed_resource
//! 9. srv_silent_stall

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub enum ServerProfile {
    /// High-speed endpoint supporting HTTP 206 Partial Content, ETag, and Content-Range
    FastStable,
    /// Bandwidth throttled per connection (e.g. 512 KB/s) to verify multi-connection speedup
    ThrottledPerConn { kbps: u64 },
    /// Returns HTTP 429 Too Many Requests when concurrency exceeds limit with Retry-After header
    RateLimited429 {
        max_conns: usize,
        retry_after_secs: u64,
    },
    /// Injects transient HTTP 503 Service Unavailable errors on every Nth request
    Transient5xx { fail_every_n: usize },
    /// Ignores Range headers, emits 200 OK with full payload
    NoRange,
    /// Emits Transfer-Encoding: chunked without Content-Length
    ChunkedUnknown,
    /// Returns HTTP 401 Unauthorized unless valid token is present
    ExpiredAuth { valid_token: String },
    /// Mutates ETag mid-stream to test Never-Splice validator mismatch detection
    ChangedResource {
        mutated_etag: String,
        mutated_payload: Vec<u8>,
    },
    /// Sends initial bytes and silently stalls TCP socket to test read timeout recovery
    SilentStall { stall_after_bytes: usize },
}

pub struct ServerStats {
    pub total_requests: AtomicUsize,
    pub rate_limited_429s: AtomicUsize,
    pub error_5xxs: AtomicUsize,
    pub active_connections: AtomicUsize,
}

pub struct LoopbackServer {
    addr: SocketAddr,
    cancel: CancellationToken,
    stats: Arc<ServerStats>,
    etag: String,
}

impl LoopbackServer {
    pub async fn start(profile: ServerProfile, payload: Vec<u8>) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .context("Failed to bind loopback test server to ephemeral port")?;
        let addr = listener.local_addr()?;

        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let etag = format!("\"{}\"", hex::encode(hasher.finalize()));

        let stats = Arc::new(ServerStats {
            total_requests: AtomicUsize::new(0),
            rate_limited_429s: AtomicUsize::new(0),
            error_5xxs: AtomicUsize::new(0),
            active_connections: AtomicUsize::new(0),
        });

        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let stats_clone = stats.clone();
        let payload = Arc::new(payload);
        let etag_clone = etag.clone();

        tokio::spawn(async move {
            let mut req_counter = 0usize;

            loop {
                tokio::select! {
                    _ = cancel_clone.cancelled() => break,
                    accept_res = listener.accept() => {
                        let (socket, _) = match accept_res {
                            Ok(pair) => pair,
                            Err(_) => break,
                        };

                        req_counter += 1;
                        let profile_clone = profile.clone();
                        let payload_clone = payload.clone();
                        let etag_val = etag_clone.clone();
                        let stats_ref = stats_clone.clone();
                        let current_req = req_counter;

                        tokio::spawn(async move {
                            stats_ref.active_connections.fetch_add(1, Ordering::SeqCst);
                            stats_ref.total_requests.fetch_add(1, Ordering::Relaxed);

                            let _ = handle_connection(
                                socket,
                                profile_clone,
                                payload_clone,
                                etag_val,
                                stats_ref.clone(),
                                current_req,
                            ).await;

                            stats_ref.active_connections.fetch_sub(1, Ordering::SeqCst);
                        });
                    }
                }
            }
        });

        Ok(Self {
            addr,
            cancel,
            stats,
            etag,
        })
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/artifact.bin", self.addr.port())
    }

    pub fn etag(&self) -> &str {
        &self.etag
    }

    pub fn stats(&self) -> &ServerStats {
        &self.stats
    }

    pub fn stop(self) {
        self.cancel.cancel();
    }
}

impl Drop for LoopbackServer {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    profile: ServerProfile,
    payload: Arc<Vec<u8>>,
    etag: String,
    stats: Arc<ServerStats>,
    req_num: usize,
) -> Result<()> {
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).await?;
    if n == 0 {
        return Ok(());
    }

    let req_str = String::from_utf8_lossy(&buf[..n]);
    let first_line = req_str.lines().next().unwrap_or("");
    let path = first_line.split_whitespace().nth(1).unwrap_or("/");

    // Extract headers
    let mut range_header = None;
    let mut if_range_header = None;
    let mut auth_header = None;

    for line in req_str.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("range:") {
            range_header = Some(line[6..].trim().to_string());
        } else if lower.starts_with("if-range:") {
            if_range_header = Some(line[9..].trim().to_string());
        } else if lower.starts_with("authorization:") {
            auth_header = Some(line[14..].trim().to_string());
        }
    }

    // Check ExpiredAuth profile
    if let ServerProfile::ExpiredAuth { ref valid_token } = profile {
        let auth_ok = auth_header
            .as_ref()
            .map(|h| h.contains(valid_token))
            .unwrap_or(false)
            || path.contains(&format!("token={}", valid_token));
        if !auth_ok {
            let resp = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nWWW-Authenticate: Bearer\r\nConnection: close\r\n\r\n";
            stream.write_all(resp.as_bytes()).await?;
            return Ok(());
        }
    }

    // Check RateLimited429 profile
    if let ServerProfile::RateLimited429 {
        max_conns,
        retry_after_secs,
    } = profile
    {
        let current_active = stats.active_connections.load(Ordering::SeqCst);
        if current_active > max_conns {
            stats.rate_limited_429s.fetch_add(1, Ordering::Relaxed);
            let resp = format!(
                "HTTP/1.1 429 Too Many Requests\r\nRetry-After: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                retry_after_secs
            );
            stream.write_all(resp.as_bytes()).await?;
            return Ok(());
        }
    }

    // Check Transient5xx profile
    if let ServerProfile::Transient5xx { fail_every_n } = profile
        && fail_every_n > 0
        && req_num.is_multiple_of(fail_every_n)
    {
        stats.error_5xxs.fetch_add(1, Ordering::Relaxed);
        let resp = "HTTP/1.1 503 Service Unavailable\r\nRetry-After: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        stream.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    // Check ChangedResource profile
    let (active_payload, active_etag) = match profile {
        ServerProfile::ChangedResource {
            ref mutated_etag,
            ref mutated_payload,
        } => (Arc::new(mutated_payload.clone()), mutated_etag.clone()),
        _ => (payload.clone(), etag.clone()),
    };

    // Check If-Range validator mismatch (Never-Splice)
    if let Some(if_range) = if_range_header
        && !if_range
            .trim_matches('"')
            .eq_ignore_ascii_case(active_etag.trim_matches('"'))
    {
        // Mismatch: return 200 OK full content per RFC 9110
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: {}\r\nConnection: close\r\n\r\n",
            active_payload.len(),
            active_etag
        );
        stream.write_all(resp.as_bytes()).await?;
        stream.write_all(&active_payload).await?;
        return Ok(());
    }

    // Check NoRange profile
    if let ServerProfile::NoRange = profile {
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            active_payload.len()
        );
        stream.write_all(resp.as_bytes()).await?;
        stream.write_all(&active_payload).await?;
        return Ok(());
    }

    // Check ChunkedUnknown profile
    if let ServerProfile::ChunkedUnknown = profile {
        let header = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
        stream.write_all(header.as_bytes()).await?;
        for chunk in active_payload.chunks(8192) {
            let chunk_head = format!("{:X}\r\n", chunk.len());
            stream.write_all(chunk_head.as_bytes()).await?;
            stream.write_all(chunk).await?;
            stream.write_all(b"\r\n").await?;
        }
        stream.write_all(b"0\r\n\r\n").await?;
        return Ok(());
    }

    let total_len = active_payload.len() as u64;

    // Parse Range header if present
    if let Some(range_val) = range_header
        && let Some(range_spec) = range_val.strip_prefix("bytes=")
    {
        let parts: Vec<&str> = range_spec.split('-').collect();
        let start = parts
            .first()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let end = parts
            .get(1)
            .and_then(|s| {
                if s.is_empty() {
                    None
                } else {
                    s.parse::<u64>().ok()
                }
            })
            .unwrap_or(total_len.saturating_sub(1))
            .min(total_len.saturating_sub(1));

        if start <= end && start < total_len {
            let range_slice = &active_payload[start as usize..=end as usize];
            let content_len = range_slice.len();

            let resp_head = format!(
                "HTTP/1.1 206 Partial Content\r\n\
                     Accept-Ranges: bytes\r\n\
                     Content-Range: bytes {}-{}/{}\r\n\
                     Content-Length: {}\r\n\
                     ETag: {}\r\n\
                     Connection: keep-alive\r\n\r\n",
                start, end, total_len, content_len, active_etag
            );
            stream.write_all(resp_head.as_bytes()).await?;

            // Check SilentStall profile
            if let ServerProfile::SilentStall { stall_after_bytes } = profile {
                let to_send = stall_after_bytes.min(range_slice.len());
                stream.write_all(&range_slice[..to_send]).await?;
                // Stall indefinitely until socket drops
                tokio::time::sleep(Duration::from_secs(60)).await;
                return Ok(());
            }

            // Check ThrottledPerConn profile
            if let ServerProfile::ThrottledPerConn { kbps } = profile {
                let chunk_size = 16 * 1024;
                let delay_per_chunk = Duration::from_millis(
                    (chunk_size as f64 / (kbps as f64 * 1024.0) * 1000.0) as u64,
                );
                for chunk in range_slice.chunks(chunk_size) {
                    stream.write_all(chunk).await?;
                    tokio::time::sleep(delay_per_chunk).await;
                }
                return Ok(());
            }

            // FastStable / default range write
            stream.write_all(range_slice).await?;
            return Ok(());
        }
    }

    // Default 200 OK response if no Range requested
    let resp = format!(
        "HTTP/1.1 200 OK\r\n\
         Accept-Ranges: bytes\r\n\
         Content-Length: {}\r\n\
         ETag: {}\r\n\
         Connection: close\r\n\r\n",
        total_len, active_etag
    );
    stream.write_all(resp.as_bytes()).await?;
    stream.write_all(&active_payload).await?;
    Ok(())
}
