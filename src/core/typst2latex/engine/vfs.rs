//! Virtual File System trait for module loading.
//!
//! This module provides an abstraction for file system operations,
//! allowing the evaluator to work with different file sources
//! (real files, in-memory content, bundled modules, etc.)

use std::collections::HashMap;
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

/// Result type for VFS operations.
pub type VfsResult<T> = Result<T, VfsError>;

/// Error type for VFS operations.
#[derive(Debug, Clone)]
pub enum VfsError {
    /// File not found.
    NotFound(String),
    /// Read error.
    ReadError(String),
    /// Invalid path.
    InvalidPath(String),
    /// Permission denied.
    PermissionDenied(String),
}

impl std::fmt::Display for VfsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VfsError::NotFound(path) => write!(f, "file not found: {}", path),
            VfsError::ReadError(msg) => write!(f, "read error: {}", msg),
            VfsError::InvalidPath(path) => write!(f, "invalid path: {}", path),
            VfsError::PermissionDenied(path) => write!(f, "permission denied: {}", path),
        }
    }
}

impl std::error::Error for VfsError {}

/// Virtual File System trait.
///
/// Implementations of this trait provide file system operations
/// for loading modules and other resources.
pub trait VirtualFileSystem: Send + Sync {
    /// Read a file as text.
    fn read_text(&self, path: &str) -> VfsResult<String>;

    /// Read a file as bytes.
    fn read_bytes(&self, path: &str) -> VfsResult<Vec<u8>>;

    /// Check if a file exists.
    fn exists(&self, path: &str) -> bool;

    /// Resolve a relative path against a base path.
    fn resolve(&self, base: &str, relative: &str) -> VfsResult<String>;

    /// Get the current working directory.
    fn cwd(&self) -> VfsResult<String>;
}

/// A no-op VFS that always returns errors.
///
/// Useful for sandboxed environments where file access is not allowed.
pub struct NoopVfs;

impl VirtualFileSystem for NoopVfs {
    fn read_text(&self, path: &str) -> VfsResult<String> {
        Err(VfsError::NotFound(path.to_string()))
    }

    fn read_bytes(&self, path: &str) -> VfsResult<Vec<u8>> {
        Err(VfsError::NotFound(path.to_string()))
    }

    fn exists(&self, _path: &str) -> bool {
        false
    }

    fn resolve(&self, _base: &str, relative: &str) -> VfsResult<String> {
        Ok(relative.to_string())
    }

    fn cwd(&self) -> VfsResult<String> {
        Ok(".".to_string())
    }
}

/// An in-memory VFS for testing and bundled content.
#[derive(Debug, Clone, Default)]
pub struct MemoryVfs {
    files: HashMap<String, Vec<u8>>,
    cwd: String,
}

impl MemoryVfs {
    /// Create a new empty memory VFS.
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
            cwd: ".".to_string(),
        }
    }

    /// Add a file to the VFS.
    pub fn add_file(&mut self, path: impl Into<String>, content: impl Into<Vec<u8>>) {
        self.files.insert(path.into(), content.into());
    }

    /// Add a text file to the VFS.
    pub fn add_text_file(&mut self, path: impl Into<String>, content: impl Into<String>) {
        self.add_file(path, content.into().into_bytes());
    }

    /// Set the current working directory.
    pub fn set_cwd(&mut self, cwd: impl Into<String>) {
        self.cwd = cwd.into();
    }
}

impl VirtualFileSystem for MemoryVfs {
    fn read_text(&self, path: &str) -> VfsResult<String> {
        self.files
            .get(path)
            .map(|bytes| String::from_utf8_lossy(bytes).to_string())
            .ok_or_else(|| VfsError::NotFound(path.to_string()))
    }

