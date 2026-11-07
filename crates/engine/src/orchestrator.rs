use anyhow::{Context, Result, anyhow};
use futures_util::StreamExt;
use reqwest::Client;
use reqwest::header::{IF_RANGE, RANGE};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::disk::FileWriter;
use crate::manifest::ManifestStore;
use crate::probe::probe_url;
use crate::retry::{ErrorClassification, RetryPolicy};
use crate::segment::{SegmentTable, SharedSegmentTable};
use crate::verify::compute_file_sha256;
use std::path::PathBuf;
use types::{ByteRange, DownloadStatus, JobRecord, ProgressSnapshot};

#[cfg(windows)]
mod platform {
    pub const ES_CONTINUOUS: u32 = 0x80000000;
    pub const ES_SYSTEM_REQUIRED: u32 = 0x00000001;
    pub const ES_AWAYMODE_REQUIRED: u32 = 0x00000040;

    unsafe extern "system" {
        fn SetThreadExecutionState(esFlags: u32) -> u32;
    }

    pub struct ExecutionLock;

    impl ExecutionLock {
        pub fn acquire() -> Self {
            unsafe {
                SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_AWAYMODE_REQUIRED);
            }
            Self
        }
    }

    impl Drop for ExecutionLock {
        fn drop(&mut self) {
            unsafe {
                SetThreadExecutionState(ES_CONTINUOUS);
            }
        }
    }
}

pub struct DownloadManager {
    client: Client,
    manifest: Arc<ManifestStore>,
    progress_tx: broadcast::Sender<ProgressSnapshot>,
    host_semaphores:
        Arc<tokio::sync::Mutex<std::collections::HashMap<String, Arc<tokio::sync::Semaphore>>>>,
    max_host_connections: usize,
}

#[derive(Debug)]
enum AimdWorkerEvent {
    RateLimited(Option<Duration>),
    Error,
}

struct WorkerGuard(Arc<std::sync::atomic::AtomicUsize>);
impl Drop for WorkerGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl DownloadManager {
    pub fn new(manifest: Arc<ManifestStore>) -> Result<Self> {
        Self::with_max_host_connections(manifest, 16)
    }

    pub fn with_max_host_connections(
        manifest: Arc<ManifestStore>,
        max_host_connections: usize,
    ) -> Result<Self> {
        let client = Client::builder()
            .http1_only()
            .tcp_nodelay(true)
            .pool_max_idle_per_host(256)
            .pool_idle_timeout(Duration::from_secs(90))
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(15))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36 Otter/0.1.0")
            .build()
            .context("Failed to build HTTP client")?;

        let (progress_tx, _) = broadcast::channel(100);

