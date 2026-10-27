//! Optional, content-addressed private background media. Open and verification run only
//! on the existing demux worker; no URL parser or network credential policy is relaxed.
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

pub const ROOT: &str = "/media/internal/pinpin-family/cache";
pub const MAX_BYTES: u64 = 128 * 1024 * 1024;
const RECEIPT_MAX_BYTES: u64 = 4096;

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Identity {
    device: String,
    inode: String,
    size: String,
    modified_ns: String,
    changed_ns: String,
}
impl Identity {
    fn of(metadata: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        Self {
            device: metadata.dev().to_string(), inode: metadata.ino().to_string(),
            size: metadata.len().to_string(),
            modified_ns: ((metadata.mtime() as i128) * 1_000_000_000 + metadata.mtime_nsec() as i128).to_string(),
            changed_ns: ((metadata.ctime() as i128) * 1_000_000_000 + metadata.ctime_nsec() as i128).to_string(),
        }
    }
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt { version: u32, sha256: String, bytes: u64, file: Identity }

// This is an administrative trust boundary, not a user-configurable verification bypass.
// Keep the policy pure so non-root development hosts can exercise every ownership case.
#[derive(Clone, Copy)]
struct Protection { uid: u32, mode: u32, regular: bool }
impl Protection {
    fn of(metadata: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        Self { uid: metadata.uid(), mode: metadata.mode(), regular: metadata.is_file() }
    }
}
fn receipt_matches(root: Protection, file: Protection, stamp: Protection, identity: &Identity, spec: &Spec, bytes: &[u8]) -> bool {
    if !spec.valid() || root.uid != 0 || root.mode & 0o022 != 0
        || file.uid != 0 || !file.regular || file.mode & 0o222 != 0
        || stamp.uid != 0 || !stamp.regular || stamp.mode & 0o022 != 0
        || bytes.len() as u64 > RECEIPT_MAX_BYTES { return false; }
    let Ok(receipt) = serde_json::from_slice::<Receipt>(bytes) else { return false; };
    receipt.version == 1 && receipt.sha256 == spec.sha256 && receipt.bytes == spec.bytes && &receipt.file == identity
}
fn trusted_receipt(root: &Path, file: &File, spec: &Spec) -> bool {
    use std::os::unix::fs::OpenOptionsExt;
    let check = || -> io::Result<bool> {
        let root_metadata = std::fs::symlink_metadata(root)?;
        if !root_metadata.is_dir() { return Ok(false); }
        let before = file.metadata()?;
        let mut stamp = std::fs::OpenOptions::new().read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(root.join(format!("{}.verified.json", spec.sha256)))?;
        let metadata = stamp.metadata()?;
        if !metadata.is_file() || metadata.len() > RECEIPT_MAX_BYTES { return Ok(false); }
        let mut bytes = Vec::new();
        (&mut stamp).take(RECEIPT_MAX_BYTES + 1).read_to_end(&mut bytes)?;
        let identity = Identity::of(&before);
        Ok(receipt_matches(Protection::of(&root_metadata), Protection::of(&before), Protection::of(&metadata), &identity, spec, &bytes)
            && identity == Identity::of(&file.metadata()?))
    };
    check().unwrap_or(false)
}

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
        if trusted_receipt(root, &file, spec) {
            if cancelled() { return Err(abort()); }
            crate::player::log("local cache: verified installation receipt reused");
            // The same read-only opened inode is handed to AVIO, at its original offset zero.
            return Ok(Self { file, size: spec.bytes });
        }
        crate::player::log("local cache: installation receipt unavailable; hashing local media");
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
    fn receipt_fixture()->(Protection,Protection,Protection,Identity,Spec,Vec<u8>){
        let root=Protection{uid:0,mode:0o755,regular:false};
        let file=Protection{uid:0,mode:0o444,regular:true};
        let stamp=Protection{uid:0,mode:0o444,regular:true};
        let identity=Identity{device:"7".into(),inode:"9007199254740993".into(),size:"6".into(),modified_ns:"1793087207043977101".into(),changed_ns:"1793087208939977099".into()};
        let spec=Spec{sha256:"a".repeat(64),bytes:6};
        let bytes=serde_json::to_vec(&serde_json::json!({"version":1,"sha256":spec.sha256,"bytes":6,"file":{"device":identity.device,"inode":identity.inode,"size":identity.size,"modifiedNs":identity.modified_ns,"changedNs":identity.changed_ns}})).unwrap();
        (root,file,stamp,identity,spec,bytes)
    }
    #[test]
    fn root_receipt_matches_exact_identity_without_losing_nanoseconds(){
        let (root,file,stamp,identity,spec,bytes)=receipt_fixture();
        assert!(receipt_matches(root,file,stamp,&identity,&spec,&bytes));
        for field in [0,1,2,3,4]{
            let mut changed=identity.clone();
            match field{0=>changed.device="8".into(),1=>changed.inode="9007199254740994".into(),2=>changed.size="7".into(),3=>changed.modified_ns="1793087207043977102".into(),_=>changed.changed_ns="1793087208939977100".into()}
            assert!(!receipt_matches(root,file,stamp,&changed,&spec,&bytes));
        }
    }
    #[test]
    fn receipt_trust_requires_root_owned_protected_objects_and_bounded_exact_schema(){
        let (root,file,stamp,identity,spec,bytes)=receipt_fixture();
        for index in 0..3{
            let mut policy=[root,file,stamp];policy[index].uid=501;
            assert!(!receipt_matches(policy[0],policy[1],policy[2],&identity,&spec,&bytes));
            policy=[root,file,stamp];policy[index].mode|=0o020;
            assert!(!receipt_matches(policy[0],policy[1],policy[2],&identity,&spec,&bytes));
        }
        assert!(!receipt_matches(root,Protection{mode:0o644,..file},stamp,&identity,&spec,&bytes));
        assert!(!receipt_matches(root,file,Protection{regular:false,..stamp},&identity,&spec,&bytes));
        assert!(!receipt_matches(root,file,stamp,&identity,&spec,&vec![b' ';RECEIPT_MAX_BYTES as usize+1]));
        let wrong=Spec{sha256:"b".repeat(64),bytes:6};assert!(!receipt_matches(root,file,stamp,&identity,&wrong,&bytes));
        let wrong=Spec{sha256:spec.sha256.clone(),bytes:7};assert!(!receipt_matches(root,file,stamp,&identity,&wrong,&bytes));
        assert!(!receipt_matches(root,file,stamp,&identity,&spec,b"{\"version\":2}"));
    }
    #[test]
    fn untrusted_symlink_receipt_never_skips_content_verification(){
        let (root,spec)=fixture(b"abcdef");
        let stamp=root.join(format!("{}.verified.json",spec.sha256));
        std::os::unix::fs::symlink(root.join("missing"),stamp).unwrap();
        std::fs::write(root.join(format!("{}.mp4",spec.sha256)),b"ABCDEF").unwrap();
        assert!(LocalFile::open_at(&root,&spec,||false).is_err());
        std::fs::remove_dir_all(root).unwrap();
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