    fn read_bytes(&self, path: &str) -> VfsResult<Vec<u8>> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| VfsError::NotFound(path.to_string()))
    }

    fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    fn resolve(&self, base: &str, relative: &str) -> VfsResult<String> {
        // Simple path resolution
        if relative.starts_with('/') || relative.starts_with("@") {
            return Ok(relative.to_string());
        }

        let base_path = Path::new(base);
        let parent = base_path.parent().unwrap_or(Path::new(""));
        let resolved = parent.join(relative);

        // Normalize the path
        let mut components: Vec<&str> = Vec::new();
        for component in resolved.components() {
            match component {
                std::path::Component::ParentDir => {
                    components.pop();
                }
                std::path::Component::CurDir => {}
                std::path::Component::Normal(s) => {
                    if let Some(s) = s.to_str() {
                        components.push(s);
                    }
                }
                std::path::Component::RootDir => {
                    components.clear();
                    components.push("");
                }
                std::path::Component::Prefix(_) => {}
            }
        }

        Ok(components.join("/"))
    }

    fn cwd(&self) -> VfsResult<String> {
        Ok(self.cwd.clone())
    }
}

/// A real file system VFS.
#[cfg(not(target_arch = "wasm32"))]
pub struct RealVfs {
    root: PathBuf,
}

#[cfg(not(target_arch = "wasm32"))]
impl RealVfs {
    /// Create a new real VFS rooted at the given path.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn resolve_path(&self, path: &str) -> PathBuf {
        if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            self.root.join(path)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl VirtualFileSystem for RealVfs {
    fn read_text(&self, path: &str) -> VfsResult<String> {
        let full_path = self.resolve_path(path);
        std::fs::read_to_string(&full_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                VfsError::NotFound(path.to_string())
            } else if e.kind() == std::io::ErrorKind::PermissionDenied {
                VfsError::PermissionDenied(path.to_string())
            } else {
                VfsError::ReadError(e.to_string())
            }
        })
    }

    fn read_bytes(&self, path: &str) -> VfsResult<Vec<u8>> {
        let full_path = self.resolve_path(path);
        std::fs::read(&full_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                VfsError::NotFound(path.to_string())
            } else if e.kind() == std::io::ErrorKind::PermissionDenied {
                VfsError::PermissionDenied(path.to_string())
            } else {
                VfsError::ReadError(e.to_string())
            }
        })
    }

    fn exists(&self, path: &str) -> bool {
        self.resolve_path(path).exists()
    }

    fn resolve(&self, base: &str, relative: &str) -> VfsResult<String> {
        if relative.starts_with('/') || relative.starts_with("@") {
            return Ok(relative.to_string());
        }

        let base_path = Path::new(base);
        let parent = base_path.parent().unwrap_or(Path::new(""));
        let resolved = parent.join(relative);

        resolved
            .to_str()
            .map(|s| s.to_string())
            .ok_or_else(|| VfsError::InvalidPath(relative.to_string()))
    }

    fn cwd(&self) -> VfsResult<String> {
        self.root
            .to_str()
            .map(|s| s.to_string())
            .ok_or_else(|| VfsError::InvalidPath("cwd".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_vfs() {
        let mut vfs = MemoryVfs::new();
        vfs.add_text_file("test.typ", "#let x = 1");
        vfs.add_text_file("lib/utils.typ", "#let add(a, b) = a + b");

        assert!(vfs.exists("test.typ"));
        assert!(vfs.exists("lib/utils.typ"));
        assert!(!vfs.exists("nonexistent.typ"));

        assert_eq!(vfs.read_text("test.typ").unwrap(), "#let x = 1");
    }

    #[test]
    fn test_memory_vfs_resolve() {
        let vfs = MemoryVfs::new();

        // Resolve relative to base
        assert_eq!(
            vfs.resolve("src/main.typ", "utils.typ").unwrap(),
            "src/utils.typ"
        );

        // Resolve parent reference
        assert_eq!(
            vfs.resolve("src/lib/main.typ", "../utils.typ").unwrap(),
            "src/utils.typ"
        );

        // Absolute paths are returned as-is
        assert_eq!(
            vfs.resolve("src/main.typ", "/root.typ").unwrap(),
            "/root.typ"
        );
    }

    #[test]
    fn test_noop_vfs() {
        let vfs = NoopVfs;

        assert!(!vfs.exists("anything.typ"));
        assert!(vfs.read_text("anything.typ").is_err());
    }
}
