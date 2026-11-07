use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_ENGINE;
use std::sync::OnceLock;

pub const LOGO_HF_BYTES: &[u8] =
    include_bytes!("../../../logos/c3635b59-a3d2-444a-b636-a9d0061dcdde.png");
pub const LOGO_EPIC_BYTES: &[u8] = include_bytes!("../../../logos/Epic-Games-logo.png");
pub const LOGO_GITHUB_BYTES: &[u8] = include_bytes!("../../../logos/github_logo_icon_145252.png");
pub const LOGO_CHROME_BYTES: &[u8] =
    include_bytes!("../../../logos/google-chrome-web-browser-google-logo-google-i-o-google.jpg");
pub const LOGO_STEAM_BYTES: &[u8] = include_bytes!("../../../logos/images (1).jpeg");
pub const LOGO_INSTAGRAM_BYTES: &[u8] = include_bytes!("../../../logos/images (3).png");
pub const LOGO_PINTEREST_BYTES: &[u8] =
    include_bytes!("../../../logos/pinterest-logo-pinterest-transparent-free-png.webp");
pub const LOGO_FOLDER_BYTES: &[u8] = include_bytes!(
    "../../../logos/pngtree-transparent-file-folder-icon-for-storage-concept-design-organize-object-vector-png-image_41394742.jpg"
);

#[derive(Debug, Clone)]
pub struct AppLogos {
    pub hf: String,
    pub epic: String,
    pub github: String,
    pub chrome: String,
    pub steam: String,
    pub instagram: String,
    pub pinterest: String,
    pub folder: String,
}

static LOGOS: OnceLock<AppLogos> = OnceLock::new();

pub fn get_logos() -> &'static AppLogos {
    LOGOS.get_or_init(|| AppLogos {
        hf: format!(
            "data:image/png;base64,{}",
            BASE64_ENGINE.encode(LOGO_HF_BYTES)
        ),
        epic: format!(
            "data:image/png;base64,{}",
            BASE64_ENGINE.encode(LOGO_EPIC_BYTES)
        ),
        github: format!(
            "data:image/png;base64,{}",
            BASE64_ENGINE.encode(LOGO_GITHUB_BYTES)
        ),
        chrome: format!(
            "data:image/jpeg;base64,{}",
            BASE64_ENGINE.encode(LOGO_CHROME_BYTES)
        ),
        steam: format!(
            "data:image/jpeg;base64,{}",
            BASE64_ENGINE.encode(LOGO_STEAM_BYTES)
        ),
        instagram: format!(
            "data:image/png;base64,{}",
            BASE64_ENGINE.encode(LOGO_INSTAGRAM_BYTES)
        ),
        pinterest: format!(
            "data:image/webp;base64,{}",
            BASE64_ENGINE.encode(LOGO_PINTEREST_BYTES)
        ),
        folder: format!(
            "data:image/jpeg;base64,{}",
            BASE64_ENGINE.encode(LOGO_FOLDER_BYTES)
        ),
    })
}

pub fn get_logo_img_tag(app_id_or_domain: &str) -> String {
    let logos = get_logos();
    let lower = app_id_or_domain.to_lowercase();
    let (src, alt) = if lower.contains("chrome") || lower.contains("google") {
        (&logos.chrome, "Google Chrome")
    } else if lower.contains("steam") {
        (&logos.steam, "Steam")
    } else if lower.contains("epic") {
        (&logos.epic, "Epic Games")
    } else if lower.contains("huggingface") || lower.contains("hf") {
        (&logos.hf, "Hugging Face")
    } else if lower.contains("github") {
        (&logos.github, "GitHub")
    } else if lower.contains("instagram") {
        (&logos.instagram, "Instagram")
    } else if lower.contains("pinterest") || lower.contains("pin.it") {
        (&logos.pinterest, "Pinterest")
    } else {
        (&logos.folder, "Folder")
    };

    format!(
        r#"<img src="{}" alt="{}" class="brand-img-icon" />"#,
        src, alt
    )
}
