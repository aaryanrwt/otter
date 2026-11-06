use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64, // inclusive
}

impl ByteRange {
    pub fn new(start: u64, end: u64) -> Self {
        assert!(start <= end, "start must be <= end");
        Self { start, end }
    }

    pub fn len(&self) -> u64 {
        self.end.saturating_sub(self.start) + 1
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn contains(&self, offset: u64) -> bool {
        offset >= self.start && offset <= self.end
    }

    pub fn overlaps(&self, other: &ByteRange) -> bool {
        self.start <= other.end && other.start <= self.end
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadStatus {
    Queued,
    Active,
    Paused,
    WaitingAuth,
    WaitingDisk,
    Completed,
    Verifying,
    Error,
}

impl std::fmt::Display for DownloadStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Queued => write!(f, "Queued"),
            Self::Active => write!(f, "Active"),
            Self::Paused => write!(f, "Paused"),
            Self::WaitingAuth => write!(f, "WaitingAuth"),
            Self::WaitingDisk => write!(f, "WaitingDisk"),
            Self::Completed => write!(f, "Completed"),
            Self::Verifying => write!(f, "Verifying"),
            Self::Error => write!(f, "Error"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteFileInfo {
    pub url: String,
    pub filename: String,
    pub total_size: Option<u64>,
    pub resumable: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressSnapshot {
    pub job_id: String,
    pub filename: String,
    pub destination_path: String,
    pub total_bytes: Option<u64>,
    pub downloaded_bytes: u64,
    pub verified_bytes: u64,
    pub speed_bps: f64,
    pub eta_seconds: Option<u64>,
    pub status: DownloadStatus,
    pub resumable: bool,
    pub connections: usize,
    pub error_message: Option<String>,
}

impl ProgressSnapshot {
    pub fn percentage(&self) -> f64 {
        match self.total_bytes {
            Some(total) if total > 0 => {
                let pct = (self.downloaded_bytes as f64 / total as f64) * 100.0;
                pct.clamp(0.0, 100.0)
            }
            _ => 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: String,
    pub url: String,
    pub filename: String,
    pub destination: String,
    pub total_size: Option<u64>,
    pub downloaded_bytes: u64,
    pub status: DownloadStatus,
    pub resumable: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub expected_sha256: Option<String>,
    pub computed_sha256: Option<String>,
    pub error: Option<String>,
    pub created_at: i64,
    pub completed_at: Option<i64>,
}
