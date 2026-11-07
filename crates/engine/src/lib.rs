pub mod adaptive;
pub mod artifact;
pub mod disk;
pub mod doctor;
pub mod hf;
pub mod kaggle;
pub mod manifest;
pub mod ollama;
pub mod orchestrator;
pub mod probe;
pub mod retry;
pub mod segment;
pub mod verify;

pub use adaptive::{AimdConfig, AimdController, AimdDecision, AimdState};

pub use artifact::{
    ArtifactFormat, ArtifactInspection, ManifestFileEntry, ManifestVerificationReport,
    ReproducibilityManifest, create_reproducibility_manifest, inspect_artifact,
    verify_reproducibility_manifest,
};
pub use disk::{FileWriter, attach_zone_identifier, is_rotational_media, read_zone_identifier};
pub use doctor::{DoctorCheckItem, DoctorEngine, DoctorReport, DoctorStatus};
pub use hf::{HfClient, HfRepoInfo, HfResolvedDownloadItem};
pub use kaggle::{
    KaggleClient, KaggleResolvedDataset, parse_kaggle_dataset_input, verify_kaggle_zip_archive,
};
pub use manifest::ManifestStore;
pub use ollama::{
    OllamaRegistryClient, OllamaResolvedBlob, OllamaResolvedModel, parse_model_namespace_tag,
    parse_model_tag,
};
pub use orchestrator::DownloadManager;
pub use probe::{extract_filename, probe_url, sanitize_filename};
pub use retry::RetryPolicy;
pub use types::*;
pub use verify::{compute_file_sha256, verify_file_integrity};
