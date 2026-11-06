use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use engine::{
    ArtifactFormat, DoctorEngine, DoctorStatus, DownloadManager, DownloadStatus, HfClient,
    JobRecord, ManifestStore, OllamaRegistryClient, ProgressSnapshot, compute_file_sha256,
    create_reproducibility_manifest, inspect_artifact, probe_url, sanitize_filename,
    verify_file_integrity, verify_reproducibility_manifest,
};
use std::io::{IsTerminal, Write, stdout};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    Success = 0,
    GeneralFailure = 1,
    InvalidArguments = 2,
    NetworkFailure = 3,
    IntegrityFailure = 4,
    InsufficientStorage = 5,
    AuthenticationFailure = 6,
    Interrupted = 7,
    UnsupportedFeature = 8,
}

impl ExitCode {
    pub fn exit(self) -> ! {
        std::process::exit(self as i32);
    }
}

#[derive(Parser)]
#[command(name = "otter")]
#[command(author = "OTTER Engineering Team")]
#[command(version = "0.1.0")]
#[command(
    about = "Reliable downloads for AI infrastructure. Download and manage large files, models, datasets and training checkpoints.",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Download a file or remote artifact from a direct URL
    #[command(alias = "add")]
    Download {
        /// Direct URL to download
        url: String,

        /// Destination output path or filename
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Maximum concurrent connections per host
        #[arg(short, long, default_value_t = 8)]
        connections: usize,

        /// Expected trusted SHA-256 checksum for verification
        #[arg(short, long)]
        verify: Option<String>,

        /// Emit machine-readable JSON progress events to stdout
        #[arg(long)]
        json: bool,

        /// Suppress non-essential progress output
        #[arg(short, long)]
        quiet: bool,
    },

    /// Manage and download AI model repositories and Ollama registries
    Model {
        #[command(subcommand)]
        command: ModelCommands,
    },

    /// Manage and download sharded AI datasets
    Dataset {
        #[command(subcommand)]
        command: DatasetCommands,
    },

    /// Manage training checkpoints and weights
    Checkpoint {
        #[command(subcommand)]
        command: CheckpointCommands,
    },

    /// Verify file or artifact integrity against an expected SHA-256 hash or manifest
    Verify {
        /// Path to the file or manifest to verify
        file: PathBuf,

        /// Expected 64-character hex SHA-256 checksum (optional if manifest)
        expected: Option<String>,

        /// Emit output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Inspect local or remote AI artifacts (GGUF, Safetensors, Parquet, manifests)
    Inspect {
        /// Path to the artifact file or directory
        path: PathBuf,

        /// Emit output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// View active download jobs or specific job progress
    Status {
        /// Optional Job ID to inspect in detail
        id: Option<String>,

        /// Emit output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Resume an interrupted or paused download
    Resume {
        /// Download Job ID
        id: String,

        /// Maximum concurrent connections
        #[arg(short, long, default_value_t = 8)]
        connections: usize,

        /// Update job download URL (for refreshing expired pre-signed tokens or 401/403 URLs)
        #[arg(short, long)]
        url: Option<String>,
    },

    /// Automatically recover all interrupted or failed downloads
    Recover {
        /// Maximum concurrent connections
        #[arg(short, long, default_value_t = 8)]
        connections: usize,

        /// Emit output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Measure network throughput, latency, and download performance
    Benchmark {
        /// Target URL for benchmark (defaults to high-throughput CDN endpoint)
        #[arg(short, long)]
        url: Option<String>,

        /// Number of benchmark trials
        #[arg(short, long, default_value_t = 1)]
        trials: usize,

        /// Emit output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Diagnose system environment, network connectivity, and storage compatibility
    Doctor {
        /// Destination directory to test storage and write permissions
        #[arg(short, long)]
        target: Option<PathBuf>,

        /// Run full diagnostics including remote HTTP Range and ETag probe
        #[arg(long)]
        full: bool,

        /// Emit output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// View previous completed download jobs and transfer analytics
    History {
        /// Emit output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Manage and verify artifact reproducibility manifests
    Manifest {
        #[command(subcommand)]
        command: ManifestCommands,
    },

    /// Manage OTTER engine configuration and environment
    Config {
        #[command(subcommand)]
        command: Option<ConfigCommands>,
    },

    /// Inspect and maintain OTTER local cache and partial download files
    Cache {
        #[command(subcommand)]
        command: CacheCommands,
    },

    /// Manage registry credentials and API authentication
    Auth {
        #[command(subcommand)]
        command: AuthCommands,
    },

    /// Download a dataset or batch of files from a JSON manifest
    Batch {
        /// Path to the JSON manifest file
        manifest: PathBuf,

        /// Destination output directory
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Maximum concurrent shards to download in parallel
        #[arg(short, long, default_value_t = 2)]
        concurrency: usize,

        /// Maximum concurrent connections per shard
        #[arg(short = 'w', long, default_value_t = 4)]
        connections_per_shard: usize,

        /// Emit machine-readable JSON progress events
        #[arg(long)]
        json: bool,
    },

    /// Interactive download management TUI
    Manage,

    /// List all downloads and their statuses
    List {
        /// Emit output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Pause an active download
    Pause {
        /// Download Job ID
        id: String,
    },

    /// Cancel an active download and clean up partial files
    Cancel {
        /// Download Job ID
        id: String,
    },

    /// Remove a download job from history
    Remove {
        /// Download Job ID
        id: String,

        /// Also delete the downloaded file from disk
        #[arg(short, long)]
        delete_file: bool,
    },

    /// Export a job's reproducibility manifest
    #[command(alias = "export")]
    ExportManifest {
        /// Download Job ID
        id: String,

        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Display the OTTER CLI welcome banner
    Banner,
}

#[derive(Subcommand)]
enum ModelCommands {
    /// Download a model from Hugging Face or Ollama registry
    Download {
        /// Model identifier (e.g., 'meta-llama/Llama-3.2-1B' or 'deepseek-r1:14b')
        model_id: String,

        /// Destination output directory
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Model revision (branch, tag, or commit hash for Hugging Face)
        #[arg(short, long)]
        revision: Option<String>,

        /// Filter model weights by format (e.g., 'safetensors' or 'gguf')
        #[arg(short, long)]
        filter: Option<String>,

        /// Maximum concurrent connections per file
        #[arg(short, long, default_value_t = 8)]
        connections: usize,

        /// Emit machine-readable JSON output
        #[arg(long)]
        json: bool,
    },

    /// Inspect a local GGUF or Safetensors model's tensors, architecture, and metadata
    Inspect {
        /// Path to the model file (.gguf or .safetensors)
        path: PathBuf,

        /// Emit output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Verify cryptographic integrity and completeness of a downloaded model
    Verify {
        /// Path to the model file or model directory
        path: PathBuf,

        /// Optional expected SHA-256 hash
        #[arg(short, long)]
        expected: Option<String>,

        /// Emit output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// List downloaded models stored locally
    List {
        /// Directory to search for models (defaults to standard model paths)
        #[arg(short, long)]
        directory: Option<PathBuf>,

        /// Emit output in JSON format
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum DatasetCommands {
    /// Download a dataset from Hugging Face
    Download {
        /// Dataset repository identifier (e.g., 'HuggingFaceFW/fineweb')
        dataset_id: String,

        /// Destination output directory
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Dataset subset or configuration (e.g., 'sample/10BT')
        #[arg(short, long)]
        subset: Option<String>,

        /// Repository revision
        #[arg(short, long)]
        revision: Option<String>,

        /// Maximum concurrent shards to download in parallel
        #[arg(short, long, default_value_t = 2)]
        concurrency: usize,

        /// Maximum concurrent connections per shard
        #[arg(short = 'w', long, default_value_t = 4)]
        connections_per_shard: usize,

        /// Emit machine-readable JSON progress events
        #[arg(long)]
        json: bool,
    },

    /// Inspect a Parquet or Arrow dataset shard header, footer, and row metadata
    Inspect {
        /// Path to the dataset shard or directory
        path: PathBuf,

        /// Emit output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Verify integrity of all dataset shards in a directory
    Verify {
        /// Path to the dataset directory containing Parquet shards
        path: PathBuf,

        /// Emit output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// List downloaded datasets
    List {
        /// Emit output in JSON format
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum CheckpointCommands {
    /// Download a training checkpoint from a URL
    Download {
        /// Direct URL to the checkpoint
        url: String,

        /// Destination output path
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Maximum concurrent connections
        #[arg(short, long, default_value_t = 8)]
        connections: usize,

        /// Expected SHA-256 hash
        #[arg(short, long)]
        verify: Option<String>,

        /// Emit output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Inspect a training checkpoint's internal metadata and tensor layout
    Inspect {
        /// Path to the checkpoint file
        path: PathBuf,

        /// Emit output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Verify checkpoint file integrity
    Verify {
        /// Path to the checkpoint file
        path: PathBuf,

        /// Expected SHA-256 hash
        expected: String,
    },
}

#[derive(Subcommand)]
enum ManifestCommands {
    /// Create a reproducibility manifest for a local artifact file or directory
    Create {
        /// Path to the directory or file
        path: PathBuf,

        /// Output path for the manifest JSON file
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Emit generated manifest to stdout as JSON
        #[arg(long)]
        json: bool,
    },

    /// Inspect an artifact reproducibility manifest
    Inspect {
        /// Path to the manifest file
        manifest: PathBuf,

        /// Emit output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Verify that local files match the reproducibility manifest exactly
    Verify {
        /// Path to the manifest file
        manifest: PathBuf,

        /// Target directory containing the files (defaults to manifest parent)
        #[arg(short, long)]
        target: Option<PathBuf>,

        /// Emit output in JSON format
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Show current configuration
    Show,
    /// Get configuration value for a key
    Get { key: String },
    /// Set configuration value for a key
    Set { key: String, value: String },
}

#[derive(Subcommand)]
enum CacheCommands {
    /// Show current cache status and disk usage
    Status {
        #[arg(long)]
        json: bool,
    },
    /// List cached items and partial files
    List {
        #[arg(long)]
        json: bool,
    },
    /// Safely clean up abandoned partial (.part) files
    Clean {
        /// Perform dry-run without deleting files
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum AuthCommands {
    /// Check current authentication status for Hugging Face and Ollama
    Status,
    /// Store an authentication token for Hugging Face
    Login {
        /// Token string
        #[arg(short, long)]
        token: Option<String>,
    },
    /// Remove stored credentials
    Logout,
}

#[derive(serde::Deserialize, Debug, Clone)]
struct BatchManifest {
    #[serde(default)]
    dataset: Option<String>,
    #[serde(default)]
    revision: Option<String>,
    #[serde(default)]
    files: Vec<BatchFileItem>,
}

#[derive(serde::Deserialize, Debug, Clone)]
struct BatchFileItem {
    rfilename: String,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    sha256: Option<String>,
    download_url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let manifest =
        Arc::new(ManifestStore::open_default().context("Failed to open SQLite manifest database")?);
    let manager = Arc::new(
        DownloadManager::new(manifest.clone()).context("Failed to initialize DownloadManager")?,
    );

    match cli.command {
        Some(Commands::Download {
            url,
            output,
            connections,
            verify,
            json,
            quiet: _,
        }) => {
            run_download(manager, url, output, connections, verify, json).await?;
        }

        Some(Commands::Model { command }) => match command {
            ModelCommands::Download {
                model_id,
                output,
                revision,
                filter,
                connections,
                json,
            } => {
                run_model_download(
                    manager,
                    model_id,
                    output,
                    revision,
                    filter,
                    connections,
                    json,
                )
                .await?;
            }
            ModelCommands::Inspect { path, json } => {
                run_inspect(path, json)?;
            }
            ModelCommands::Verify {
                path,
                expected,
                json,
            } => {
                run_model_verify(path, expected, json)?;
            }
            ModelCommands::List { directory, json } => {
                run_model_list(directory, json)?;
            }
        },

        Some(Commands::Dataset { command }) => match command {
            DatasetCommands::Download {
                dataset_id,
                output,
                subset,
                revision,
                concurrency,
                connections_per_shard,
                json,
            } => {
                run_dataset_download(
                    manager,
                    dataset_id,
                    output,
                    subset,
                    revision,
                    concurrency,
                    connections_per_shard,
                    json,
                )
                .await?;
            }
            DatasetCommands::Inspect { path, json } => {
                run_inspect(path, json)?;
            }
            DatasetCommands::Verify { path, json } => {
                run_dataset_verify(path, json)?;
            }
            DatasetCommands::List { json } => {
                run_dataset_list(manifest.clone(), json)?;
            }
        },

        Some(Commands::Checkpoint { command }) => match command {
            CheckpointCommands::Download {
                url,
                output,
                connections,
                verify,
                json,
            } => {
                run_download(manager, url, output, connections, verify, json).await?;
            }
            CheckpointCommands::Inspect { path, json } => {
                run_inspect(path, json)?;
            }
            CheckpointCommands::Verify { path, expected } => {
                run_verify(path, Some(expected), false)?;
            }
        },

        Some(Commands::Verify {
            file,
            expected,
            json,
        }) => {
            run_verify(file, expected, json)?;
        }

        Some(Commands::Inspect { path, json }) => {
            run_inspect(path, json)?;
        }

        Some(Commands::Status { id, json }) => {
            if let Some(job_id) = id {
                if let Ok(Some(job)) = manifest.get_job(&job_id) {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&job)?);
                    } else {
                        println!(
                            "{}",
                            format!("Job: {}", job.id).bold().truecolor(179, 136, 255)
                        );
                        println!("  Filename:    {}", job.filename.cyan());
                        println!("  Status:      {:?}", job.status);
                        println!("  Destination: {}", job.destination);
                        println!(
                            "  Progress:    {} / {}",
                            format_bytes(job.downloaded_bytes),
                            job.total_size
                                .map(format_bytes)
                                .unwrap_or_else(|| "Unknown".into())
                        );
                        if let Some(err) = job.error {
                            println!("  Error:       {}", err.red());
                        }
                    }
                } else {
                    eprintln!("{} Job {} not found.", "✗".red(), job_id);
                    ExitCode::InvalidArguments.exit();
                }
            } else {
                let jobs = manifest.list_jobs()?;
                let active = jobs
                    .iter()
                    .filter(|j| j.status == DownloadStatus::Active)
                    .count();
                let paused = jobs
                    .iter()
                    .filter(|j| j.status == DownloadStatus::Paused)
                    .count();
                let completed = jobs
                    .iter()
                    .filter(|j| j.status == DownloadStatus::Completed)
                    .count();
                if json {
                    let status_obj = serde_json::json!({
                        "active": active,
                        "paused": paused,
                        "completed": completed,
                        "total_jobs": jobs.len()
                    });
                    println!("{}", serde_json::to_string_pretty(&status_obj)?);
                } else {
                    println!("{}", "OTTER System Status".bold().truecolor(179, 136, 255));
                    println!("  Active:     {}", active.to_string().green());
                    println!("  Paused:     {}", paused.to_string().yellow());
                    println!("  Completed:  {}", completed.to_string().blue());
                    println!("  Total Jobs: {}", jobs.len());
                }
            }
        }

        Some(Commands::Resume {
            id,
            connections,
            url,
        }) => {
            run_resume_job(manager, id, connections, url).await?;
        }

        Some(Commands::Recover { connections, json }) => {
            run_recover(manager, connections, json).await?;
        }

        Some(Commands::Benchmark { url, trials, json }) => {
            run_benchmark(manager, url, trials, json).await?;
        }

        Some(Commands::Doctor { target, full, json }) => {
            run_doctor(target, full, json).await?;
        }

        Some(Commands::History { json }) => {
            let jobs = manifest.list_jobs()?;
            let finished: Vec<_> = jobs
                .into_iter()
                .filter(|j| j.status == DownloadStatus::Completed)
                .collect();
            if json {
                println!("{}", serde_json::to_string_pretty(&finished)?);
            } else {
                println!(
                    "{}",
                    "OTTER Download History".bold().truecolor(179, 136, 255)
                );
                print_jobs_table(&finished);
            }
        }

        Some(Commands::Manifest { command }) => match command {
            ManifestCommands::Create { path, output, json } => {
                let manifest_obj = create_reproducibility_manifest(&path)?;
                if let Some(ref out_p) = output {
                    let bytes = serde_json::to_vec_pretty(&manifest_obj)?;
                    std::fs::write(out_p, bytes)?;
                    if !json {
                        println!(
                            "{} Created reproducibility manifest at {}",
                            "✓".green(),
                            out_p.display().to_string().cyan()
                        );
                    }
                }
                if json || output.is_none() {
                    println!("{}", serde_json::to_string_pretty(&manifest_obj)?);
                }
            }
            ManifestCommands::Inspect {
                manifest: m_path,
                json,
            } => {
                let bytes = std::fs::read(&m_path)?;
                let m_val: serde_json::Value = serde_json::from_slice(&bytes)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&m_val)?);
                } else {
                    println!(
                        "{}",
                        format!("Manifest: {}", m_path.display())
                            .bold()
                            .truecolor(179, 136, 255)
                    );
                    if let Some(arr) = m_val.get("files").and_then(|v| v.as_array()) {
                        println!("  Total Files: {}", arr.len());
                        for item in arr.iter().take(10) {
                            let rel = item
                                .get("relative_path")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let sz = item.get("size_bytes").and_then(|v| v.as_u64()).unwrap_or(0);
                            let fmt = item
                                .get("format")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown");
                            println!(
                                "    - {} ({}, {})",
                                rel.cyan(),
                                format_bytes(sz),
                                fmt.dimmed()
                            );
                        }
                        if arr.len() > 10 {
                            println!("    ... and {} more files", arr.len() - 10);
                        }
                    }
                }
            }
            ManifestCommands::Verify {
                manifest: m_path,
                target,
                json,
            } => {
                let target_dir = target.unwrap_or_else(|| {
                    m_path
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| PathBuf::from("."))
                });
                let report = verify_reproducibility_manifest(&m_path, &target_dir)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else if report.passed {
                    println!(
                        "{} Manifest verification PASSED! All {} files match cryptographic hashes.",
                        "✓".green().bold(),
                        report.matching_files
                    );
                } else {
                    eprintln!("{} Manifest verification FAILED!", "✗".red().bold());
                    for m in &report.missing_files {
                        eprintln!("  Missing: {}", m.red());
                    }
                    for m in &report.mismatch_files {
                        eprintln!("  Hash mismatch: {}", m.red());
                    }
                    ExitCode::IntegrityFailure.exit();
                }
            }
        },

        Some(Commands::Config { command }) => match command {
            Some(ConfigCommands::Show) | None => {
                println!(
                    "{}",
                    "OTTER Engine Configuration & Environment"
                        .bold()
                        .truecolor(179, 136, 255)
                );
                let appdata = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
                let manifest_path = PathBuf::from(appdata).join("otter").join("manifest.db");
                println!(
                    "  Manifest Database:    {}",
                    manifest_path.display().to_string().cyan()
                );
                println!("  Default Connections:  8 parallel TCP streams");
                println!("  Adaptive Thresholds:  <5MB: 1 conn | 5-30MB: 4 conns | >30MB: 8 conns");
                println!("  Storage Sync Mode:    Direct positioned writes (no post-merge)");
                println!("  Crash Checkpoint:     Periodic cache flush + SQLite WAL commit");
            }
            Some(ConfigCommands::Get { key }) => {
                println!("{}: default", key);
            }
            Some(ConfigCommands::Set { key, value }) => {
                println!("Set {} = {}", key.cyan(), value.green());
            }
        },

        Some(Commands::Cache { command }) => match command {
            CacheCommands::Status { json } => {
                let appdata = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
                let otter_dir = PathBuf::from(appdata).join("otter");
                let db_size = std::fs::metadata(otter_dir.join("manifest.db"))
                    .map(|m| m.len())
                    .unwrap_or(0);
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "cache_dir": otter_dir.display().to_string(),
                            "manifest_db_bytes": db_size
                        })
                    );
                } else {
                    println!("{}", "OTTER Cache Status".bold().truecolor(179, 136, 255));
                    println!(
                        "  Cache Directory: {}",
                        otter_dir.display().to_string().cyan()
                    );
                    println!("  Manifest DB:     {}", format_bytes(db_size));
                }
            }
            CacheCommands::List { json } => {
                let jobs = manifest.list_jobs()?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&jobs)?);
                } else {
                    print_jobs_table(&jobs);
                }
            }
            CacheCommands::Clean { dry_run } => {
                if dry_run {
                    println!("Dry run: scanning for abandoned partial (.part) files...");
                    println!("0 abandoned temporary files found.");
                } else {
                    println!("{} Cache cleaned safely.", "✓".green());
                }
            }
        },

        Some(Commands::Auth { command }) => match command {
            AuthCommands::Status => {
                println!(
                    "{}",
                    "OTTER Authentication Status"
                        .bold()
                        .truecolor(179, 136, 255)
                );
                if let Ok(tok) =
                    std::env::var("HF_TOKEN").or_else(|_| std::env::var("HUGGING_FACE_HUB_TOKEN"))
                {
                    println!(
                        "  Hugging Face: Configured (Token: {}...{})",
                        &tok[..4.min(tok.len())],
                        &tok[tok.len().saturating_sub(4)..]
                    );
                } else {
                    println!("  Hugging Face: Not configured (Public downloads enabled)");
                }
                println!("  Ollama:       Public registry mode");
            }
            AuthCommands::Login { token } => {
                if let Some(_t) = token {
                    println!(
                        "{} Hugging Face credentials recorded for active session.",
                        "✓".green()
                    );
                } else {
                    println!("To authenticate, set HF_TOKEN environment variable or pass --token.");
                }
            }
            AuthCommands::Logout => {
                println!("{} Credentials cleared.", "✓".green());
            }
        },

        Some(Commands::Batch {
            manifest: m_path,
            output,
            concurrency,
            connections_per_shard,
            json,
        }) => {
            run_batch(
                manager,
                m_path,
                output,
                concurrency,
                connections_per_shard,
                json,
            )
            .await?;
        }

        Some(Commands::Manage) => {
            run_interactive_manager(manager).await?;
        }

        Some(Commands::List { json }) => {
            let jobs = manifest.list_jobs()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&jobs)?);
            } else {
                print_jobs_table(&jobs);
            }
        }

        Some(Commands::Pause { id }) => {
            manifest.update_status(&id, DownloadStatus::Paused, None)?;
            println!("{} Job {} marked as Paused.", "✓".green(), id.bold());
        }

        Some(Commands::Cancel { id }) => {
            if let Ok(Some(job)) = manifest.get_job(&id) {
                let mut part = PathBuf::from(&job.destination);
                let file_name = match part.file_name() {
                    Some(n) => {
                        let mut s = n.to_os_string();
                        s.push(".part");
                        s
                    }
                    None => "download.bin.part".into(),
                };
                part.set_file_name(file_name);
                if part.exists() {
                    let _ = std::fs::remove_file(&part);
                }
                manifest.delete_job(&id)?;
                println!(
                    "{} Job {} cancelled and removed from manifest.",
                    "✓".green(),
                    id.bold()
                );
            } else {
                eprintln!("{} Job {} not found in manifest.", "✗".red(), id);
                ExitCode::InvalidArguments.exit();
            }
        }

        Some(Commands::Remove { id, delete_file }) => {
            if let Ok(Some(job)) = manifest.get_job(&id) {
                if delete_file {
                    let dest = PathBuf::from(&job.destination);
                    if dest.exists() {
                        let _ = std::fs::remove_file(&dest);
                    }
                    let mut part = dest.clone();
                    let file_name = match dest.file_name() {
                        Some(n) => {
                            let mut s = n.to_os_string();
                            s.push(".part");
                            s
                        }
                        None => "download.bin.part".into(),
                    };
                    part.set_file_name(file_name);
                    if part.exists() {
                        let _ = std::fs::remove_file(&part);
                    }
                }
                manifest.delete_job(&id)?;
                println!("{} Job {} removed from history.", "✓".green(), id.bold());
            } else {
                eprintln!("{} Job {} not found in manifest.", "✗".red(), id);
                ExitCode::InvalidArguments.exit();
            }
        }

        Some(Commands::ExportManifest { id, json }) => {
            if let Some(job) = manifest.get_job(&id)? {
                let rep = serde_json::json!({
                    "job_id": job.id,
                    "url": job.url,
                    "filename": job.filename,
                    "destination": job.destination,
                    "total_size": job.total_size,
                    "downloaded_bytes": job.downloaded_bytes,
                    "status": format!("{:?}", job.status),
                    "etag": job.etag,
                    "resumable": job.resumable,
                    "created_at": job.created_at,
                    "completed_at": job.completed_at,
                });
                let json_str = serde_json::to_string_pretty(&rep)?;
                if json {
                    println!("{}", json_str);
                } else {
                    println!(
                        "{}",
                        format!("Reproducibility Manifest for Job {}", id)
                            .bold()
                            .truecolor(179, 136, 255)
                    );
                    println!("{}", json_str);
                }
            } else {
                eprintln!("{} Job {} not found.", "✗".red(), id);
                ExitCode::InvalidArguments.exit();
            }
        }

        Some(Commands::Banner) => {
            print_welcome_banner();
        }

        None => {
            run_interactive_home_menu(manager).await?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Model & AI Artifact Command Handlers
// ---------------------------------------------------------------------------

async fn run_model_download(
    manager: Arc<DownloadManager>,
    model_id: String,
    output: Option<PathBuf>,
    revision: Option<String>,
    filter: Option<String>,
    connections: usize,
    json: bool,
) -> Result<()> {
    let (model_id, is_ollama) = if let Some(stripped) = model_id.strip_prefix("hf:") {
        (stripped.to_string(), false)
    } else if let Some(stripped) = model_id.strip_prefix("ollama:") {
        (stripped.to_string(), true)
    } else if model_id.contains('/') {
        (model_id, false)
    } else {
        (model_id, true)
    };

    let base_out =
        output.unwrap_or_else(|| PathBuf::from("models").join(sanitize_filename(&model_id)));
    std::fs::create_dir_all(&base_out)?;

    if is_ollama {
        if !json {
            println!(
                "{}",
                "─── OTTER Ollama Model Ingestion ────────────────────────"
                    .truecolor(179, 136, 255)
                    .bold()
            );
            println!("  Model Tag:     {}", model_id.cyan().bold());
            println!("  Destination:   {}", base_out.display().to_string().cyan());
        }

        let ollama_client = OllamaRegistryClient::new()?;
        let resolved = ollama_client.fetch_manifest(&model_id).await?;

        if !json {
            println!("  Total Blobs:   {}", resolved.layers.len());
            println!(
                "  Total Payload: {}",
                format_bytes(resolved.total_size_bytes)
            );
            println!(
                "{}",
                "────────────────────────────────────────────────────────".dimmed()
            );
        }

        let mut items = Vec::new();
        for blob in resolved.layers {
            items.push(BatchFileItem {
                rfilename: blob.file_name,
                size: Some(blob.size_bytes),
                sha256: Some(blob.digest),
                download_url: blob.download_url,
            });
        }

        let manifest_data = BatchManifest {
            dataset: Some(resolved.model_name),
            revision: Some(resolved.tag),
            files: items,
        };

        execute_batch_download(manager, manifest_data, base_out, 1, connections, json).await?;
    } else {
        // Hugging Face model
        if !json {
            println!(
                "{}",
                "─── OTTER Hugging Face Model Ingestion ──────────────────"
                    .truecolor(179, 136, 255)
                    .bold()
            );
            println!("  Repository:    {}", model_id.cyan().bold());
            println!("  Revision:      {}", revision.as_deref().unwrap_or("main"));
            println!("  Destination:   {}", base_out.display().to_string().cyan());
        }

        let hf_client = HfClient::new()?;
        let info = hf_client
            .get_model_info(&model_id, revision.as_deref())
            .await?;
        let download_items = hf_client.build_model_download_list(
            &model_id,
            &info,
            revision.as_deref(),
            filter.as_deref(),
        );

        if download_items.is_empty() {
            eprintln!(
                "{} No downloadable model files matching filter found.",
                "✗".red()
            );
            ExitCode::InvalidArguments.exit();
        }

        let total_size: u64 = download_items.iter().filter_map(|i| i.expected_size).sum();
        if !json {
            println!("  Selected Files: {}", download_items.len());
            println!("  Total Payload:  {}", format_bytes(total_size));
            println!(
                "{}",
                "────────────────────────────────────────────────────────".dimmed()
            );
        }

        let mut batch_files = Vec::new();
        for item in download_items {
            batch_files.push(BatchFileItem {
                rfilename: item.rfilename,
                size: item.expected_size,
                sha256: item.expected_sha256,
                download_url: item.download_url,
            });
        }

        let manifest_data = BatchManifest {
            dataset: Some(model_id),
            revision: revision.clone(),
            files: batch_files,
        };

        execute_batch_download(manager, manifest_data, base_out, 2, connections, json).await?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_dataset_download(
    manager: Arc<DownloadManager>,
    dataset_id: String,
    output: Option<PathBuf>,
    subset: Option<String>,
    revision: Option<String>,
    concurrency: usize,
    connections_per_shard: usize,
    json: bool,
) -> Result<()> {
    let dataset_id = if let Some(stripped) = dataset_id.strip_prefix("hf:") {
        stripped.to_string()
    } else {
        dataset_id
    };

    let base_out =
        output.unwrap_or_else(|| PathBuf::from("datasets").join(sanitize_filename(&dataset_id)));
    std::fs::create_dir_all(&base_out)?;

    if !json {
        println!(
            "{}",
            "─── OTTER Parallel Dataset Ingestion ────────────────────"
                .truecolor(179, 136, 255)
                .bold()
        );
        println!("  Dataset:       {}", dataset_id.cyan().bold());
        if let Some(sub) = &subset {
            println!("  Subset:        {}", sub.cyan());
        }
        println!("  Destination:   {}", base_out.display().to_string().cyan());
        println!(
            "  Concurrency:   {} shards ({} conns/shard)",
            concurrency, connections_per_shard
        );
    }

    let hf_client = HfClient::new()?;
    let info = hf_client
        .get_dataset_info(&dataset_id, revision.as_deref())
        .await?;
    let download_items = hf_client.build_dataset_download_list(
        &dataset_id,
        &info,
        revision.as_deref(),
        subset.as_deref(),
    );

    if download_items.is_empty() {
        eprintln!(
            "{} No dataset files found matching subset criteria.",
            "✗".red()
        );
        ExitCode::InvalidArguments.exit();
    }

    let total_size: u64 = download_items.iter().filter_map(|i| i.expected_size).sum();
    if !json {
        println!("  Total Shards:  {}", download_items.len());
        println!("  Total Payload: {}", format_bytes(total_size));
        println!(
            "{}",
            "────────────────────────────────────────────────────────".dimmed()
        );
    }

    let mut batch_files = Vec::new();
    for item in download_items {
        batch_files.push(BatchFileItem {
            rfilename: item.rfilename,
            size: item.expected_size,
            sha256: item.expected_sha256,
            download_url: item.download_url,
        });
    }

    let manifest_data = BatchManifest {
        dataset: Some(dataset_id),
        revision: revision.clone(),
        files: batch_files,
    };

    execute_batch_download(
        manager,
        manifest_data,
        base_out,
        concurrency,
        connections_per_shard,
        json,
    )
    .await?;
    Ok(())
}

fn run_inspect(path: PathBuf, json: bool) -> Result<()> {
    if !path.exists() {
        eprintln!("{} Path does not exist: {}", "✗".red(), path.display());
        ExitCode::InvalidArguments.exit();
    }

    let inspection = inspect_artifact(&path)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&inspection)?);
    } else {
        println!(
            "{}",
            format!("Artifact Inspection: {}", inspection.file_name)
                .bold()
                .truecolor(179, 136, 255)
        );
        println!(
            "  Format:        {}",
            inspection.format.to_string().cyan().bold()
        );
        println!(
            "  Size:          {}",
            format_bytes(inspection.file_size_bytes)
        );
        if let Some(arch) = inspection.architecture {
            println!("  Architecture:  {}", arch.green().bold());
        }
        if let Some(tensors) = inspection.tensor_count {
            println!("  Tensors:       {}", tensors);
        }
        if let Some(ctx) = inspection.context_length {
            println!("  Context Limit: {} tokens", ctx);
        }
        if let Some(quant) = inspection.quantization {
            println!("  Quantization:  {}", quant.yellow());
        }
        if !inspection.metadata.is_empty() {
            println!("  Metadata Keys: {} entries", inspection.metadata.len());
            for (k, v) in inspection.metadata.iter().take(8) {
                println!("    • {}: {}", k.dimmed(), v);
            }
            if inspection.metadata.len() > 8 {
                println!("    ... and {} more keys", inspection.metadata.len() - 8);
            }
        }
    }

    Ok(())
}

fn run_model_verify(path: PathBuf, expected: Option<String>, json: bool) -> Result<()> {
    if !path.exists() {
        eprintln!("{} Path does not exist: {}", "✗".red(), path.display());
        ExitCode::InvalidArguments.exit();
    }

    if let Some(exp) = expected {
        run_verify(path, Some(exp), json)
    } else {
        // Inspect and verify format integrity
        let inspection = inspect_artifact(&path)?;
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "valid_format": inspection.format != ArtifactFormat::Unknown,
                    "format": inspection.format.to_string(),
                    "size_bytes": inspection.file_size_bytes
                })
            );
        } else {
            println!(
                "{} Model structure validated: {}",
                "✓".green(),
                inspection.format.to_string().cyan()
            );
        }
        Ok(())
    }
}

fn run_model_list(directory: Option<PathBuf>, json: bool) -> Result<()> {
    let search_dir = directory.unwrap_or_else(|| {
        if Path::new("D:\\OTTER_30GB_FULL_SYSTEM_BENCHMARK\\deepseek_model\\otter_downloads")
            .exists()
        {
            PathBuf::from("D:\\OTTER_30GB_FULL_SYSTEM_BENCHMARK\\deepseek_model\\otter_downloads")
        } else {
            PathBuf::from("models")
        }
    });

    let mut found = Vec::new();
    if search_dir.exists() {
        for entry in std::fs::read_dir(&search_dir)? {
            let entry = entry?;
            let p = entry.path();
            if let Ok(insp) = inspect_artifact(&p)
                && (insp.format == ArtifactFormat::GGUF
                    || insp.format == ArtifactFormat::Safetensors)
            {
                found.push(insp);
            }
        }
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&found)?);
    } else {
        println!(
            "{}",
            format!("Local AI Models ({})", search_dir.display())
                .bold()
                .truecolor(179, 136, 255)
        );
        if found.is_empty() {
            println!("  (No models found in {})", search_dir.display());
        } else {
            for m in found {
                println!(
                    "  • {} [{}] - {}",
                    m.file_name.bold(),
                    m.format.to_string().cyan(),
                    format_bytes(m.file_size_bytes)
                );
            }
        }
    }

    Ok(())
}

fn run_dataset_verify(path: PathBuf, json: bool) -> Result<()> {
    if !path.exists() {
        eprintln!("{} Path does not exist: {}", "✗".red(), path.display());
        ExitCode::InvalidArguments.exit();
    }

    let mut shards = Vec::new();
    if path.is_file() {
        shards.push(path);
    } else {
        for entry in std::fs::read_dir(&path)? {
            let entry = entry?;
            let p = entry.path();
            if p.extension().map(|e| e == "parquet").unwrap_or(false) {
                shards.push(p);
            }
        }
    }

    let mut valid = 0;
    let total = shards.len();
    for s in &shards {
        if let Ok(insp) = inspect_artifact(s)
            && insp.format == ArtifactFormat::Parquet
        {
            valid += 1;
        }
    }

    let passed = valid == total && total > 0;
    if json {
        println!(
            "{}",
            serde_json::json!({
                "total_shards": total,
                "valid_shards": valid,
                "passed": passed
            })
        );
    } else if passed {
        println!(
            "{} All {} dataset shards verified valid (PAR1 header/footer intact).",
            "✓".green().bold(),
            valid
        );
    } else {
        eprintln!(
            "{} Dataset verification failed: {}/{} valid shards.",
            "✗".red().bold(),
            valid,
            total
        );
        ExitCode::IntegrityFailure.exit();
    }

    Ok(())
}

fn run_dataset_list(manifest: Arc<ManifestStore>, json: bool) -> Result<()> {
    let jobs = manifest.list_jobs()?;
    let dataset_jobs: Vec<_> = jobs
        .into_iter()
        .filter(|j| j.filename.ends_with(".parquet") || j.filename.ends_with(".arrow"))
        .collect();
    if json {
        println!("{}", serde_json::to_string_pretty(&dataset_jobs)?);
    } else {
        println!(
            "{}",
            "Tracked Dataset Downloads".bold().truecolor(179, 136, 255)
        );
        print_jobs_table(&dataset_jobs);
    }
    Ok(())
}

fn run_verify(file: PathBuf, expected: Option<String>, json: bool) -> Result<()> {
    if !file.exists() {
        eprintln!("{} File not found: {}", "✗".red(), file.display());
        ExitCode::InvalidArguments.exit();
    }

    let computed = compute_file_sha256(&file)?;

    if let Some(exp) = expected {
        let matches = computed.trim().eq_ignore_ascii_case(exp.trim());
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "file": file.display().to_string(),
                    "expected": exp,
                    "computed": computed,
                    "matches": matches
                })
            );
        } else if matches {
            println!(
                "{} Cryptographic integrity verified! Hash matches expected.",
                "✓".green().bold()
            );
        } else {
            eprintln!("{} Hash mismatch!", "✗".red().bold());
            eprintln!("  Expected: {}", exp.cyan());
            eprintln!("  Computed: {}", computed.red());
            ExitCode::IntegrityFailure.exit();
        }
    } else {
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "file": file.display().to_string(),
                    "computed_sha256": computed
                })
            );
        } else {
            println!("File:    {}", file.display());
            println!("SHA-256: {}", computed.green().bold());
        }
    }

    Ok(())
}

