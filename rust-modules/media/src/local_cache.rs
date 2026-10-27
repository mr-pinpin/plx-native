//! Optional, content-addressed private background media. Open and verification run only
//! on the existing demux worker; no URL parser or network credential policy is relaxed.
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

pub const ROOT: &str = "/media/internal/pinpin-family/cache";
pub const MAX_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub sha256: String,
    pub bytes: u64,
}
impl Spec {
    pub fn valid(&self) -> bool {
        self.bytes > 0
            && self.bytes <= MAX_BYTES
            && self.sha256.len() == 64
            && self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }
}

pub struct LocalFile {
    file: File,
    size: u64,
}
impl LocalFile {
    #[cfg(test)]
    pub(crate) fn fixture(root: &Path, spec: &Spec) -> io::Result<Self> {
        Self::open_at(root, spec, || false)
    }
    pub fn open(
        _off: &plx_base::task::OffFrame,
        spec: &Spec,
        cancelled: impl FnMut() -> bool,
    ) -> io::Result<Self> {
        Self::open_at(Path::new(ROOT), spec, cancelled)
    }
    fn open_at(root: &Path, spec: &Spec, mut cancelled: impl FnMut() -> bool) -> io::Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "local cache rejected");
        let abort = || io::Error::new(io::ErrorKind::Interrupted, "local cache cancelled");
        if cancelled() {
            return Err(abort());
        }
        if !spec.valid()
            || !root.is_absolute()
            || root.canonicalize()? != root
            || !std::fs::symlink_metadata(root)?.is_dir()
        {
            return Err(invalid());
        }
        // No path is supplied by config. The only leaf is a lowercase SHA-256 plus .mp4.
        // The fixed private root must itself have no symlink components. O_NOFOLLOW closes
        // leaf replacement races; verification and playback retain this same open inode.
        let path = root.join(format!("{}.mp4", spec.sha256));
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() != spec.bytes {
            return Err(invalid());
        }
        let mut digest = plx_base::sha256::Sha256::new();
        let mut count = 0u64;
        let mut chunk = [0; 65536];
        loop {
            if cancelled() {
                return Err(abort());
            }
            let n = file.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            count = count.checked_add(n as u64).ok_or_else(invalid)?;
            if count > spec.bytes {
                return Err(invalid());
            }
            digest.update(&chunk[..n]);
        }
        let actual: String = digest
            .finish()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if count != spec.bytes || actual != spec.sha256 || file.metadata()?.len() != spec.bytes {
            return Err(invalid());
        }
        if cancelled() {
            return Err(abort());
        }
        file.seek(SeekFrom::Start(0))?;
        Ok(Self { file, size: count })
    }
    pub fn size(&self) -> i64 {
        self.size as i64
    }
    pub fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        self.file.read(dst)
    }
    pub fn seek(&mut self, target: i64) -> bool {
        target >= 0 && self.file.seek(SeekFrom::Start(target as u64)).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(data: &[u8]) -> (std::path::PathBuf, Spec) {
        let root = std::env::temp_dir().join(format!(
            "pinpin-local-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let spec = Spec {
            sha256: plx_base::sha256::sha256(data)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            bytes: data.len() as u64,
        };
        std::fs::write(root.join(format!("{}.mp4", spec.sha256)), data).unwrap();
        (root, spec)
    }
    #[test]
    fn verified_file_read_seek_eof_and_size() {
        let (root, spec) = fixture(b"abcdef");
        let mut file = LocalFile::open_at(&root, &spec, || false).unwrap();
        assert_eq!(file.size(), 6);
        let mut buf = [0; 3];
        assert_eq!(file.read(&mut buf).unwrap(), 3);
        assert_eq!(&buf, b"abc");
        assert!(file.seek(5));
        assert_eq!(file.read(&mut buf).unwrap(), 1);
        assert_eq!(buf[0], b'f');
        assert_eq!(file.read(&mut buf).unwrap(), 0);
        assert!(file.seek(6));
        assert_eq!(file.read(&mut buf).unwrap(), 0);
        assert!(!file.seek(-1));
        assert!(file.seek(0));
        assert_eq!(file.read(&mut buf).unwrap(), 3);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn corrupt_size_hash_and_cancel_are_rejected() {
        let (root, mut spec) = fixture(b"abcdef");
        assert!(LocalFile::open_at(&root, &spec, || true).is_err());
        spec.bytes += 1;
        assert!(LocalFile::open_at(&root, &spec, || false).is_err());
        spec.bytes -= 1;
        std::fs::write(root.join(format!("{}.mp4", spec.sha256)), b"ABCDEF").unwrap();
        assert!(LocalFile::open_at(&root, &spec, || false).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cancellation_during_hash_is_observed_between_bounded_chunks() {
        let (root, spec) = fixture(&vec![42; 200000]);
        let mut checks = 0;
        let result = LocalFile::open_at(&root, &spec, || {
            checks += 1;
            checks >= 3
        });
        assert!(matches!(result,Err(ref error) if error.kind()==io::ErrorKind::Interrupted));
        assert_eq!(checks, 3);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn traversal_and_oversized_declarations_are_invalid() {
        assert!(!Spec {
            sha256: "../outside".into(),
            bytes: 1
        }
        .valid());
        assert!(!Spec {
            sha256: "a".repeat(64),
            bytes: MAX_BYTES + 1
        }
        .valid());
    }
    #[cfg(unix)]
    #[test]
    fn symlink_file_and_symlink_root_are_rejected() {
        let (root, spec) = fixture(b"abcdef");
        let file = root.join(format!("{}.mp4", spec.sha256));
        let actual = root.join("actual");
        std::fs::rename(&file, &actual).unwrap();
        std::os::unix::fs::symlink(&actual, &file).unwrap();
        assert!(LocalFile::open_at(&root, &spec, || false).is_err());
        let link = root.with_extension("link");
        std::os::unix::fs::symlink(&root, &link).unwrap();
        assert!(LocalFile::open_at(&link, &spec, || false).is_err());
        std::fs::remove_file(link).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
