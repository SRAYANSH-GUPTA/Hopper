use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct TextFileResponse {
    pub exists: bool,
    pub content: String,
    pub truncated: bool,
}

fn missing_response() -> TextFileResponse {
    TextFileResponse {
        exists: false,
        content: String::new(),
        truncated: false,
    }
}

fn resolve_root(
    root: &Path,
    root_context: &str,
    root_may_be_missing: bool,
) -> Result<Option<PathBuf>, String> {
    if root_may_be_missing && !root.exists() {
        return Ok(None);
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|err| format!("Failed to resolve {root_context}: {err}"))?;
    if !canonical_root.is_dir() {
        return Err(format!("{root_context} is not a directory"));
    }
    Ok(Some(canonical_root))
}

fn resolve_or_create_root(root: &Path, root_context: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(root)
        .map_err(|err| format!("Failed to create {root_context}: {err}"))?;
    let canonical_root = root
        .canonicalize()
        .map_err(|err| format!("Failed to resolve {root_context}: {err}"))?;
    if !canonical_root.is_dir() {
        return Err(format!("{root_context} is not a directory"));
    }
    Ok(canonical_root)
}

pub(crate) fn read_text_file_within(
    root: &Path,
    filename: &str,
    root_may_be_missing: bool,
    root_context: &str,
    file_context: &str,
    allow_external_symlink_target: bool,
) -> Result<TextFileResponse, String> {
    let Some(canonical_root) = resolve_root(root, root_context, root_may_be_missing)? else {
        return Ok(missing_response());
    };

    let candidate = canonical_root.join(filename);
    if !candidate.exists() {
        return Ok(missing_response());
    }

    let candidate_is_symlink = std::fs::symlink_metadata(&candidate)
        .map_err(|err| format!("Failed to open {file_context}: {err}"))?
        .file_type()
        .is_symlink();
    let canonical_path = candidate
        .canonicalize()
        .map_err(|err| format!("Failed to open {file_context}: {err}"))?;
    if !canonical_path.starts_with(&canonical_root)
        && !(allow_external_symlink_target && candidate_is_symlink)
    {
        return Err(format!("Invalid {file_context} path"));
    }

    let mut file = File::open(&canonical_path)
        .map_err(|err| format!("Failed to open {file_context}: {err}"))?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)
        .map_err(|err| format!("Failed to read {file_context}: {err}"))?;
    let content =
        String::from_utf8(buffer).map_err(|_| format!("{file_context} is not valid UTF-8"))?;

    Ok(TextFileResponse {
        exists: true,
        content,
        truncated: false,
    })
}