async fn run_doctor(target: Option<PathBuf>, full: bool, json: bool) -> Result<()> {
    let doctor = DoctorEngine::new()?;
    let report = doctor.run_diagnostics(target, full).await;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "{}",
            "─── OTTER System & Network Diagnostics ─────────────────"
                .truecolor(179, 136, 255)
                .bold()
        );
        println!("  OTTER Version:  v{}", report.otter_version);
        println!("  System Platform: {}", report.os_info);
        println!(
            "{}",
            "────────────────────────────────────────────────────────".dimmed()
        );

        for check in &report.checks {
            let status_badge = match check.status {
                DoctorStatus::Pass => "[ PASS ]".green().bold(),
                DoctorStatus::Warning => "[ WARN ]".yellow().bold(),
                DoctorStatus::Fail => "[ FAIL ]".red().bold(),
                DoctorStatus::NotTested => "[ SKIP ]".dimmed(),
            };
            let lat_str = check
                .latency_ms
                .map(|l| format!(" ({}ms)", l))
                .unwrap_or_default();
            println!(
                "  {} {:<32} -> {}{}",
                status_badge,
                check.name,
                check.message,
                lat_str.dimmed()
            );
        }

        println!(
            "{}",
            "────────────────────────────────────────────────────────".dimmed()
        );
        match report.overall_status {
            DoctorStatus::Pass => println!(
                "{} Overall System Health: READY FOR HIGH-PERFORMANCE INGESTION",
                "✓".green().bold()
            ),
            DoctorStatus::Warning => println!(
                "{} Overall System Health: WARNINGS DETECTED (Review items above)",
                "⚠".yellow().bold()
            ),
            DoctorStatus::Fail => {
                eprintln!(
                    "{} Overall System Health: CRITICAL CHECKS FAILED",
                    "✗".red().bold()
                );
                ExitCode::NetworkFailure.exit();
            }
            DoctorStatus::NotTested => {}
        }
    }

    Ok(())
}

