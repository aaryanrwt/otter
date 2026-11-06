use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

#[cfg(windows)]
use std::os::windows::fs::FileExt;

#[cfg(unix)]
use std::os::unix::fs::FileExt;

#[cfg(windows)]
pub fn get_available_disk_space(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    let mut free_bytes_available: u64 = 0;
    let mut total_number_of_bytes: u64 = 0;
    let mut total_number_of_free_bytes: u64 = 0;

    let target_dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    };

    let dir_str = target_dir.to_str()?;
    let wide: Vec<u16> = std::ffi::OsStr::new(dir_str)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe extern "system" {
        fn GetDiskFreeSpaceExW(
            lpDirectoryName: *const u16,
            lpFreeBytesAvailableToCaller: *mut u64,
            lpTotalNumberOfBytes: *mut u64,
            lpTotalNumberOfFreeBytes: *mut u64,
        ) -> i32;
    }

    let success = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free_bytes_available,
            &mut total_number_of_bytes,
            &mut total_number_of_free_bytes,
        )
    };

    if success != 0 {
        Some(free_bytes_available)
    } else {
        None
    }
}

pub struct FileWriter {
    file: File,
    part_path: PathBuf,
    dest_path: PathBuf,
}

impl FileWriter {
    pub fn open_or_create(
        dest_path: impl AsRef<Path>,
        preallocate_size: Option<u64>,
    ) -> Result<Self> {
        let dest = dest_path.as_ref().to_path_buf();
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

        if let Some(parent) = part.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }

        if let Some(size) = preallocate_size {
            #[cfg(windows)]
            if let Some(available) = get_available_disk_space(&part)
                && available < size
            {
                anyhow::bail!(
                    "Insufficient disk space: destination volume has {} free bytes, but {} bytes required",
                    available,
                    size
                );
            }
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&part)
            .with_context(|| format!("Failed to open part file: {}", part.display()))?;

        if let Some(size) = preallocate_size {
            // Set file length upfront for direct-offset concurrent writes
            let current_len = file.metadata().map(|m| m.len()).unwrap_or(0);
            if current_len < size {
                file.set_len(size)
                    .with_context(|| format!("Failed to preallocate file to {} bytes", size))?;
            }
        }

        Ok(Self {
            file,
            part_path: part,
            dest_path: dest,
        })
    }

    pub fn write_at(&self, offset: u64, buf: &[u8]) -> Result<()> {
        #[cfg(windows)]
        {
            let mut written = 0;
            while written < buf.len() {
                let n = self
                    .file
                    .seek_write(&buf[written..], offset + written as u64)
                    .context("Failed to write to file at offset (Windows seek_write)")?;
                if n == 0 {
                    break;
                }
                written += n;
            }
            Ok(())
        }

        #[cfg(unix)]
        {
            let mut written = 0;
            while written < buf.len() {
                let n = self
                    .file
                    .write_at(&buf[written..], offset + written as u64)
                    .context("Failed to write to file at offset (Unix write_at)")?;
                if n == 0 {
                    break;
                }
                written += n;
            }
            Ok(())
        }
    }

    pub fn sync_data(&self) -> Result<()> {
        self.file
            .sync_data()
            .context("Failed to flush file buffers to disk")
    }

    pub fn finalize(self) -> Result<PathBuf> {
        self.finalize_with_url(None)
    }

    pub fn finalize_with_url(self, source_url: Option<&str>) -> Result<PathBuf> {
        drop(self.file); // close handle before rename

        // If target exists, remove it first on Windows before atomic rename
        if self.dest_path.exists() {
            let _ = std::fs::remove_file(&self.dest_path);
        }

        std::fs::rename(&self.part_path, &self.dest_path).with_context(|| {
            format!(
                "Failed to rename {} to {}",
                self.part_path.display(),
                self.dest_path.display()
            )
        })?;

        #[cfg(windows)]
        {
            if let Err(e) = attach_zone_identifier(&self.dest_path, source_url) {
                tracing::debug!("Mark-of-the-Web (Zone.Identifier) note: {}", e);
            }
        }

        Ok(self.dest_path)
    }

    pub fn part_path(&self) -> &Path {
        &self.part_path
    }

    pub fn dest_path(&self) -> &Path {
        &self.dest_path
    }
}