pub(crate) fn write_text_file_within(
    root: &Path,
    filename: &str,
    content: &str,
    create_root: bool,
    root_context: &str,
    file_context: &str,
    allow_external_symlink_target: bool,
) -> Result<(), String> {
    if !matches!(
        Path::new(filename)
            .components()
            .collect::<Vec<_>>()
            .as_slice(),
        [Component::Normal(_)]
    ) {
        return Err(format!("Invalid {file_context} path"));
    }
    let canonical_root = if create_root {
        resolve_or_create_root(root, root_context)?
    } else {
        resolve_root(root, root_context, false)?
            .ok_or_else(|| format!("Failed to resolve {root_context}"))?
    };

    let candidate = canonical_root.join(filename);
    if !candidate.starts_with(&canonical_root) {
        return Err(format!("Invalid {file_context} path"));
    }

    // `exists()` follows symlinks and reports false for a dangling link. Treating
    // that case as a new file would create its target outside the workspace.
    let metadata = match std::fs::symlink_metadata(&candidate) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("Failed to resolve {file_context}: {error}")),
    };
    let target_exists = metadata.is_some();
    let target_path = if let Some(metadata) = metadata {
        let candidate_is_symlink = metadata.file_type().is_symlink();
        let canonical_path = candidate
            .canonicalize()
            .map_err(|err| format!("Failed to resolve {file_context}: {err}"))?;
        if !canonical_path.starts_with(&canonical_root)
            && !(allow_external_symlink_target && candidate_is_symlink)
        {
            return Err(format!("Invalid {file_context} path"));
        }
        canonical_path
    } else {
        candidate
    };

    let mut options = std::fs::OpenOptions::new();
    options
        .write(true)
        .truncate(true)
        .create_new(!target_exists);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Even if a link is introduced after the checks above, do not follow it.
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options
        .open(&target_path)
        .and_then(|mut file| file.write_all(content.as_bytes()))
        .map_err(|err| format!("Failed to write {file_context}: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("hopper-file-io-{}", Uuid::new_v4()))
    }

    #[test]
    fn read_returns_missing_when_root_absent() {
        let root = temp_dir();
        let response =
            read_text_file_within(&root, "AGENTS.md", true, "CODEX_HOME", "AGENTS.md", false)
                .expect("read should succeed");
        assert!(!response.exists);
        assert!(response.content.is_empty());
    }

    #[test]
    fn write_creates_root_and_round_trips() {
        let root = temp_dir();
        write_text_file_within(
            &root,
            "AGENTS.md",
            "hello",
            true,
            "CODEX_HOME",
            "AGENTS.md",
            false,
        )
        .expect("write should succeed");
        let response =
            read_text_file_within(&root, "AGENTS.md", false, "CODEX_HOME", "AGENTS.md", false)
                .expect("read should succeed");
        assert!(response.exists);
        assert_eq!(response.content, "hello");
    }

    #[cfg(unix)]
    #[test]
    fn write_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let root = temp_dir();
        let outside = temp_dir();
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::create_dir_all(&outside).expect("create outside");

        let outside_file = outside.join("AGENTS.md");
        std::fs::write(&outside_file, "outside").expect("seed outside file");

        let link_path = root.join("AGENTS.md");
        symlink(&outside_file, &link_path).expect("create symlink");

        let error = write_text_file_within(
            &root,
            "AGENTS.md",
            "updated",
            false,
            "workspace root",
            "AGENTS.md",
            false,
        )
        .expect_err("should reject symlink escape");
        assert!(error.contains("Invalid AGENTS.md path"));
    }

    #[cfg(unix)]
    #[test]
    fn write_rejects_dangling_symlink_escape_without_creating_its_target() {
        use std::os::unix::fs::symlink;
        let root = temp_dir();
        let outside = temp_dir();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let outside_file = outside.join("not-created.md");
        symlink(&outside_file, root.join("AGENTS.md")).unwrap();

        assert!(write_text_file_within(
            &root,
            "AGENTS.md",
            "escape",
            false,
            "workspace root",
            "AGENTS.md",
            false,
        )
        .is_err());
        assert!(!outside_file.exists());
        assert!(root.join("AGENTS.md").is_symlink());
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn write_rejects_relative_traversal_to_a_missing_file() {
        let root = temp_dir();
        std::fs::create_dir_all(root.join("workspace")).unwrap();
        assert!(write_text_file_within(
            &root.join("workspace"),
            "../outside.md",
            "escape",
            false,
            "workspace root",
            "AGENTS.md",
            false,
        )
        .is_err());
        assert!(!root.join("outside.md").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn read_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let root = temp_dir();
        let outside = temp_dir();
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::create_dir_all(&outside).expect("create outside");

        let outside_file = outside.join("AGENTS.md");
        std::fs::write(&outside_file, "outside").expect("seed outside file");

        let link_path = root.join("AGENTS.md");
        symlink(&outside_file, &link_path).expect("create symlink");

        let error = read_text_file_within(
            &root,
            "AGENTS.md",
            false,
            "workspace root",
            "AGENTS.md",
            false,
        )
        .expect_err("should reject symlink escape");
        assert!(error.contains("Invalid AGENTS.md path"));
    }

    #[cfg(unix)]
    #[test]
    fn read_allows_external_symlink_when_enabled() {
        use std::os::unix::fs::symlink;

        let root = temp_dir();
        let outside = temp_dir();
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::create_dir_all(&outside).expect("create outside");

        let outside_file = outside.join("AGENTS.md");
        std::fs::write(&outside_file, "outside").expect("seed outside file");

        let link_path = root.join("AGENTS.md");
        symlink(&outside_file, &link_path).expect("create symlink");

        let response =
            read_text_file_within(&root, "AGENTS.md", false, "CODEX_HOME", "AGENTS.md", true)
                .expect("read should succeed");
        assert!(response.exists);
        assert_eq!(response.content, "outside");
    }

    #[cfg(unix)]
    #[test]
    fn write_allows_external_symlink_when_enabled() {
        use std::os::unix::fs::symlink;

        let root = temp_dir();
        let outside = temp_dir();
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::create_dir_all(&outside).expect("create outside");

        let outside_file = outside.join("AGENTS.md");
        std::fs::write(&outside_file, "outside").expect("seed outside file");

        let link_path = root.join("AGENTS.md");
        symlink(&outside_file, &link_path).expect("create symlink");

        write_text_file_within(
            &root,
            "AGENTS.md",
            "updated",
            false,
            "CODEX_HOME",
            "AGENTS.md",
            true,
        )
        .expect("write should succeed");

        let updated = std::fs::read_to_string(&outside_file).expect("read outside file");
        assert_eq!(updated, "updated");
    }

    #[cfg(unix)]
    #[test]
    fn config_symlink_escape_is_still_rejected_when_disabled() {
        use std::os::unix::fs::symlink;

        let root = temp_dir();
        let outside = temp_dir();
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::create_dir_all(&outside).expect("create outside");

        let outside_file = outside.join("config.toml");
        std::fs::write(&outside_file, "name = \"outside\"").expect("seed outside file");

        let link_path = root.join("config.toml");
        symlink(&outside_file, &link_path).expect("create symlink");

        let error = read_text_file_within(
            &root,
            "config.toml",
            false,
            "CODEX_HOME",
            "config.toml",
            false,
        )
        .expect_err("should reject symlink escape");
        assert!(error.contains("Invalid config.toml path"));
    }
}