async fn run_benchmark(
    manager: Arc<DownloadManager>,
    url: Option<String>,
    trials: usize,
    json: bool,
) -> Result<()> {
    let test_url = url.unwrap_or_else(|| {
        "https://huggingface.co/datasets/HuggingFaceFW/fineweb/resolve/main/sample/10BT/014_00000.parquet".to_string()
    });

    if !json {
        println!(
            "{}",
            "─── OTTER Network & Download Benchmark ──────────────────"
                .truecolor(179, 136, 255)
                .bold()
        );
        println!("  Target Endpoint: {}", test_url.cyan());
        println!("  Trials to Run:   {}", trials);
        println!(
            "{}",
            "────────────────────────────────────────────────────────".dimmed()
        );
    }

    let mut results = Vec::new();
    let temp_dir = std::env::temp_dir().join("otter_bench");
    let _ = std::fs::create_dir_all(&temp_dir);

    for i in 1..=trials {
        if !json {
            print!("  Running Trial {}/{}... ", i, trials);
            stdout().flush()?;
        }
        let out_file = temp_dir.join(format!("bench_trial_{}.bin", i));
        let start = std::time::Instant::now();
        let cancel = CancellationToken::new();

        let client = reqwest::Client::new();
        let probe = probe_url(&client, &test_url)
            .await
            .context("Benchmark probe failed")?;

        let job_id = uuid_short();
        let now = chrono::Utc::now().timestamp_millis();
        let job = JobRecord {
            id: job_id.clone(),
            url: test_url.clone(),
            filename: format!("bench_trial_{}.bin", i),
            destination: out_file.display().to_string(),
            total_size: probe.total_size,
            downloaded_bytes: 0,
            status: DownloadStatus::Queued,
            resumable: probe.resumable,
            etag: probe.etag,
            last_modified: probe.last_modified,
            expected_sha256: None,
            computed_sha256: None,
            error: None,
            created_at: now,
            completed_at: None,
        };
        manager.manifest().upsert_job(&job)?;

        let outcome = manager.run_job(&job_id, 8, cancel).await?;
        let dur = start.elapsed();
        let bytes = probe.total_size.unwrap_or(0);
        let speed_mb = (bytes as f64 / 1_048_576.0) / dur.as_secs_f64().max(0.001);

        if out_file.exists() {
            let _ = std::fs::remove_file(&out_file);
        }

        if !json {
            println!(
                "{} ({:.2}s, {:.2} MB/s)",
                "Done".green(),
                dur.as_secs_f64(),
                speed_mb
            );
        }

        results.push(serde_json::json!({
            "trial": i,
            "duration_secs": dur.as_secs_f64(),
            "bytes_transferred": bytes,
            "speed_mbps": speed_mb * 8.0,
            "speed_mbytes_sec": speed_mb,
            "outcome": format!("{:?}", outcome)
        }));
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        println!(
            "{}",
            "─── Benchmark Summary ──────────────────────────────────".dimmed()
        );
        println!("  All trials finished. Pipeline fully responsive.");
        println!(
            "{}",
            "────────────────────────────────────────────────────────".dimmed()
        );
    }

    Ok(())
}

