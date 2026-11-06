use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use types::{ByteRange, DownloadStatus, JobRecord};

pub struct ManifestStore {
    conn: Arc<Mutex<Connection>>,
    db_path: PathBuf,
}

impl ManifestStore {
    pub fn open_default() -> Result<Self> {
        let db_dir = if let Ok(custom) = std::env::var("OTTER_DATA_DIR") {
            PathBuf::from(custom)
        } else if let Ok(app_data) = std::env::var("LOCALAPPDATA") {
            let candidate = PathBuf::from(app_data).join("otter");
            if std::fs::create_dir_all(&candidate).is_ok() {
                candidate
            } else {
                PathBuf::from(".otter")
            }
        } else {
            PathBuf::from(".otter")
        };

        let _ = std::fs::create_dir_all(&db_dir);
        let db_path = db_dir.join("manifest.db");
        Self::open(&db_path)
    }

    pub fn open(db_path: impl AsRef<Path>) -> Result<Self> {
        let path = db_path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&path)
            .with_context(|| format!("Failed to open SQLite database: {}", path.display()))?;

        // Enforce crash resilience and concurrency:
        // In WAL mode, synchronous = NORMAL is fully crash-durable against application
        // and OS crashes, eliminating redundant synchronous disk flush stalls.
        // Implement DEC-ITEM-4: Tune WAL checkpoint cadence according to media seek characteristics.
        let wal_pages = if crate::disk::is_rotational_media(&path) {
            8000 // 32 MB WAL checkpoint interval on rotational HDDs to prevent arm thrashing
        } else {
            1000 // 4 MB default WAL checkpoint interval on SSDs / NVMe
        };

