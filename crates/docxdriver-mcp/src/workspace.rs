use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use docxdriver_core::SourceHash;
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct Workspace {
    root: Arc<PathBuf>,
    dir: Arc<Dir>,
}

#[derive(Debug)]
pub struct DocumentBytes {
    pub bytes: Vec<u8>,
    pub source: SourceHash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostError {
    pub code: &'static str,
    pub message: String,
}

impl HostError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for HostError {}

impl Workspace {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, HostError> {
        let root = fs::canonicalize(root.as_ref()).map_err(|error| {
            HostError::new(
                "workspace_unavailable",
                format!("resolve workspace {}: {error}", root.as_ref().display()),
            )
        })?;
        if !root.is_dir() {
            return Err(HostError::new(
                "workspace_unavailable",
                format!("workspace is not a directory: {}", root.display()),
            ));
        }
        let dir = Dir::open_ambient_dir(&root, ambient_authority()).map_err(|error| {
            HostError::new(
                "workspace_unavailable",
                format!("open workspace {}: {error}", root.display()),
            )
        })?;
        Ok(Self {
            root: Arc::new(root),
            dir: Arc::new(dir),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn read_existing(&self, path: &str) -> Result<DocumentBytes, HostError> {
        let relative = self.relative(path)?;
        self.resolve_existing(path)?;
        let bytes = self
            .dir
            .read(&relative)
            .map_err(|error| self.cap_error("read_failed", path, error))?;
        let source = SourceHash::from_bytes(&bytes);
        Ok(DocumentBytes { bytes, source })
    }

    pub fn read_text_existing(&self, path: &str) -> Result<String, HostError> {
        let relative = self.relative(path)?;
        self.resolve_existing(path)?;
        self.dir
            .read_to_string(&relative)
            .map_err(|error| self.cap_error("read_failed", path, error))
    }

    pub fn resolve_existing(&self, path: &str) -> Result<PathBuf, HostError> {
        let lexical = self.lexical(path)?;
        let actual = fs::canonicalize(&lexical).map_err(|error| {
            HostError::new("path_unavailable", format!("resolve {path}: {error}"))
        })?;
        if !self.inside(&actual) {
            return Err(HostError::new(
                "path_escape",
                format!("path escapes workspace: {path}"),
            ));
        }
        Ok(actual)
    }

    pub fn resolve_new(&self, path: &str) -> Result<PathBuf, HostError> {
        let relative = self.relative(path)?;
        match self.dir.symlink_metadata(&relative) {
            Ok(_) => {
                return Err(HostError::new(
                    "destination_exists",
                    format!("refusing to overwrite {path}"),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(self.cap_error("path_escape", path, error)),
        }
        let parent = relative.parent().unwrap_or_else(|| Path::new(""));
        self.dir.create_dir_all(parent).map_err(|error| {
            HostError::new(
                "path_escape",
                format!("create output directory for {path}: {error}"),
            )
        })?;
        match self.dir.symlink_metadata(&relative) {
            Ok(_) => Err(HostError::new(
                "destination_exists",
                format!("refusing to overwrite {path}"),
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(relative),
            Err(error) => Err(self.cap_error("path_escape", path, error)),
        }
    }

    pub fn create_exclusive(&self, path: &str, bytes: &[u8]) -> Result<PathBuf, HostError> {
        let target = self.resolve_new(path)?;
        let temp = self.write_temp(&target, path, bytes, "create")?;
        let result = match self.dir.symlink_metadata(&target) {
            Ok(_) => Err(HostError::new(
                "destination_exists",
                format!("destination appeared before create: {path}"),
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => self
                .dir
                .hard_link(&temp, &self.dir, &target)
                .map_err(|error| {
                    if error.kind() == io::ErrorKind::AlreadyExists {
                        HostError::new(
                            "destination_exists",
                            format!("destination appeared before create: {path}"),
                        )
                    } else {
                        HostError::new(
                            "create_failed",
                            format!("create {path} exclusively: {error}"),
                        )
                    }
                }),
            Err(error) => Err(self.cap_error("path_escape", path, error)),
        };
        let _ = self.dir.remove_file(&temp);
        result.map(|_| self.root.join(target))
    }

    pub fn write_existing_atomic(
        &self,
        path: &str,
        expected: &SourceHash,
        bytes: &[u8],
    ) -> Result<PathBuf, HostError> {
        let target = self.relative(path)?;
        self.resolve_existing(path)?;
        let current = self
            .dir
            .read(&target)
            .map_err(|error| self.cap_error("read_failed", path, error))?;
        if SourceHash::from_bytes(&current) != *expected {
            return Err(HostError::new(
                "source_changed",
                "source changed before commit",
            ));
        }
        let temp = self.write_temp(&target, path, bytes, "replace")?;
        let result = (|| {
            let current = self
                .dir
                .read(&target)
                .map_err(|error| self.cap_error("read_failed", path, error))?;
            if SourceHash::from_bytes(&current) != *expected {
                return Err(HostError::new(
                    "source_changed",
                    "source changed before commit",
                ));
            }
            self.dir
                .rename(&temp, &self.dir, &target)
                .map_err(|error| {
                    HostError::new("rename_failed", format!("atomic replace {path}: {error}"))
                })?;
            Ok(self.root.join(&target))
        })();
        let _ = self.dir.remove_file(&temp);
        result
    }

    fn write_temp(
        &self,
        target: &Path,
        path: &str,
        bytes: &[u8],
        operation: &str,
    ) -> Result<PathBuf, HostError> {
        let parent = target.parent().unwrap_or_else(|| Path::new(""));
        let name = target
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("document.docx");
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        for counter in 0..128u32 {
            let candidate = parent.join(format!(
                ".{name}.docxdriver-{}-{stamp}-{counter}.tmp",
                std::process::id()
            ));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            match self.dir.open_with(&candidate, &options) {
                Ok(mut file) => {
                    let result = file.write_all(bytes).and_then(|_| file.sync_all());
                    drop(file);
                    match result {
                        Ok(()) => return Ok(candidate),
                        Err(error) => {
                            let _ = self.dir.remove_file(&candidate);
                            return Err(HostError::new(
                                "write_failed",
                                format!("write {operation} temporary output for {path}: {error}"),
                            ));
                        }
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(self.cap_error("tempfile_failed", path, error));
                }
            }
        }
        Err(HostError::new(
            "tempfile_failed",
            format!("could not allocate {operation} temporary file"),
        ))
    }

    fn relative(&self, path: &str) -> Result<PathBuf, HostError> {
        let lexical = self.lexical(path)?;
        let relative = lexical.strip_prefix(self.root()).map_err(|_| {
            HostError::new("path_escape", format!("path escapes workspace: {path}"))
        })?;
        if relative.as_os_str().is_empty() {
            return Err(HostError::new(
                "invalid_path",
                format!("path must name a file: {path}"),
            ));
        }
        Ok(relative.to_path_buf())
    }

    fn lexical(&self, path: &str) -> Result<PathBuf, HostError> {
        if path.trim().is_empty() {
            return Err(HostError::new("invalid_path", "path must be non-empty"));
        }
        let input = Path::new(path);
        let absolute = if input.is_absolute() {
            input.to_path_buf()
        } else {
            self.root.join(input)
        };
        let mut normalized = PathBuf::new();
        for component in absolute.components() {
            match component {
                Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
                Component::RootDir => normalized.push(component.as_os_str()),
                Component::CurDir => {}
                Component::ParentDir => {
                    if !normalized.pop() {
                        return Err(HostError::new(
                            "path_escape",
                            format!("path escapes workspace: {path}"),
                        ));
                    }
                }
                Component::Normal(part) => normalized.push(part),
            }
        }
        if !self.inside(&normalized) {
            return Err(HostError::new(
                "path_escape",
                format!("path escapes workspace: {path}"),
            ));
        }
        Ok(normalized)
    }

    fn inside(&self, path: &Path) -> bool {
        path == self.root() || path.starts_with(self.root())
    }

    fn cap_error(&self, default_code: &'static str, path: &str, error: io::Error) -> HostError {
        let code = if matches!(
            error.kind(),
            io::ErrorKind::PermissionDenied | io::ErrorKind::InvalidInput
        ) {
            "path_escape"
        } else {
            default_code
        };
        HostError::new(code, format!("{path}: {error}"))
    }
}