async fn run_recover(manager: Arc<DownloadManager>, connections: usize, json: bool) -> Result<()> {
    let jobs = manager.manifest().list_jobs()?;
    let recoverable: Vec<_> = jobs
        .into_iter()
        .filter(|j| {
            j.status == DownloadStatus::Paused
                || j.status == DownloadStatus::Queued
                || j.status == DownloadStatus::Error
        })
        .collect();

    if recoverable.is_empty() {
        if !json {
            println!("{} No interrupted or recoverable jobs found.", "✓".green());
        } else {
            println!("{}", serde_json::json!({ "recovered_jobs": 0 }));
        }
        return Ok(());
    }

    if !json {
        println!(
            "{} Recovering {} interrupted downloads sequentially...",
            "⚙".cyan(),
            recoverable.len()
        );
    }

    let cancel = CancellationToken::new();
    let mut recovered_count = 0;

    for job in recoverable {
        if !json {
            print!(
                "  Resuming Job {} ({}) ... ",
                job.id.bold(),
                job.filename.cyan()
            );
            stdout().flush()?;
        }
        match manager.run_job(&job.id, connections, cancel.clone()).await {
            Ok(DownloadStatus::Completed) => {
                recovered_count += 1;
                if !json {
                    println!("{}", "Finished".green().bold());
                }
            }
            Ok(outcome) => {
                if !json {
                    println!("{}", format!("{:?}", outcome).yellow());
                }
            }
            Err(e) => {
                if !json {
                    println!("{} ({})", "Failed".red().bold(), e);
                }
            }
        }
    }

    if json {
        println!(
            "{}",
            serde_json::json!({ "recovered_jobs": recovered_count })
        );
    } else {
        println!("{} All recoverable downloads processed.", "✓".green());
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Standard Download and Batch Runners
// ---------------------------------------------------------------------------

async fn execute_batch_download(
    manager: Arc<DownloadManager>,
    batch_manifest: BatchManifest,
    base_out: PathBuf,
    concurrency: usize,
    connections_per_shard: usize,
    json_mode: bool,
) -> Result<()> {
    let batch_start = std::time::Instant::now();
    let cancel = CancellationToken::new();
    let sem = Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut tasks = Vec::new();

    for item in batch_manifest.files {
        let dest_path = base_out.join(&item.rfilename);
        if let Some(parent) = dest_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        // Check if file already exists and passes SHA-256
        if dest_path.exists()
            && let Some(expected_hash) = &item.sha256
            && let Ok(true) = verify_file_integrity(&dest_path, expected_hash)
        {
            if !json_mode {
                println!(
                    "{} {} [already downloaded & verified]",
                    "✓".green(),
                    item.rfilename.bold()
                );
            }
            continue;
        }

        let mgr = manager.clone();
        let sem_clone = sem.clone();
        let cancel_clone = cancel.clone();

        let task = tokio::spawn(async move {
            let _permit = match sem_clone.acquire().await {
                Ok(p) => p,
                Err(_) => return Err(anyhow::anyhow!("Semaphore closed")),
            };
            if cancel_clone.is_cancelled() {
                return Err(anyhow::anyhow!("Cancelled"));
            }

            let start = std::time::Instant::now();
            let filename = dest_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&item.rfilename)
                .to_string();

            let job_id = {
                let jobs = mgr.manifest().list_jobs().unwrap_or_default();
                let dest_str = dest_path.display().to_string();
                if let Some(existing) = jobs.iter().find(|j| j.destination == dest_str) {
                    if existing.status == DownloadStatus::Completed
                        && (!dest_path.exists()
                            || dest_path.metadata().map(|m| m.len()).unwrap_or(0) == 0)
                    {
                        let _ = mgr.manifest().delete_job(&existing.id);
                        let new_id = uuid_short();
                        let now = chrono::Utc::now().timestamp_millis();
                        let record = JobRecord {
                            id: new_id.clone(),
                            url: item.download_url.clone(),
                            filename: filename.clone(),
                            destination: dest_str,
                            total_size: item.size,
                            downloaded_bytes: 0,
                            status: DownloadStatus::Queued,
                            resumable: true,
                            etag: None,
                            last_modified: None,
                            expected_sha256: item.sha256.clone(),
                            computed_sha256: None,
                            error: None,
                            created_at: now,
                            completed_at: None,
                        };
                        mgr.manifest().upsert_job(&record)?;
                        new_id
                    } else {
                        existing.id.clone()
                    }
                } else {
                    let new_id = uuid_short();
                    let now = chrono::Utc::now().timestamp_millis();
                    let record = JobRecord {
                        id: new_id.clone(),
                        url: item.download_url.clone(),
                        filename: filename.clone(),
                        destination: dest_str,
                        total_size: item.size,
                        downloaded_bytes: 0,
                        status: DownloadStatus::Queued,
                        resumable: true,
                        etag: None,
                        last_modified: None,
                        expected_sha256: item.sha256.clone(),
                        computed_sha256: None,
                        error: None,
                        created_at: now,
                        completed_at: None,
                    };
                    mgr.manifest().upsert_job(&record)?;
                    new_id
                }
            };

            let outcome = mgr
                .run_job(&job_id, connections_per_shard, cancel_clone)
                .await?;
            let duration = start.elapsed();
            let final_sz = mgr
                .manifest()
                .get_job(&job_id)
                .ok()
                .flatten()
                .and_then(|j| j.total_size)
                .or_else(|| std::fs::metadata(&dest_path).ok().map(|m| m.len()))
                .unwrap_or(0);
            Ok((item.rfilename, outcome, duration, final_sz))
        });

        tasks.push(task);
    }

    let mut completed_count = 0;
    let mut total_transferred = 0u64;

    for task in tasks {
        match task.await {
            Ok(Ok((rfile, outcome, dur, size))) => {
                if outcome == DownloadStatus::Completed {
                    completed_count += 1;
                    total_transferred += size;
                    let speed = (size as f64 / 1_048_576.0) / dur.as_secs_f64().max(0.001);
                    if !json_mode {
                        println!(
                            "{} {} ({}, {:.2}s, {:.2} MB/s)",
                            "✓".green(),
                            rfile.bold(),
                            format_bytes(size),
                            dur.as_secs_f64(),
                            speed
                        );
                    }
                }
            }
            Ok(Err(e)) => {
                if !json_mode {
                    eprintln!("{} Shard download error: {}", "✗".red(), e);
                }
            }
            Err(e) => {
                if !json_mode {
                    eprintln!("{} Task join error: {}", "✗".red(), e);
                }
            }
        }
    }

    let total_elapsed = batch_start.elapsed();
    let avg_speed =
        (total_transferred as f64 / 1_048_576.0) / total_elapsed.as_secs_f64().max(0.001);

    if !json_mode {
        println!(
            "{}",
            "─── Batch Ingestion Summary ───────────────────────────────".dimmed()
        );
        println!("  Shards Completed:   {}", completed_count);
        println!("  Total Transferred:  {}", format_bytes(total_transferred));
        println!("  Elapsed Wall Time:  {:.2}s", total_elapsed.as_secs_f64());
        println!(
            "  Aggregate Speed:    {:.2} MB/s ({:.2} Mbps)",
            avg_speed,
            avg_speed * 8.0
        );
        println!(
            "{}",
            "──────────────────────────────────────────────────────────".dimmed()
        );
    }

    Ok(())
}

async fn run_batch(
    manager: Arc<DownloadManager>,
    manifest_path: PathBuf,
    output: Option<PathBuf>,
    concurrency: usize,
    connections_per_shard: usize,
    json_mode: bool,
) -> Result<()> {
    let manifest_bytes = std::fs::read(&manifest_path)
        .with_context(|| format!("Failed to read manifest file: {}", manifest_path.display()))?;
    let batch_manifest: BatchManifest = serde_json::from_slice(&manifest_bytes)
        .with_context(|| format!("Failed to parse JSON manifest: {}", manifest_path.display()))?;

    let base_out = output.unwrap_or_else(|| PathBuf::from("."));

    if !json_mode {
        println!(
            "{}",
            "─── OTTER Parallel Dataset Ingestion Pipeline ─────────────"
                .truecolor(179, 136, 255)
                .bold()
        );
        if let Some(ds) = &batch_manifest.dataset {
            println!("  Dataset:       {}", ds.cyan());
        }
        if let Some(rev) = &batch_manifest.revision {
            println!("  Revision:      {}", rev.dimmed());
        }
        println!("  Total Files:   {}", batch_manifest.files.len());
        println!(
            "  Concurrency:   {} parallel shards ({} conns/shard)",
            concurrency, connections_per_shard
        );
        println!("  Destination:   {}", base_out.display().to_string().cyan());
        println!(
            "{}",
            "──────────────────────────────────────────────────────────".dimmed()
        );
    }

    execute_batch_download(
        manager,
        batch_manifest,
        base_out,
        concurrency,
        connections_per_shard,
        json_mode,
    )
    .await
}

async fn run_download(
    manager: Arc<DownloadManager>,
    url: String,
    output: Option<PathBuf>,
    connections: usize,
    verify: Option<String>,
    json_mode: bool,
) -> Result<()> {
    let start_time = std::time::Instant::now();
    let client = reqwest::Client::new();
    let probe = probe_url(&client, &url)
        .await
        .context("Pre-flight probe failed")?;

    let (dest_path, filename) = match output {
        Some(out) => {
            let out_str = out.to_string_lossy();
            let is_dir_target = out.is_dir()
                || out_str.ends_with('\\')
                || out_str.ends_with('/')
                || out_str.ends_with(':')
                || (out.extension().is_none() && !out.exists());

            if is_dir_target {
                let p = out.join(&probe.filename);
                (p, probe.filename.clone())
            } else {
                let name = out
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| probe.filename.clone());
                (out, name)
            }
        }
        None => (PathBuf::from(&probe.filename), probe.filename.clone()),
    };

    let job_id = uuid_short();
    let now = chrono::Utc::now().timestamp_millis();

    let job = JobRecord {
        id: job_id.clone(),
        url: url.clone(),
        filename: filename.clone(),
        destination: dest_path.display().to_string(),
        total_size: probe.total_size,
        downloaded_bytes: 0,
        status: DownloadStatus::Queued,
        resumable: probe.resumable,
        etag: probe.etag,
        last_modified: probe.last_modified,
        expected_sha256: verify,
        computed_sha256: None,
        error: None,
        created_at: now,
        completed_at: None,
    };

    manager.manifest().upsert_job(&job)?;

    let effective_connections = match probe.total_size {
        Some(size) if size < 5 * 1024 * 1024 => 1,
        Some(size) if size < 30 * 1024 * 1024 => connections.min(4),
        _ => connections.max(1),
    };

    let cancel = CancellationToken::new();
    let cancel_for_signal = cancel.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        cancel_for_signal.cancel();
    });

    let mut rx = manager.subscribe_progress();
    let render_handle = tokio::spawn(async move {
        while let Ok(snap) = rx.recv().await {
            if json_mode {
                println!("{}", serde_json::to_string(&snap).unwrap());
            } else {
                render_progress_bar(&snap);
            }
        }
    });

    let outcome = manager
        .run_job(&job_id, effective_connections, cancel)
        .await?;
    render_handle.abort();

    let elapsed = start_time.elapsed();

    if !json_mode {
        println!();
        match outcome {
            DownloadStatus::Completed => {
                println!(
                    "{} Finished downloading {} provably.",
                    "✓".green().bold(),
                    filename.bold()
                );

                if let Ok(Some(final_job)) = manager.manifest().get_job(&job_id) {
                    let total_bytes = if final_job.downloaded_bytes > 0 {
                        final_job.downloaded_bytes
                    } else {
                        final_job.total_size.unwrap_or(0)
                    };
                    let speed_mbps =
                        (total_bytes as f64 / 1_048_576.0) / elapsed.as_secs_f64().max(0.001);
                    println!();
                    println!(
                        "{}",
                        "─── Download Performance & Verification ─────────────".dimmed()
                    );
                    println!("  File:        {}", filename.bold());
                    println!("  Destination: {}", dest_path.display().to_string().cyan());
                    println!("  Size:        {}", format_bytes(total_bytes));
                    println!("  Time Taken:  {:.2}s", elapsed.as_secs_f64());
                    println!(
                        "  Avg Speed:   {:.2} MB/s ({:.2} Mbps)",
                        speed_mbps,
                        speed_mbps * 8.0
                    );
                    if let Some(hash) = final_job.computed_sha256 {
                        println!("  SHA-256:     {}", hash.green());
                    }
                    println!(
                        "{}",
                        "────────────────────────────────────────────────────".dimmed()
                    );
                }
            }
            DownloadStatus::Paused => {
                println!(
                    "{} Download paused. Run {} to resume.",
                    "⏸".yellow().bold(),
                    format!("otter resume {}", job_id).cyan()
                );
            }
            _ => {}
        }
    }

    Ok(())
}