        conn.execute_batch(&format!(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;
             PRAGMA wal_autocheckpoint = {};",
            wal_pages
        ))?;

        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path: path,
        };

        store.init_schema()?;
        Ok(store)
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS jobs (
                id TEXT PRIMARY KEY NOT NULL,
                url TEXT NOT NULL,
                filename TEXT NOT NULL,
                destination TEXT NOT NULL,
                total_size INTEGER,
                downloaded_bytes INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL,
                resumable INTEGER NOT NULL,
                etag TEXT,
                last_modified TEXT,
                expected_sha256 TEXT,
                computed_sha256 TEXT,
                error TEXT,
                created_at INTEGER NOT NULL,
                completed_at INTEGER
            );

            CREATE TABLE IF NOT EXISTS completed_ranges (
                job_id TEXT NOT NULL,
                start_offset INTEGER NOT NULL,
                end_offset INTEGER NOT NULL,
                PRIMARY KEY (job_id, start_offset),
                FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_ranges_job ON completed_ranges(job_id);",
        )?;
        Ok(())
    }

    pub fn upsert_job(&self, job: &JobRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO jobs (
                id, url, filename, destination, total_size, downloaded_bytes,
                status, resumable, etag, last_modified, expected_sha256,
                computed_sha256, error, created_at, completed_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
            ON CONFLICT(id) DO UPDATE SET
                url = excluded.url,
                filename = excluded.filename,
                destination = excluded.destination,
                total_size = excluded.total_size,
                downloaded_bytes = excluded.downloaded_bytes,
                status = excluded.status,
                resumable = excluded.resumable,
                etag = excluded.etag,
                last_modified = excluded.last_modified,
                expected_sha256 = excluded.expected_sha256,
                computed_sha256 = excluded.computed_sha256,
                error = excluded.error,
                completed_at = excluded.completed_at;",
            params![
                job.id,
                job.url,
                job.filename,
                job.destination,
                job.total_size.map(|s| s as i64),
                job.downloaded_bytes as i64,
                job.status.to_string(),
                if job.resumable { 1 } else { 0 },
                job.etag,
                job.last_modified,
                job.expected_sha256,
                job.computed_sha256,
                job.error,
                job.created_at,
                job.completed_at,
            ],
        )?;
        Ok(())
    }

    pub fn add_completed_range(&self, job_id: &str, start: u64, end: u64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO completed_ranges (job_id, start_offset, end_offset)
             VALUES (?1, ?2, ?3);",
            params![job_id, start as i64, end as i64],
        )?;
        Ok(())
    }

    pub fn add_completed_ranges_batch(&self, job_id: &str, ranges: &[(u64, u64)]) -> Result<()> {
        if ranges.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO completed_ranges (job_id, start_offset, end_offset)
                 VALUES (?1, ?2, ?3);",
            )?;
            for &(start, end) in ranges {
                stmt.execute(params![job_id, start as i64, end as i64])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_completed_ranges(&self, job_id: &str) -> Result<Vec<ByteRange>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT start_offset, end_offset FROM completed_ranges
             WHERE job_id = ?1 ORDER BY start_offset ASC;",
        )?;

        let rows = stmt.query_map(params![job_id], |row| {
            let start: i64 = row.get(0)?;
            let end: i64 = row.get(1)?;
            Ok(ByteRange::new(start as u64, end as u64))
        })?;

        let mut ranges = Vec::new();
        for r in rows {
            ranges.push(r?);
        }
        Ok(merge_adjacent_ranges(&ranges))
    }

    pub fn get_missing_ranges(&self, job_id: &str, total_size: u64) -> Result<Vec<ByteRange>> {
        let completed = self.get_completed_ranges(job_id)?;
        if completed.is_empty() {
            if total_size > 0 {
                return Ok(vec![ByteRange::new(0, total_size - 1)]);
            } else {
                return Ok(Vec::new());
            }
        }

        let mut missing = Vec::new();
        let mut cursor = 0u64;

        for r in completed {
            if r.start > cursor {
                missing.push(ByteRange::new(cursor, r.start - 1));
            }
            cursor = cursor.max(r.end + 1);
        }

        if cursor < total_size {
            missing.push(ByteRange::new(cursor, total_size - 1));
        }

        Ok(missing)
    }

    pub fn list_jobs(&self) -> Result<Vec<JobRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, url, filename, destination, total_size, downloaded_bytes,
                    status, resumable, etag, last_modified, expected_sha256,
                    computed_sha256, error, created_at, completed_at
             FROM jobs ORDER BY created_at DESC;",
        )?;

        let rows = stmt.query_map([], |row| {
            let status_str: String = row.get(6)?;
            let status = match status_str.as_str() {
                "Active" => DownloadStatus::Active,
                "Paused" => DownloadStatus::Paused,
                "WaitingAuth" => DownloadStatus::WaitingAuth,
                "WaitingDisk" => DownloadStatus::WaitingDisk,
                "Completed" => DownloadStatus::Completed,
                "Verifying" => DownloadStatus::Verifying,
                "Error" => DownloadStatus::Error,
                _ => DownloadStatus::Queued,
            };

            let total_size: Option<i64> = row.get(4)?;
            let downloaded_bytes: i64 = row.get(5)?;
            let resumable_int: i32 = row.get(7)?;

            Ok(JobRecord {
                id: row.get(0)?,
                url: row.get(1)?,
                filename: row.get(2)?,
                destination: row.get(3)?,
                total_size: total_size.map(|s| s as u64),
                downloaded_bytes: downloaded_bytes as u64,
                status,
                resumable: resumable_int != 0,
                etag: row.get(8)?,
                last_modified: row.get(9)?,
                expected_sha256: row.get(10)?,
                computed_sha256: row.get(11)?,
                error: row.get(12)?,
                created_at: row.get(13)?,
                completed_at: row.get(14)?,
            })
        })?;

        let mut jobs = Vec::new();
        for j in rows {
            jobs.push(j?);
        }
        Ok(jobs)
    }

    pub fn get_job(&self, job_id: &str) -> Result<Option<JobRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, url, filename, destination, total_size, downloaded_bytes,
                    status, resumable, etag, last_modified, expected_sha256,
                    computed_sha256, error, created_at, completed_at
             FROM jobs WHERE id = ?1;",
        )?;

        let mut rows = stmt.query(params![job_id])?;
        if let Some(row) = rows.next()? {
            let status_str: String = row.get(6)?;
            let status = match status_str.as_str() {
                "Active" => DownloadStatus::Active,
                "Paused" => DownloadStatus::Paused,
                "WaitingAuth" => DownloadStatus::WaitingAuth,
                "WaitingDisk" => DownloadStatus::WaitingDisk,
                "Completed" => DownloadStatus::Completed,
                "Verifying" => DownloadStatus::Verifying,
                "Error" => DownloadStatus::Error,
                _ => DownloadStatus::Queued,
            };

            let total_size: Option<i64> = row.get(4)?;
            let downloaded_bytes: i64 = row.get(5)?;
            let resumable_int: i32 = row.get(7)?;

            Ok(Some(JobRecord {
                id: row.get(0)?,
                url: row.get(1)?,
                filename: row.get(2)?,
                destination: row.get(3)?,
                total_size: total_size.map(|s| s as u64),
                downloaded_bytes: downloaded_bytes as u64,
                status,
                resumable: resumable_int != 0,
                etag: row.get(8)?,
                last_modified: row.get(9)?,
                expected_sha256: row.get(10)?,
                computed_sha256: row.get(11)?,
                error: row.get(12)?,
                created_at: row.get(13)?,
                completed_at: row.get(14)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn update_status(
        &self,
        job_id: &str,
        status: DownloadStatus,
        error: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE jobs SET status = ?1, error = ?2 WHERE id = ?3;",
            params![status.to_string(), error, job_id],
        )?;
        Ok(())
    }

    pub fn update_downloaded_bytes(&self, job_id: &str, downloaded: u64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE jobs SET downloaded_bytes = ?1 WHERE id = ?2;",
            params![downloaded as i64, job_id],
        )?;
        Ok(())
    }

    pub fn update_job_url(&self, job_id: &str, new_url: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE jobs SET url = ?1 WHERE id = ?2;",
            params![new_url, job_id],
        )?;
        Ok(())
    }

    pub fn delete_job(&self, job_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM jobs WHERE id = ?1;", params![job_id])?;
        conn.execute(
            "DELETE FROM completed_ranges WHERE job_id = ?1;",
            params![job_id],
        )?;
        Ok(())
    }
}

fn merge_adjacent_ranges(ranges: &[ByteRange]) -> Vec<ByteRange> {
    if ranges.is_empty() {
        return Vec::new();
    }

    let mut sorted = ranges.to_vec();
    sorted.sort_by_key(|r| r.start);

    let mut merged = Vec::new();
    let mut current = sorted[0];

    for next in &sorted[1..] {
        if next.start <= current.end + 1 {
            current.end = current.end.max(next.end);
        } else {
            merged.push(current);
            current = *next;
        }
    }
    merged.push(current);
    merged
}
