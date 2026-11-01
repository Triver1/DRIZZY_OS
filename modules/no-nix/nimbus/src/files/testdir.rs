//! Throw-away fixture folders for tests: made-up names, under the system
//! temp dir, gone when dropped.

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
};

static N: AtomicU32 = AtomicU32::new(0);

pub struct TestDir(PathBuf);

impl TestDir {
    pub fn new(tag: &str) -> Self {
        let id = N.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("nimbus-test-{}-{tag}-{id}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Create a small file, and its parents.
    pub fn file(&self, rel: &str) {
        self.write(rel, b"x");
    }

    pub fn write(&self, rel: &str, data: &[u8]) {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, data).unwrap();
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        // A test may leave a folder unreadable.
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