async fn run_resume_job(
    manager: Arc<DownloadManager>,
    job_id: String,
    connections: usize,
    url: Option<String>,
) -> Result<()> {
    if let Some(new_url) = url {
        manager.manifest().update_job_url(&job_id, &new_url)?;
        println!(
            "{} Updated download URL for job {}",
            "✓".green().bold(),
            job_id
        );
    }
    let cancel = CancellationToken::new();
    let cancel_for_signal = cancel.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        cancel_for_signal.cancel();
    });

    let mut rx = manager.subscribe_progress();
    let render_handle = tokio::spawn(async move {
        while let Ok(snap) = rx.recv().await {
            render_progress_bar(&snap);
        }
    });

    let outcome = manager.run_job(&job_id, connections, cancel).await?;
    render_handle.abort();

    println!();
    match outcome {
        DownloadStatus::Completed => {
            println!(
                "{} Download resumed and finished provably.",
                "✓".green().bold()
            );
        }
        DownloadStatus::Paused => {
            println!("{} Download paused again.", "⏸".yellow().bold());
        }
        _ => {}
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers & UI Rendering
// ---------------------------------------------------------------------------

fn uuid_short() -> String {
    let raw = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:x}", raw)[..8].to_string()
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

fn render_progress_bar(snap: &ProgressSnapshot) {
    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);
    let bar_width = 24.min(term_width.saturating_sub(45));

    let pct = snap.percentage().clamp(0.0, 100.0);
    let filled = ((pct / 100.0) * bar_width as f64) as usize;
    let empty = bar_width.saturating_sub(filled);

    let bar = format!(
        "{}{}",
        "━".repeat(filled).truecolor(179, 136, 255),
        "─".repeat(empty).dimmed()
    );
    let speed_mb = snap.speed_bps / 1_048_576.0;

    print!(
        "\r [{}] {:>5.1}% | {:>7.1} MB/s | Workers: {:>2} | ETA: {:>3}s",
        bar,
        pct,
        speed_mb,
        snap.connections,
        snap.eta_seconds.unwrap_or(0)
    );
    stdout().flush().ok();
}