        Ok(Self {
            client,
            manifest,
            progress_tx,
            host_semaphores: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            max_host_connections,
        })
    }

    pub fn subscribe_progress(&self) -> broadcast::Receiver<ProgressSnapshot> {
        self.progress_tx.subscribe()
    }

    pub fn manifest(&self) -> Arc<ManifestStore> {
        self.manifest.clone()
    }

    pub async fn run_job(
        &self,
        job_id: &str,
        max_connections: usize,
        cancel: CancellationToken,
    ) -> Result<DownloadStatus> {
        #[cfg(windows)]
        let _execution_lock = platform::ExecutionLock::acquire();

        let mut job = self
            .manifest
            .get_job(job_id)?
            .ok_or_else(|| anyhow!("Job not found in manifest: {}", job_id))?;

        self.manifest
            .update_status(job_id, DownloadStatus::Active, None)?;

        // 1. Re-probe remote file to verify capabilities and validators
        let probe = probe_url(&self.client, &job.url).await;
        let remote_info = match probe {
            Ok(info) => info,
            Err(e) => {
                let err_str = format!("Probe failed: {e:#}");
                let is_auth = err_str.contains("Authentication required")
                    || err_str.contains("401")
                    || err_str.contains("403");
                let status = if is_auth {
                    DownloadStatus::WaitingAuth
                } else {
                    DownloadStatus::Error
                };
                self.manifest
                    .update_status(job_id, status, Some(&err_str))?;
                return Err(anyhow!(err_str));
            }
        };

        // 2. Validator-aware Resume Verification (Never-Splice rule)
        if job.resumable && remote_info.resumable {
            let mut validator_mismatch = false;
            if let (Some(stored_etag), Some(remote_etag)) = (&job.etag, &remote_info.etag)
                && stored_etag != remote_etag
            {
                validator_mismatch = true;
            }
            if let (Some(stored_size), Some(remote_size)) = (job.total_size, remote_info.total_size)
                && stored_size != remote_size
            {
                validator_mismatch = true;
            }

            if validator_mismatch {
                tracing::warn!(
                    "Remote resource changed (ETag/Size mismatch). Discarding partial ranges to prevent splicing."
                );
                // Purge stale ranges and restart clean
                let _ = self.manifest.delete_job(job_id);
                job.etag = remote_info.etag.clone();
                job.total_size = remote_info.total_size;
                job.last_modified = remote_info.last_modified.clone();
                job.downloaded_bytes = 0;
                self.manifest.upsert_job(&job)?;
            }
        }

        // 3. Update job record with latest probe info
        job.total_size = remote_info.total_size;
        job.resumable = remote_info.resumable;
        job.etag = remote_info.etag.clone();
        job.last_modified = remote_info.last_modified.clone();
        self.manifest.upsert_job(&job)?;

        // 4. Branch: Single-connection non-resumable mode vs Segmented Resumable mode
        let result = match (job.resumable, job.total_size) {
            (true, Some(total_size)) if total_size > 0 => {
                self.download_segmented(&job, total_size, max_connections, cancel)
                    .await
            }
            _ => self.download_single_stream(&job, cancel).await,
        };

        match result {
            Ok(status) => {
                let _ = self.manifest.update_status(job_id, status, None);
                Ok(status)
            }
            Err(e) => {
                let err_str = format!("{e:#}");
                let _ = self
                    .manifest
                    .update_status(job_id, DownloadStatus::Error, Some(&err_str));
                Err(e)
            }
        }
    }

    async fn download_segmented(
        &self,
        job: &JobRecord,
        total_size: u64,
        connections: usize,
        cancel: CancellationToken,
    ) -> Result<DownloadStatus> {
        let dest_exists = std::path::Path::new(&job.destination).exists();
        let mut missing_ranges = self.manifest.get_missing_ranges(&job.id, total_size)?;
        if missing_ranges.is_empty() && !dest_exists {
            let _ = self.manifest.delete_job(&job.id);
            let mut reset_job = job.clone();
            reset_job.downloaded_bytes = 0;
            let _ = self.manifest.upsert_job(&reset_job);
            missing_ranges = vec![ByteRange::new(0, total_size - 1)];
        }
        if missing_ranges.is_empty() {
            // Already 100% downloaded, verify
            return self.finalize_and_verify(job).await;
        }

        let writer = match FileWriter::open_or_create(&job.destination, Some(total_size)) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                let err_str = format!("{e:#}");
                let is_disk_full = err_str.contains("Insufficient disk space");
                let status = if is_disk_full {
                    DownloadStatus::WaitingDisk
                } else {
                    DownloadStatus::Error
                };
                let _ = self.manifest.update_status(&job.id, status, Some(&err_str));
                return Err(anyhow!(err_str));
            }
        };

        let table: SharedSegmentTable = Arc::new(std::sync::Mutex::new(SegmentTable::new(
            total_size,
            &missing_ranges,
            connections,
        )));

        let downloaded_bytes = Arc::new(AtomicU64::new(
            total_size.saturating_sub(missing_ranges.iter().map(|r| r.len()).sum()),
        ));

        let initial_workers = connections.max(1);
        let active_workers = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let target_concurrency = Arc::new(std::sync::atomic::AtomicUsize::new(initial_workers));

        let aimd_config = crate::adaptive::AimdConfig {
            min_concurrency: 2.min(initial_workers),
            max_concurrency: connections.max(16),
            evaluation_interval: Duration::from_secs(5),
            throughput_gain_threshold: 0.10,
            backoff_duration: Duration::from_secs(5),
        };
        let mut aimd =
            crate::adaptive::AimdController::with_initial_concurrency(aimd_config, initial_workers);
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<AimdWorkerEvent>();

        let mut join_set = tokio::task::JoinSet::new();

        let host = reqwest::Url::parse(&job.url)
            .ok()
            .and_then(|u| u.host_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "default".to_string());

        let host_sem = {
            let mut map = self.host_semaphores.lock().await;
            map.entry(host)
                .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(self.max_host_connections)))
                .clone()
        };

        let spawn_worker = |join_set: &mut tokio::task::JoinSet<()>,
                            client: Client,
                            job_id: String,
                            url: String,
                            etag: Option<String>,
                            table: SharedSegmentTable,
                            writer: Arc<FileWriter>,
                            downloaded_ref: Arc<AtomicU64>,
                            cancel: CancellationToken,
                            host_sem: Arc<tokio::sync::Semaphore>,
                            target_concurrency: Arc<std::sync::atomic::AtomicUsize>,
                            active_workers: Arc<std::sync::atomic::AtomicUsize>,
                            event_tx: tokio::sync::mpsc::UnboundedSender<AimdWorkerEvent>,
                            manifest: Arc<ManifestStore>| {
            active_workers.fetch_add(1, Ordering::SeqCst);
            join_set.spawn(async move {
                worker_loop(
                    client,
                    job_id,
                    url,
                    etag,
                    table,
                    writer,
                    downloaded_ref,
                    cancel,
                    host_sem,
                    target_concurrency,
                    active_workers,
                    event_tx,
                    manifest,
                )
                .await
            });
        };

        for _ in 0..initial_workers {
            spawn_worker(
                &mut join_set,
                self.client.clone(),
                job.id.clone(),
                job.url.clone(),
                job.etag.clone(),
                table.clone(),
                writer.clone(),
                downloaded_bytes.clone(),
                cancel.clone(),
                host_sem.clone(),
                target_concurrency.clone(),
                active_workers.clone(),
                event_tx.clone(),
                self.manifest.clone(),
            );
        }

        // Monitoring and periodic SQLite checkpoint task
        let mut ticker = tokio::time::interval(Duration::from_millis(250));
        let mut last_checkpoint = Instant::now();
        let mut last_checkpoint_bytes = downloaded_bytes.load(Ordering::Relaxed);
        let mut last_speed_calc = Instant::now();
        let mut last_bytes = downloaded_bytes.load(Ordering::Relaxed);
        let _start_time = Instant::now();

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    tracing::info!("Download paused/cancelled by user");
                    let _ = writer.sync_data();
                    let current_done = downloaded_bytes.load(Ordering::Relaxed);
                    let mut updated_job = job.clone();
                    updated_job.downloaded_bytes = current_done;
                    updated_job.status = DownloadStatus::Paused;
                    let _ = self.manifest.upsert_job(&updated_job);
                    return Ok(DownloadStatus::Paused);
                }
                _ = ticker.tick() => {
                    let current_done = downloaded_bytes.load(Ordering::Relaxed);
                    let now = Instant::now();
                    let dt = (now - last_speed_calc).as_secs_f64();

                    // Process AIMD worker feedback events
                    while let Ok(event) = event_rx.try_recv() {
                        match event {
                            AimdWorkerEvent::RateLimited(retry_after) => {
                                let dec = aimd.record_rate_limit(retry_after);
                                if let crate::adaptive::AimdDecision::Decrease(cut) = dec {
                                    target_concurrency.store(cut, Ordering::SeqCst);
                                    tracing::info!("AIMD: Server 429 received -> cut concurrency to {}", cut);
                                }
                            }
                            AimdWorkerEvent::Error => {
                                aimd.record_error();
                            }
                        }
                    }

                    // Evaluate periodic AIMD throughput adaptation
                    match aimd.evaluate(current_done) {
                        crate::adaptive::AimdDecision::Increase(new_conc) => {
                            let cur_active = active_workers.load(Ordering::SeqCst);
                            target_concurrency.store(new_conc, Ordering::SeqCst);
                            if new_conc > cur_active {
                                tracing::info!("AIMD: +10% goodput gain detected -> increased concurrency to {}", new_conc);
                                for _ in cur_active..new_conc {
                                    spawn_worker(
                                        &mut join_set,
                                        self.client.clone(),
                                        job.id.clone(),
                                        job.url.clone(),
                                        job.etag.clone(),
                                        table.clone(),
                                        writer.clone(),
                                        downloaded_bytes.clone(),
                                        cancel.clone(),
                                        host_sem.clone(),
                                        target_concurrency.clone(),
                                        active_workers.clone(),
                                        event_tx.clone(),
                                        self.manifest.clone(),
                                    );
                                }
                            }
                        }
                        crate::adaptive::AimdDecision::Decrease(new_conc) => {
                            target_concurrency.store(new_conc, Ordering::SeqCst);
                            tracing::info!("AIMD: Decreased concurrency to {}", new_conc);
                        }
                        crate::adaptive::AimdDecision::Maintain(_) => {}
                    }

                    // Calculate speed
                    let speed = if dt > 0.0 {
                        (current_done.saturating_sub(last_bytes) as f64) / dt
                    } else {
                        0.0
                    };
                    last_bytes = current_done;
                    last_speed_calc = now;

                    let eta = if speed > 0.0 && total_size > current_done {
                        Some(((total_size - current_done) as f64 / speed) as u64)
                    } else {
                        None
                    };

                    let active_conn_count = active_workers.load(Ordering::Relaxed).max(1);

                    // Broadcast snapshot
                    let _ = self.progress_tx.send(ProgressSnapshot {
                        job_id: job.id.clone(),
                        filename: job.filename.clone(),
                        destination_path: job.destination.clone(),
                        total_bytes: Some(total_size),
                        downloaded_bytes: current_done,
                        verified_bytes: current_done,
                        speed_bps: speed,
                        eta_seconds: eta,
                        status: DownloadStatus::Active,
                        resumable: true,
                        connections: active_conn_count,
                        error_message: None,
                    });

                    // Checkpoint every 5 seconds or 32 MiB:
                    // 1. Flush NTFS disk buffers
                    // 2. Commit completed byte ranges to SQLite WAL inside a single transaction
                    let bytes_since_checkpoint = current_done.saturating_sub(last_checkpoint_bytes);
                    let time_since_checkpoint = last_checkpoint.elapsed();
                    if time_since_checkpoint >= Duration::from_secs(5) || bytes_since_checkpoint >= 32 * 1024 * 1024 {
                        let _ = writer.sync_data();
                        let snapshots = { table.lock().unwrap().snapshot_segments() };
                        let valid_ranges: Vec<(u64, u64)> = snapshots
                            .into_iter()
                            .filter(|&(start, pos, _end)| pos > start)
                            .map(|(start, pos, _end)| (start, pos - 1))
                            .collect();
                        let _ = self.manifest.add_completed_ranges_batch(&job.id, &valid_ranges);
                        let _ = self.manifest.update_downloaded_bytes(&job.id, current_done);
                        last_checkpoint = Instant::now();
                        last_checkpoint_bytes = current_done;
                    }

                    if current_done >= total_size {
                        break;
                    }
                }
                Some(res) = join_set.join_next() => {
                    if let Err(join_err) = res {
                        tracing::error!("Worker join error: {join_err}");
                    }
                    if join_set.is_empty() {
                        break;
                    }
                }
            }
        }

        // Flush and record completed ranges to SQLite
        let _ = writer.sync_data();
        let snapshots = { table.lock().unwrap().snapshot_segments() };
        let valid_ranges: Vec<(u64, u64)> = snapshots
            .into_iter()
            .filter(|&(start, pos, _end)| pos > start)
            .map(|(start, pos, _end)| (start, pos - 1))
            .collect();
        let _ = self
            .manifest
            .add_completed_ranges_batch(&job.id, &valid_ranges);

        let current_done = downloaded_bytes.load(Ordering::Relaxed);
        let _ = self.manifest.update_downloaded_bytes(&job.id, current_done);

        if current_done >= total_size {
            let _ = self
                .manifest
                .add_completed_range(&job.id, 0, total_size.saturating_sub(1));
            // Drop writer to release file handle before rename
            drop(writer);
            self.finalize_and_verify(job).await
        } else if cancel.is_cancelled() {
            Ok(DownloadStatus::Paused)
        } else {
            Err(anyhow!("Download stopped before completion"))
        }
    }

    async fn download_single_stream(
        &self,
        job: &JobRecord,
        cancel: CancellationToken,
    ) -> Result<DownloadStatus> {
        let writer = match FileWriter::open_or_create(&job.destination, job.total_size) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                let err_str = format!("{e:#}");
                let is_disk_full = err_str.contains("Insufficient disk space");
                let status = if is_disk_full {
                    DownloadStatus::WaitingDisk
                } else {
                    DownloadStatus::Error
                };
                let _ = self.manifest.update_status(&job.id, status, Some(&err_str));
                return Err(anyhow!(err_str));
            }
        };

        let mut resp = self
            .client
            .get(&job.url)
            .send()
            .await
            .context("Single stream request failed")?;

        let mut offset = 0u64;
        let mut last_tick = Instant::now();
        let mut last_offset = 0u64;

        while let Some(chunk_res) = resp.chunk().await.transpose() {
            if cancel.is_cancelled() {
                let _ = writer.sync_data();
                return Ok(DownloadStatus::Paused);
            }

            let chunk = chunk_res.context("Error receiving chunk in single stream")?;
            writer.write_at(offset, &chunk)?;
            offset += chunk.len() as u64;

            if last_tick.elapsed() >= Duration::from_millis(500) {
                let dt = last_tick.elapsed().as_secs_f64();
                let speed = (offset.saturating_sub(last_offset) as f64) / dt;
                last_offset = offset;
                last_tick = Instant::now();

                let eta = if let Some(tot) = job.total_size {
                    if speed > 0.0 && tot > offset {
                        Some(((tot - offset) as f64 / speed) as u64)
                    } else {
                        None
                    }
                } else {
                    None
                };

                let _ = self.progress_tx.send(ProgressSnapshot {
                    job_id: job.id.clone(),
                    filename: job.filename.clone(),
                    destination_path: job.destination.clone(),
                    total_bytes: job.total_size,
                    downloaded_bytes: offset,
                    verified_bytes: offset,
                    speed_bps: speed,
                    eta_seconds: eta,
                    status: DownloadStatus::Active,
                    resumable: false,
                    connections: 1,
                    error_message: None,
                });
            }
        }

        let _ = writer.sync_data();
        drop(writer);
        self.finalize_and_verify(job).await
    }

    async fn finalize_and_verify(&self, job: &JobRecord) -> Result<DownloadStatus> {
        self.manifest
            .update_status(&job.id, DownloadStatus::Verifying, None)?;

        let dest = PathBuf::from(&job.destination);
        let mut part = dest.clone();
        let file_name = match dest.file_name() {
            Some(name) => {
                let mut s = name.to_os_string();
                s.push(".part");
                s
            }
            None => "download.bin.part".into(),
        };
        part.set_file_name(file_name);

        let final_path = if part.exists() {
            if dest.exists() {
                let _ = std::fs::remove_file(&dest);
            }
            std::fs::rename(&part, &dest).with_context(|| {
                format!("Failed to rename {} to {}", part.display(), dest.display())
            })?;
            #[cfg(windows)]
            let _ = crate::disk::attach_zone_identifier(&dest, Some(&job.url));
            dest
        } else if dest.exists() {
            dest
        } else {
            return Err(anyhow!("Destination file not found after download"));
        };

        // Compute SHA-256
        let computed_hash =
            compute_file_sha256(&final_path).context("Failed to compute final SHA-256")?;

        let mut updated = job.clone();
        if let Some(tot) = job.total_size {
            updated.downloaded_bytes = tot;
        } else if let Ok(meta) = std::fs::metadata(&final_path) {
            updated.downloaded_bytes = meta.len();
        }
        updated.computed_sha256 = Some(computed_hash.clone());
        updated.completed_at = Some(chrono::Utc::now().timestamp_millis());

        if let Some(expected) = &job.expected_sha256
            && !expected.eq_ignore_ascii_case(&computed_hash)
        {
            let err_msg = format!(
                "SHA-256 mismatch! Expected: {}, Computed: {}",
                expected, computed_hash
            );
            self.manifest
                .update_status(&job.id, DownloadStatus::Error, Some(&err_msg))?;
            return Err(anyhow!(err_msg));
        }

        updated.status = DownloadStatus::Completed;
        self.manifest.upsert_job(&updated)?;

        Ok(DownloadStatus::Completed)
    }
}