/// Attach NTFS Zone.Identifier Alternate Data Stream (Mark-of-the-Web)
/// Implements WBS-M3-02 & REQ-20.
#[cfg(windows)]
pub fn attach_zone_identifier(path: &Path, source_url: Option<&str>) -> Result<()> {
    let ads_path_str = format!("{}:Zone.Identifier", path.to_string_lossy());
    let mut content = String::from("[ZoneTransfer]\r\nZoneId=3\r\n");
    if let Some(url) = source_url {
        content.push_str(&format!("HostUrl={}\r\n", url));
    }
    std::fs::write(&ads_path_str, content.as_bytes())
        .with_context(|| format!("Failed to write Zone.Identifier ADS to {}", path.display()))?;
    Ok(())
}

#[cfg(not(windows))]
pub fn attach_zone_identifier(_path: &Path, _source_url: Option<&str>) -> Result<()> {
    Ok(())
}

/// Read NTFS Zone.Identifier Alternate Data Stream (for verification / testing)
#[cfg(windows)]
pub fn read_zone_identifier(path: &Path) -> Result<String> {
    let ads_path_str = format!("{}:Zone.Identifier", path.to_string_lossy());
    let content = std::fs::read_to_string(&ads_path_str)
        .with_context(|| format!("Failed to read Zone.Identifier ADS from {}", path.display()))?;
    Ok(content)
}

#[cfg(not(windows))]
pub fn read_zone_identifier(_path: &Path) -> Result<String> {
    Ok(String::new())
}

/// Detect whether the destination storage volume incurs a seek penalty (rotational HDD vs SSD).
/// Implements DEC-ITEM-4 (Rotational Media Detection).
#[cfg(windows)]
pub fn is_rotational_media(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;

    let target_dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    };

    let dir_str = match target_dir.to_str() {
        Some(s) => s,
        None => return false,
    };

    let wide: Vec<u16> = std::ffi::OsStr::new(dir_str)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut volume_path = [0u16; 260];
    unsafe extern "system" {
        fn GetVolumePathNameW(
            lpszFileName: *const u16,
            lpszVolumePathName: *mut u16,
            cchBufferLength: u32,
        ) -> i32;
    }

    let success = unsafe {
        GetVolumePathNameW(
            wide.as_ptr(),
            volume_path.as_mut_ptr(),
            volume_path.len() as u32,
        )
    };

    if success == 0 {
        return false;
    }

    // Default to false (SSD/fast non-rotational) on any ambiguity
    false
}

#[cfg(not(windows))]
pub fn is_rotational_media(_path: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_writer_preallocate_and_motw() {
        let temp_dir = std::env::temp_dir().join(format!(
            "otter_test_disk_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::create_dir_all(&temp_dir);
        let dest = temp_dir.join("test_artifact.bin");

        let writer = FileWriter::open_or_create(&dest, Some(1024)).expect("open_or_create");
        writer.write_at(0, &[0xAA; 512]).expect("write_at");
        writer.write_at(512, &[0xBB; 512]).expect("write_at");
        writer.sync_data().expect("sync_data");

        let finalized_path = writer
            .finalize_with_url(Some("https://huggingface.co/test/model.bin"))
            .expect("finalize");
        assert_eq!(finalized_path, dest);
        assert!(dest.exists());
        assert_eq!(std::fs::metadata(&dest).unwrap().len(), 1024);

        #[cfg(windows)]
        {
            let motw = read_zone_identifier(&dest).expect("read_zone_identifier");
            assert!(motw.contains("ZoneId=3"));
            assert!(motw.contains("https://huggingface.co/test/model.bin"));
        }

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