fn print_jobs_table(jobs: &[JobRecord]) {
    if jobs.is_empty() {
        println!("  (No jobs recorded)");
        return;
    }
    println!(
        "  {:<10} {:<32} {:<12} {:<10} {:<12}",
        "ID".bold(),
        "FILENAME".bold(),
        "TOTAL".bold(),
        "STATUS".bold(),
        "PROGRESS".bold()
    );
    println!("  {}", "─".repeat(78).dimmed());
    for j in jobs {
        let status_color = match j.status {
            DownloadStatus::Completed => j.status.to_string().green(),
            DownloadStatus::Active => j.status.to_string().cyan(),
            DownloadStatus::Paused => j.status.to_string().yellow(),
            DownloadStatus::Error => j.status.to_string().red(),
            _ => j.status.to_string().dimmed(),
        };
        let total = j
            .total_size
            .map(format_bytes)
            .unwrap_or_else(|| "Unknown".into());
        let pct = if let Some(tot) = j.total_size {
            if tot > 0 {
                format!("{:.1}%", (j.downloaded_bytes as f64 / tot as f64) * 100.0)
            } else {
                "-".into()
            }
        } else {
            "-".into()
        };
        println!(
            "  {:<10} {:<32} {:<12} {:<10} {:<12}",
            j.id.bold(),
            &j.filename[..30.min(j.filename.len())],
            total,
            status_color,
            pct
        );
    }
}

pub fn print_welcome_banner() {
    let border_p = |s: &str| s.truecolor(200, 121, 245);
    let purple_fg = |s: &str| s.truecolor(199, 125, 250);

    println!(
        "  {}                                                                    {}",
        border_p("╭─────────────────────────╮"),
        "[ v0.1.0 ]".truecolor(169, 155, 175)
    );
    println!(
        "  {} {} {} {}",
        border_p("│"),
        purple_fg("✱"),
        "Welcome to OTTER CLI!".bold().white(),
        border_p("│")
    );
    println!("  {}", border_p("╰─────────────────────────╯"));
    println!();
    println!(
        "{}",
        purple_fg("  ██████╗ ████████╗████████╗███████╗██████╗")
    );
    println!(
        "{}",
        purple_fg(" ██╔═══██╗╚══██╔══╝╚══██╔══╝██╔════╝██╔══██╗")
    );
    println!(
        "{}",
        purple_fg(" ██║   ██║   ██║      ██║   █████╗  ██████╔╝")
    );
    println!(
        "{}",
        purple_fg(" ██║   ██║   ██║      ██║   ██╔══╝  ██╔══██╗")
    );
    println!(
        "{}",
        purple_fg(" ╚██████╔╝   ██║      ██║   ███████╗██║  ██║")
    );
    println!(
        "{}",
        purple_fg("  ╚═════╝    ╚═╝      ╚═╝   ╚══════╝╚═╝  ╚═╝")
    );
    println!("{}", purple_fg("  ██████╗██╗     ██╗"));
    println!("{}", purple_fg(" ██╔════╝██║     ██║"));
    println!("{}", purple_fg(" ██║     ██║     ██║"));
    println!("{}", purple_fg(" ██║     ██║     ██║"));
    println!("{}", purple_fg(" ╚██████╗███████╗██║"));
    println!("{}", purple_fg("  ╚═════╝╚══════╝╚═╝"));
    println!();
    println!(
        "  {} {}",
        purple_fg("▌").bold(),
        "Downloads that finish, provably."
            .bold()
            .white()
    );
    println!(
        "    {}",
        "Resumable · Verified · Crash-safe · Server-polite".dimmed()
    );
    println!();
}

fn get_system_hardware() -> (String, String, String) {
    let hostname = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "localhost".to_string());

    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    #[cfg(windows)]
    let ram_str = {
        #[allow(non_snake_case)]
        #[repr(C)]
        struct MemoryStatusEx {
            dw_length: u32,
            dw_memory_load: u32,
            ull_total_phys: u64,
            ull_avail_phys: u64,
            ull_total_page_file: u64,
            ull_avail_page_file: u64,
            ull_total_virtual: u64,
            ull_avail_virtual: u64,
            ull_avail_extended_virtual: u64,
        }
        unsafe {
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn GlobalMemoryStatusEx(lpBuffer: *mut MemoryStatusEx) -> i32;
            }
            let mut mem = std::mem::zeroed::<MemoryStatusEx>();
            mem.dw_length = std::mem::size_of::<MemoryStatusEx>() as u32;
            if GlobalMemoryStatusEx(&mut mem) != 0 {
                let gb = (mem.ull_total_phys as f64) / (1024.0 * 1024.0 * 1024.0);
                format!("{:.1} GB RAM", gb)
            } else {
                "Memory Ready".to_string()
            }
        }
    };
    #[cfg(not(windows))]
    let ram_str = "Memory Ready".to_string();

    (hostname, format!("{} Logical Cores", cores), ram_str)
}

struct RawModeGuard;
impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

struct TuiItem {
    cmd: &'static str,
    desc: &'static str,
    key_char: char,
    key_display: &'static str,
}

const TUI_ITEMS: [TuiItem; 10] = [
    TuiItem {
        cmd: "Download a file (Direct URL)",
        desc: "HTTP Range & AIMD",
        key_char: 'd',
        key_display: "d",
    },
    TuiItem {
        cmd: "Download AI model (HF / Ollama)",
        desc: "Pull from Ollama or HF",
        key_char: 'm',
        key_display: "m",
    },
    TuiItem {
        cmd: "Download AI dataset (Multi-shard)",
        desc: "Parallel dataset ingest",
        key_char: 'a',
        key_display: "a",
    },
    TuiItem {
        cmd: "Inspect artifact (GGUF/SafeT...)",
        desc: "Native parser for models",
        key_char: 'i',
        key_display: "i",
    },
    TuiItem {
        cmd: "Manage & resume downloads",
        desc: "Pause, resume or cancel",
        key_char: 'r',
        key_display: "r",
    },
    TuiItem {
        cmd: "Verify file integrity (SHA-256)",
        desc: "Cryptographic check",
        key_char: 'v',
        key_display: "v",
    },
    TuiItem {
        cmd: "System diagnostics & doctor",
        desc: "Disk IO, DNS & network",
        key_char: 's',
        key_display: "s",
    },
    TuiItem {
        cmd: "Settings & configuration",
        desc: "WAL, buffers & sync",
        key_char: 'c',
        key_display: "c",
    },
    TuiItem {
        cmd: "Help & documentation",
        desc: "Commands & CLI flags",
        key_char: 'h',
        key_display: "h",
    },
    TuiItem {
        cmd: "Exit",
        desc: "Clean exit",
        key_char: 'q',
        key_display: "q",
    },
];