#[allow(clippy::too_many_arguments)]
async fn worker_loop(
    client: Client,
    job_id: String,
    url: String,
    etag: Option<String>,
    table: SharedSegmentTable,
    writer: Arc<FileWriter>,
    downloaded_ref: Arc<AtomicU64>,
    cancel: CancellationToken,
    host_sem: Arc<tokio::sync::Semaphore>,
    target_concurrency: Arc<std::sync::atomic::AtomicUsize>,
    active_workers: Arc<std::sync::atomic::AtomicUsize>,
    event_tx: tokio::sync::mpsc::UnboundedSender<AimdWorkerEvent>,
    manifest: Arc<ManifestStore>,
) {
    let _guard = WorkerGuard(active_workers.clone());
    let retry_policy = RetryPolicy::default();

    loop {
        if cancel.is_cancelled() {
            break;
        }

        // Dynamic AIMD scaling down: if active workers exceed target concurrency, exit gracefully
        if active_workers.load(Ordering::SeqCst) > target_concurrency.load(Ordering::SeqCst) {
            break;
        }

        let _permit = match host_sem.acquire().await {
            Ok(p) => p,
            Err(_) => break,
        };

        let claim = {
            let mut tbl = table.lock().unwrap();
            tbl.claim()
        };

        let (seg_id, range) = match claim {
            Some(c) => c,
            None => {
                // No more work available
                tokio::time::sleep(Duration::from_millis(100)).await;
                let is_done = { table.lock().unwrap().is_all_complete() };
                if is_done {
                    break;
                }
                continue;
            }
        };

        let mut attempt = 0;
        let mut cur_offset = range.start;

        while !cancel.is_cancelled() {
            let active_end = {
                let tbl = table.lock().unwrap();
                match tbl.current_end(seg_id) {
                    Some(e) if cur_offset <= e => e,
                    _ => break,
                }
            };

            let mut req = client
                .get(&url)
                .header(RANGE, format!("bytes={}-{}", cur_offset, active_end));

            if let Some(et) = &etag {
                req = req.header(IF_RANGE, et.as_str());
            }

            let resp = match req.send().await {
                Ok(r) => {
                    if attempt > 0 {
                        tracing::info!(
                            "Network connectivity confirmed. Resumed segment {seg_id} from {cur_offset}."
                        );
                    }
                    r
                }
                Err(e) => {
                    attempt += 1;
                    let _ = event_tx.send(AimdWorkerEvent::Error);
                    if e.is_connect() || e.is_timeout() {
                        tracing::warn!(
                            "Network connection lost: {e}. Retrying ({attempt}/{})...",
                            retry_policy.max_retries
                        );
                    } else {
                        tracing::warn!(
                            "Request failed ({attempt}/{}): {e}",
                            retry_policy.max_retries
                        );
                    }
                    if attempt > retry_policy.max_retries {
                        tracing::error!("Segment {seg_id} exceeded max retries: {e}");
                        let mut tbl = table.lock().unwrap();
                        tbl.release(seg_id);
                        return;
                    }
                    let backoff = retry_policy.calculate_backoff(attempt);
                    tokio::time::sleep(backoff).await;
                    continue;
                }
            };

            let status = resp.status();
            if status == reqwest::StatusCode::OK {
                if cur_offset != 0 {
                    tracing::error!(
                        "Server returned 200 OK to Range request with If-Range: remote resource mutated or byte ranges unsupported"
                    );
                    let mut tbl = table.lock().unwrap();
                    tbl.release(seg_id);
                    return;
                }
            } else if status != reqwest::StatusCode::PARTIAL_CONTENT {
                let classification = RetryPolicy::classify_status(status);
                if classification == ErrorClassification::RateLimited {
                    let sleep_dur = RetryPolicy::parse_retry_after(resp.headers())
                        .unwrap_or_else(|| Duration::from_secs(5));
                    let _ = event_tx.send(AimdWorkerEvent::RateLimited(Some(sleep_dur)));
                    tokio::time::sleep(sleep_dur).await;
                } else if classification == ErrorClassification::AuthOrExpired {
                    tracing::error!("Authorization expired for segment {seg_id}");
                    let mut tbl = table.lock().unwrap();
                    tbl.release(seg_id);
                    let _ = manifest.update_status(
                        &job_id,
                        DownloadStatus::WaitingAuth,
                        Some("Authorization expired or invalid (HTTP 401/403)"),
                    );
                    return;
                }
                attempt += 1;
                tokio::time::sleep(retry_policy.calculate_backoff(attempt)).await;
                continue;
            }

            let mut stream = resp.bytes_stream();
            let mut stream_failed = false;
            let mut uncommitted_bytes = 0u64;
            let mut write_buf = Vec::with_capacity(1024 * 1024);

            while let Some(chunk_res) = stream.next().await {
                if cancel.is_cancelled() {
                    break;
                }

                match chunk_res {
                    Ok(chunk) => {
                        let dynamic_end = {
                            let tbl = table.lock().unwrap();
                            tbl.current_end(seg_id).unwrap_or(active_end)
                        };
                        let remaining_in_range =
                            (dynamic_end + 1).saturating_sub(cur_offset + write_buf.len() as u64);
                        if remaining_in_range == 0 {
                            break;
                        }
                        let to_take = (chunk.len() as u64).min(remaining_in_range) as usize;
                        write_buf.extend_from_slice(&chunk[..to_take]);

                        // Batch write to disk every 512KB to minimize Windows WriteFile syscalls
                        if write_buf.len() >= 512 * 1024 {
                            if let Err(e) = writer.write_at(cur_offset, &write_buf) {
                                tracing::error!("Write error at {cur_offset}: {e}");
                                stream_failed = true;
                                break;
                            }
                            let written_len = write_buf.len() as u64;
                            cur_offset += written_len;
                            downloaded_ref.fetch_add(written_len, Ordering::Relaxed);
                            uncommitted_bytes += written_len;
                            write_buf.clear();

                            // Batch advance table mutex every 512KB to eliminate thread lock contention
                            if uncommitted_bytes >= 512 * 1024 {
                                let mut tbl = table.lock().unwrap();
                                tbl.advance(seg_id, uncommitted_bytes);
                                uncommitted_bytes = 0;
                            }
                        }

                        attempt = 0; // reset retry counter on progress

                        if to_take < chunk.len()
                            || cur_offset + write_buf.len() as u64 > dynamic_end
                        {
                            // Reached end of assigned segment boundary
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Stream error at {cur_offset}: {e}");
                        stream_failed = true;
                        break;
                    }
                }
            }

            // Flush remaining write buffer to disk upon stream end
            if !write_buf.is_empty() && !stream_failed {
                if let Err(e) = writer.write_at(cur_offset, &write_buf) {
                    tracing::error!("Write error at {cur_offset}: {e}");
                    stream_failed = true;
                } else {
                    let written_len = write_buf.len() as u64;
                    cur_offset += written_len;
                    downloaded_ref.fetch_add(written_len, Ordering::Relaxed);
                    uncommitted_bytes += written_len;
                }
                write_buf.clear();
            }

            // Flush any remaining uncommitted bytes to the segment table
            if uncommitted_bytes > 0 {
                let mut tbl = table.lock().unwrap();
                tbl.advance(seg_id, uncommitted_bytes);
            }

            if stream_failed {
                attempt += 1;
                tokio::time::sleep(retry_policy.calculate_backoff(attempt)).await;
            }
        }

        let mut tbl = table.lock().unwrap();
        tbl.release(seg_id);
    }
}