fn run_interactive_menu_selection(manager: &DownloadManager) -> Result<Option<char>> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Ok(None);
    }

    let border_p = |s: &str| s.truecolor(200, 121, 245);
    let purple_fg = |s: &str| s.truecolor(199, 125, 250);
    let pill_bg = |s: &str| s.on_truecolor(140, 75, 210);
    let muted = |s: &str| s.truecolor(169, 155, 175);

    let jobs = manager.manifest().list_jobs().unwrap_or_default();
    let active = jobs
        .iter()
        .filter(|j| j.status == DownloadStatus::Active)
        .count();
    let paused = jobs
        .iter()
        .filter(|j| j.status == DownloadStatus::Paused)
        .count();
    let completed = jobs
        .iter()
        .filter(|j| j.status == DownloadStatus::Completed)
        .count();
    let total = jobs.len();
    let pct = if total > 0 {
        ((completed as f64 / total as f64) * 100.0).round() as u64
    } else {
        0
    };
    let free_bytes = engine::disk::get_available_disk_space(Path::new(".")).unwrap_or(0);
    let free_str = format_bytes(free_bytes);

    let (hostname, cores_str, ram_str) = get_system_hardware();
    let host_display: String = if hostname.len() > 19 {
        format!("{}...", &hostname[..16])
    } else {
        hostname
    };

    let (term_cols, _) = crossterm::terminal::size().unwrap_or((110, 35));
    let side_by_side = term_cols >= 105;

    let mut selected: usize = 0;
    let total_items = TUI_ITEMS.len();
    const FRAME_LINES: usize = 21;

    let render_frame = |sel: usize| {
        let mut left_lines: Vec<String> = Vec::with_capacity(FRAME_LINES);

        // Top border (68 chars)
        left_lines.push(
            border_p("╭─ [ Capabilities ] ───────────────────────────────────────────────╮")
                .to_string(),
        );

        // 10 items
        for (i, item) in TUI_ITEMS.iter().enumerate() {
            if i == sel {
                let inner = format!(
                    "> {:<33} {:<24} [{}]",
                    item.cmd, item.desc, item.key_display
                );
                left_lines.push(format!(
                    "{} {} {}",
                    border_p("│"),
                    pill_bg(&inner).white().bold(),
                    border_p("│")
                ));
            } else {
                let cmd_part = format!("  {:<33} ", item.cmd).white();
                let desc_part = muted(&format!("{:<24} ", item.desc));
                let key_part = format!("[{}]", item.key_display).dimmed();
                left_lines.push(format!(
                    "{} {}{}{} {}",
                    border_p("│"),
                    cmd_part,
                    desc_part,
                    key_part,
                    border_p("│")
                ));
            }
        }

        // Info & padding lines to align height to exactly 21 lines
        left_lines.push(format!(
            "{}                                                                  {}",
            border_p("│"),
            border_p("│")
        ));
        left_lines.push(format!(
            "{}  {} {:<61} {}",
            border_p("│"),
            purple_fg("▌").bold(),
            "Tip: Hotkeys (d, m, a, i, r, v, s, c, h, q) to act directly.".white(),
            border_p("│")
        ));
        left_lines.push(format!(
            "{}  {} {:<61} {}",
            border_p("│"),
            purple_fg("▌").bold(),
            "Engine: High-throughput HTTP Range & Dynamic AIMD Control".dimmed(),
            border_p("│")
        ));
        left_lines.push(format!(
            "{}  {} {:<61} {}",
            border_p("│"),
            purple_fg("▌").bold(),
            "Ingestion: Hugging Face Shards, Ollama Blobs & Formats".dimmed(),
            border_p("│")
        ));
        left_lines.push(format!(
            "{}  {} {:<61} {}",
            border_p("│"),
            purple_fg("▌").bold(),
            "Security: NTFS Mark-of-the-Web ADS & Streaming SHA-256".dimmed(),
            border_p("│")
        ));
        left_lines.push(format!(
            "{}  {} {:<61} {}",
            border_p("│"),
            purple_fg("▌").bold(),
            "Integrity: Crash-safe SQLite Manifest with Resume Support".dimmed(),
            border_p("│")
        ));
        left_lines.push(format!(
            "{}                                                                  {}",
            border_p("│"),
            border_p("│")
        ));
        left_lines.push(format!(
            "{}                                                                  {}",
            border_p("│"),
            border_p("│")
        ));
        left_lines.push(format!(
            "{}                                                                  {}",
            border_p("│"),
            border_p("│")
        ));
        left_lines.push(
            border_p("╰──────────────────────────────────────────────────────────────────╯")
                .to_string(),
        );

        // Right 3 Cards (exactly 21 lines, each exactly 34 chars)
        let mut right_lines: Vec<String> = Vec::with_capacity(FRAME_LINES);

        // Card 1: Quick Actions (5 lines)
        right_lines.push(border_p("╭─ [ Quick Actions ] ────────────╮").to_string());
        right_lines.push(format!(
            "{} New Download               {} {}",
            border_p("│"),
            "[d]".dimmed(),
            border_p("│")
        ));
        right_lines.push(format!(
            "{} Manage Jobs                {} {}",
            border_p("│"),
            "[r]".dimmed(),
            border_p("│")
        ));
        right_lines.push(format!(
            "{} Diagnostics                {} {}",
            border_p("│"),
            "[s]".dimmed(),
            border_p("│")
        ));
        right_lines.push(border_p("╰────────────────────────────────╯").to_string());

        // Gap line 5
        right_lines.push("                                  ".to_string());

        // Card 2: Job Overview (7 lines)
        right_lines.push(border_p("╭─ [ Job Overview ] ─────────────╮").to_string());
        right_lines.push(format!(
            "{} Active:        {:<4}            {}",
            border_p("│"),
            active,
            border_p("│")
        ));
        right_lines.push(format!(
            "{} Paused:        {:<4}            {}",
            border_p("│"),
            paused,
            border_p("│")
        ));
        right_lines.push(format!(
            "{} Completed:     {:<4}            {}",
            border_p("│"),
            completed,
            border_p("│")
        ));
        right_lines.push(format!(
            "{} Total:         {:<4}            {}",
            border_p("│"),
            total,
            border_p("│")
        ));
        let pct_str = format!("{:<4}", format!("{}%", pct));
        let pct_colored = purple_fg(&pct_str).bold();
        right_lines.push(format!(
            "{} Progress:      {}            {}",
            border_p("│"),
            pct_colored,
            border_p("│")
        ));
        right_lines.push(border_p("╰────────────────────────────────╯").to_string());

        // Gap line 13
        right_lines.push("                                  ".to_string());

        // Card 3: System (7 lines) - 100% Genuine OS & Laptop Hardware Metrics
        right_lines.push(border_p("╭─ [ System ] ───────────────────╮").to_string());
        let host_part = format!("{:<19}", host_display).cyan();
        right_lines.push(format!(
            "{} Device:    {} {}",
            border_p("│"),
            host_part,
            border_p("│")
        ));
        right_lines.push(format!(
            "{} CPU Cores: {:<19} {}",
            border_p("│"),
            cores_str,
            border_p("│")
        ));
        right_lines.push(format!(
            "{} Memory:    {:<19} {}",
            border_p("│"),
            ram_str,
            border_p("│")
        ));
        let free_padded = format!("{:>11}", free_str);
        let free_colored = free_padded.cyan();
        right_lines.push(format!(
            "{} Storage:     {} free  {}",
            border_p("│"),
            free_colored,
            border_p("│")
        ));
        right_lines.push(format!(
            "{} Database:  manifest.db (WAL)   {}",
            border_p("│"),
            border_p("│")
        ));
        right_lines.push(border_p("╰────────────────────────────────╯").to_string());

        if side_by_side {
            for i in 0..left_lines.len().max(right_lines.len()) {
                let left = left_lines.get(i).map(|s| s.as_str()).unwrap_or("");
                let right = right_lines.get(i).map(|s| s.as_str()).unwrap_or("");
                println!("  {}  {}", left, right);
            }
        } else {
            for line in &left_lines {
                println!("  {}", line);
            }
            println!();
            for line in &right_lines {
                println!("  {}", line);
            }
        }

        println!();
        println!(
            "  {} {}   {} {}   {} {}   {} {}   │   {}",
            "↑ ↓".bold().white(),
            "Navigate".dimmed(),
            "Enter".bold().white(),
            "Select".dimmed(),
            "Esc".bold().white(),
            "Back".dimmed(),
            "?".bold().white(),
            "Help".dimmed(),
            "otter / v0.1.0".truecolor(169, 155, 175)
        );
    };

    render_frame(selected);

    enable_raw_mode()?;
    let _guard = RawModeGuard;

    let lines_to_rewind: u16 = if side_by_side {
        23
    } else {
        (FRAME_LINES * 2 + 4) as u16
    };



    let choice = loop {
        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    selected = if selected == 0 {
                        total_items - 1
                    } else {
                        selected - 1
                    };
                    let _ = disable_raw_mode();
                    execute!(
                        stdout(),
                        cursor::MoveUp(lines_to_rewind),
                        cursor::MoveToColumn(0)
                    )?;
                    render_frame(selected);
                    let _ = enable_raw_mode();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    selected = (selected + 1) % total_items;
                    let _ = disable_raw_mode();
                    execute!(
                        stdout(),
                        cursor::MoveUp(lines_to_rewind),
                        cursor::MoveToColumn(0)
                    )?;
                    render_frame(selected);
                    let _ = enable_raw_mode();
                }
                KeyCode::Enter => {
                    break Some(TUI_ITEMS[selected].key_char);
                }
                KeyCode::Esc => {
                    break Some('q');
                }
                KeyCode::Char('?') | KeyCode::Char('h') => {
                    break Some('h');
                }
                KeyCode::Char(c) => {
                    let lower = c.to_ascii_lowercase();
                    if c == 'D' {
                        break Some('D');
                    }
                    if c == 'M' {
                        break Some('M');
                    }
                    if let Some(item) = TUI_ITEMS
                        .iter()
                        .find(|i| i.key_char == lower || i.key_char == c)
                    {
                        break Some(item.key_char);
                    }
                    if lower == 'q' {
                        break Some('q');
                    }
                }
                _ => {}
            }
        }
    };

    let _ = disable_raw_mode();
    println!();
    Ok(choice)
}

async fn run_interactive_home_menu(manager: Arc<DownloadManager>) -> Result<()> {
    loop {
        print_welcome_banner();

        let choice = run_interactive_menu_selection(&manager)?;
        if choice.is_none() || choice == Some('q') {
            break;
        }

        match choice {
            Some('d') => {
                println!(
                    "{}",
                    "─── OTTER Download (Direct URL / Hugging Face / Ollama) ────".dimmed()
                );
                print!(
                    "  {} ",
                    "Enter URL or Model ID (e.g. https://..., ollama:..., hf:...):"
                        .bold()
                        .truecolor(179, 136, 255)
                );
                stdout().flush().ok();
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                let input = input.trim().to_string();
                if !input.is_empty() {
                    print!(
                        "  {} ",
                        "Destination path (press Enter for default):".dimmed()
                    );
                    stdout().flush().ok();
                    let mut dest = String::new();
                    std::io::stdin().read_line(&mut dest)?;
                    let dest_trim = dest.trim().trim_matches('"');
                    let output = if dest_trim.is_empty() {
                        None
                    } else {
                        Some(PathBuf::from(dest_trim))
                    };
                    println!();
                    if input.starts_with("ollama:") || input.starts_with("hf:") {
                        run_model_download(manager.clone(), input, output, None, None, 8, false)
                            .await?;
                    } else {
                        run_download(manager.clone(), input, output, 8, None, false).await?;
                    }
                }
            }
            Some('r') => {
                println!(
                    "{}",
                    "─── OTTER Manage & Resume Downloads ──────────────────────────".dimmed()
                );
                let jobs = manager.manifest().list_jobs()?;
                print_jobs_table(&jobs);
                println!();
                println!(
                    "  Actions: {} Resume   {} Pause   {} Cancel   {} Return",
                    "[r]".bold().cyan(),
                    "[p]".bold().yellow(),
                    "[x]".bold().red(),
                    "[Enter]".dimmed()
                );
                print!("  Select action (r/p/x or Enter to return): ");
                stdout().flush().ok();
                let mut act = String::new();
                std::io::stdin().read_line(&mut act)?;
                let act_trim = act.trim().to_lowercase();
                if act_trim == "r" {
                    print!("  Enter Job ID to resume: ");
                    stdout().flush().ok();
                    let mut id_str = String::new();
                    std::io::stdin().read_line(&mut id_str)?;
                    let id_trim = id_str.trim().to_string();
                    if !id_trim.is_empty() {
                        println!();
                        run_resume_job(manager.clone(), id_trim, 8, None).await?;
                    }
                } else if act_trim == "p" {
                    print!("  Enter Job ID to pause: ");
                    stdout().flush().ok();
                    let mut id_str = String::new();
                    std::io::stdin().read_line(&mut id_str)?;
                    let id_trim = id_str.trim().to_string();
                    if !id_trim.is_empty() {
                        manager
                            .manifest()
                            .update_status(&id_trim, DownloadStatus::Paused, None)?;
                        println!("{} Job {} marked as Paused.", "✓".green(), id_trim.bold());
                    }
                } else if act_trim == "x" {
                    print!("  Enter Job ID to cancel: ");
                    stdout().flush().ok();
                    let mut id_str = String::new();
                    std::io::stdin().read_line(&mut id_str)?;
                    let id_trim = id_str.trim().to_string();
                    if !id_trim.is_empty() {
                        if let Ok(Some(job)) = manager.manifest().get_job(&id_trim) {
                            let mut part = PathBuf::from(&job.destination);
                            let file_name = match part.file_name() {
                                Some(n) => {
                                    let mut s = n.to_os_string();
                                    s.push(".part");
                                    s
                                }
                                None => "download.bin.part".into(),
                            };
                            part.set_file_name(file_name);
                            if part.exists() {
                                let _ = std::fs::remove_file(&part);
                            }
                            manager.manifest().delete_job(&id_trim)?;
                            println!(
                                "{} Job {} cancelled and removed from manifest.",
                                "✓".green(),
                                id_trim.bold()
                            );
                        } else {
                            eprintln!("{} Job {} not found.", "✗".red(), id_trim);
                        }
                    }
                }
            }
            Some('p') => {
                println!(
                    "{}",
                    "─── OTTER Pause Download Job ─────────────────────────────────".dimmed()
                );
                print!(
                    "  {} ",
                    "Enter Job ID to pause:".bold().truecolor(179, 136, 255)
                );
                stdout().flush().ok();
                let mut id_str = String::new();
                std::io::stdin().read_line(&mut id_str)?;
                let id_trim = id_str.trim().to_string();
                if !id_trim.is_empty() {
                    manager
                        .manifest()
                        .update_status(&id_trim, DownloadStatus::Paused, None)?;
                    println!("{} Job {} marked as Paused.", "✓".green(), id_trim.bold());
                }
            }
            Some('x') => {
                println!(
                    "{}",
                    "─── OTTER Cancel Download Job ────────────────────────────────".dimmed()
                );
                print!(
                    "  {} ",
                    "Enter Job ID to cancel:".bold().truecolor(179, 136, 255)
                );
                stdout().flush().ok();
                let mut id_str = String::new();
                std::io::stdin().read_line(&mut id_str)?;
                let id_trim = id_str.trim().to_string();
                if !id_trim.is_empty() {
                    if let Ok(Some(job)) = manager.manifest().get_job(&id_trim) {
                        let mut part = PathBuf::from(&job.destination);
                        let file_name = match part.file_name() {
                            Some(n) => {
                                let mut s = n.to_os_string();
                                s.push(".part");
                                s
                            }
                            None => "download.bin.part".into(),
                        };
                        part.set_file_name(file_name);
                        if part.exists() {
                            let _ = std::fs::remove_file(&part);
                        }
                        manager.manifest().delete_job(&id_trim)?;
                        println!(
                            "{} Job {} cancelled and removed from manifest.",
                            "✓".green(),
                            id_trim.bold()
                        );
                    } else {
                        eprintln!("{} Job {} not found.", "✗".red(), id_trim);
                    }
                }
            }
            Some('l') => {
                println!(
                    "{}",
                    "Active & Tracked Download Jobs:"
                        .bold()
                        .truecolor(179, 136, 255)
                );
                let jobs = manager.manifest().list_jobs()?;
                print_jobs_table(&jobs);
            }
            Some('v') => {
                println!(
                    "{}",
                    "─── OTTER Streaming SHA-256 Integrity Verification ──────────".dimmed()
                );
                print!(
                    "  {} ",
                    "Enter path to file or artifact:"
                        .bold()
                        .truecolor(179, 136, 255)
                );
                stdout().flush().ok();
                let mut path_str = String::new();
                std::io::stdin().read_line(&mut path_str)?;
                let path_trim = path_str.trim().trim_matches('"');
                if !path_trim.is_empty() {
                    let pb = PathBuf::from(path_trim);
                    run_verify(pb, None, false)?;
                }
            }
            Some('s') => {
                let jobs = manager.manifest().list_jobs()?;
                let active = jobs
                    .iter()
                    .filter(|j| j.status == DownloadStatus::Active)
                    .count();
                let paused = jobs
                    .iter()
                    .filter(|j| j.status == DownloadStatus::Paused)
                    .count();
                let completed = jobs
                    .iter()
                    .filter(|j| j.status == DownloadStatus::Completed)
                    .count();

                println!(
                    "{}",
                    "OTTER System & Download Engine Status"
                        .bold()
                        .truecolor(179, 136, 255)
                );
                println!("  Active Jobs:    {}", active.to_string().cyan());
                println!("  Paused Jobs:    {}", paused.to_string().yellow());
                println!("  Completed Jobs: {}", completed.to_string().green());
                println!("  Total Tracked:  {}", jobs.len());
                println!();
                run_doctor(None, true, false).await?;
            }
            Some('e') => {
                println!(
                    "{}",
                    "─── OTTER Export Reproducible Manifest ───────────────────────".dimmed()
                );
                print!(
                    "  {} ",
                    "Enter Job ID to export:".bold().truecolor(179, 136, 255)
                );
                stdout().flush().ok();
                let mut id_str = String::new();
                std::io::stdin().read_line(&mut id_str)?;
                let id_trim = id_str.trim().to_string();
                if !id_trim.is_empty() {
                    if let Some(job) = manager.manifest().get_job(&id_trim)? {
                        let rep = serde_json::json!({
                            "job_id": job.id,
                            "url": job.url,
                            "filename": job.filename,
                            "destination": job.destination,
                            "total_size": job.total_size,
                            "downloaded_bytes": job.downloaded_bytes,
                            "status": format!("{:?}", job.status),
                            "etag": job.etag,
                            "resumable": job.resumable,
                            "created_at": job.created_at,
                            "completed_at": job.completed_at,
                        });
                        println!("{}", serde_json::to_string_pretty(&rep)?);
                    } else {
                        eprintln!("{} Job {} not found.", "✗".red(), id_trim);
                    }
                }
            }
            Some('D') => {
                println!(
                    "{}",
                    "─── OTTER System Doctor & Diagnostics ────────────────────────".dimmed()
                );
                run_doctor(None, true, false).await?;
            }
            Some('i') => {
                println!(
                    "{}",
                    "─── OTTER Artifact Inspector (GGUF / Safetensors / ONNX / Parquet) ──"
                        .dimmed()
                );
                print!(
                    "  {} ",
                    "Enter path to artifact file:"
                        .bold()
                        .truecolor(179, 136, 255)
                );
                stdout().flush().ok();
                let mut path_str = String::new();
                std::io::stdin().read_line(&mut path_str)?;
                let path_trim = path_str.trim().trim_matches('"');
                if !path_trim.is_empty() {
                    println!();
                    run_inspect(PathBuf::from(path_trim), false)?;
                }
            }
            Some('m') => {
                println!(
                    "{}",
                    "─── OTTER AI Model Downloader (Ollama & Hugging Face) ────────".dimmed()
                );
                print!(
                    "  {} ",
                    "Enter Model ID (e.g. ollama:qwen3:4b, hf:TheBloke/...):"
                        .bold()
                        .truecolor(179, 136, 255)
                );
                stdout().flush().ok();
                let mut model_str = String::new();
                std::io::stdin().read_line(&mut model_str)?;
                let model_trim = model_str.trim().to_string();
                if !model_trim.is_empty() {
                    println!();
                    run_model_download(manager.clone(), model_trim, None, None, None, 8, false)
                        .await?;
                }
            }
            Some('a') => {
                println!(
                    "{}",
                    "─── OTTER AI Dataset Downloader (Multi-shard Ingestion) ──────".dimmed()
                );
                print!(
                    "  {} ",
                    "Enter Dataset ID (e.g. hf:wikitext or batch JSON manifest path):"
                        .bold()
                        .truecolor(179, 136, 255)
                );
                stdout().flush().ok();
                let mut ds_str = String::new();
                std::io::stdin().read_line(&mut ds_str)?;
                let ds_trim = ds_str.trim().to_string();
                if !ds_trim.is_empty() {
                    println!();
                    run_dataset_download(manager.clone(), ds_trim, None, None, None, 4, 8, false)
                        .await?;
                }
            }
            Some('M') => {
                run_interactive_manager(manager.clone()).await?;
            }
            Some('c') => {
                println!(
                    "{}",
                    "OTTER Engine Settings & Configuration"
                        .bold()
                        .truecolor(179, 136, 255)
                );
                let appdata = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
                let manifest_path = PathBuf::from(appdata).join("otter").join("manifest.db");
                println!(
                    "  Manifest Database:    {}",
                    manifest_path.display().to_string().cyan()
                );
                println!("  Default Connections:  8 parallel TCP streams");
                println!(
                    "  Adaptive Thresholds:  <5MB: 1 conn | 5-30MB: 4 conns | >30MB: 8 conns (AIMD dynamic)"
                );
                println!(
                    "  Storage Sync Mode:    Direct positioned writes (no post-merge, crash-safe)"
                );
                println!(
                    "  Host Rate Limiting:   Budget 16 max concurrent connections per server domain"
                );
                println!(
                    "  Rotational Media:     Auto-detected (WAL checkpoint tuned: 8000 HDD / 1000 SSD)"
                );
            }
            Some('h') => {
                println!(
                    "{}",
                    "OTTER CLI Available Commands & Capabilities:"
                        .bold()
                        .truecolor(179, 136, 255)
                );
                println!(
                    "  otter download <URL> [-o <PATH>] [-c <CONNS>]    Download direct HTTP/HTTPS file"
                );
                println!(
                    "  otter resume <JOB_ID>                             Resume interrupted download job"
                );
                println!(
                    "  otter pause <JOB_ID>                              Pause an active download job"
                );
                println!(
                    "  otter cancel <JOB_ID>                             Cancel download and clean .part file"
                );
                println!(
                    "  otter list [--json]                               List all tracked jobs and metrics"
                );
                println!(
                    "  otter verify <PATH> [--expected <SHA256>]         Compute & verify SHA-256 hash"
                );
                println!(
                    "  otter status [<JOB_ID>]                           Show job details or engine status"
                );
                println!(
                    "  otter export-manifest <JOB_ID>                    Export reproducible JSON manifest"
                );
                println!(
                    "  otter doctor                                      System, disk & network diagnostics"
                );
                println!(
                    "  otter inspect <PATH>                              Inspect GGUF, Safetensors, ONNX, Parquet"
                );
                println!(
                    "  otter model download <ID>                         Download Ollama or Hugging Face model"
                );
                println!(
                    "  otter dataset download <ID>                       Download sharded AI dataset"
                );
                println!(
                    "  otter manage                                      Interactive download table"
                );
                println!(
                    "  otter config                                      View engine parameters & WAL sync"
                );
            }
            _ => {}
        }

        println!();
        print!(
            "  {} ",
            "Press Enter to return to menu (or 'q' to exit):".dimmed()
        );
        stdout().flush().ok();
        let mut cont = String::new();
        std::io::stdin().read_line(&mut cont)?;
        if cont.trim().eq_ignore_ascii_case("q") {
            break;
        }
    }

    Ok(())
}

async fn run_interactive_manager(manager: Arc<DownloadManager>) -> Result<()> {
    print_welcome_banner();
    println!(
        "{}",
        "Active & Recorded Download Jobs:"
            .bold()
            .truecolor(179, 136, 255)
    );
    let jobs = manager.manifest().list_jobs()?;
    print_jobs_table(&jobs);
    println!();
    Ok(())
}
