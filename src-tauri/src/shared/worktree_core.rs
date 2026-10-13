use std::path::PathBuf;

fn sanitize_name(value: &str, fallback: &str) -> String {
    let mut result = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            result.push(ch);
        } else {
            result.push('-');
        }
    }
    let trimmed = result.trim_matches('-').to_string();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        fallback.to_string()
    } else {
        trimmed
    }
}

pub(crate) fn sanitize_worktree_name(branch: &str) -> String {
    sanitize_name(branch, "worktree")
}

pub(crate) fn sanitize_clone_dir_name(name: &str) -> String {
    sanitize_name(name, "copy")
}

pub(crate) fn unique_worktree_path_strict(
    base_dir: &PathBuf,
    name: &str,
) -> Result<PathBuf, String> {
    let candidate = base_dir.join(name);
    if !candidate.exists() && !candidate.is_symlink() {
        return Ok(candidate);
    }
    for index in 2..1000 {
        let next = base_dir.join(format!("{name}-{index}"));
        if !next.exists() && !next.is_symlink() {
            return Ok(next);
        }
    }
    Err(format!(
        "Failed to find an available worktree path under {}.",
        base_dir.display()
    ))
}

pub(crate) fn unique_worktree_path_for_rename(
    base_dir: &PathBuf,
    name: &str,
    current_path: &PathBuf,
) -> Result<PathBuf, String> {
    let candidate = base_dir.join(name);
    if candidate == *current_path {
        return Ok(candidate);
    }
    if !candidate.exists() {
        return Ok(candidate);
    }
    for index in 2..1000 {
        let next = base_dir.join(format!("{name}-{index}"));
        if next == *current_path || !next.exists() {
            return Ok(next);
        }
    }
    Err(format!(
        "Failed to find an available worktree path under {}.",
        base_dir.display()
    ))
}

pub(crate) fn reserve_clone_destination(
    copies_folder: &PathBuf,
    copy_name: &str,
) -> Result<PathBuf, String> {
    let safe_name = sanitize_clone_dir_name(copy_name);
    // Reserve atomically. A check followed by `git clone` can race with another
    // clone and must never lead its failure cleanup to remove someone else's data.
    for index in 1..1000 {
        let name = if index == 1 {
            safe_name.clone()
        } else {
            format!("{safe_name}-{index}")
        };
        let candidate = copies_folder.join(name);
        match std::fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Failed to create clone destination: {error}")),
        }
    }
    Err(format!(
        "Failed to find an available clone path under {}.",
        copies_folder.display()
    ))
}

pub(crate) fn null_device_path() -> &'static str {
    if cfg!(windows) {
        "NUL"
    } else {
        "/dev/null"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clone_reservations_never_reuse_existing_destinations() {
        let root = std::env::temp_dir().join(format!("hopper-clones-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let first = reserve_clone_destination(&root, "copy").unwrap();
        std::fs::write(first.join("keep.txt"), "user data").unwrap();
        let second = reserve_clone_destination(&root, "copy").unwrap();
        assert_ne!(first, second);
        assert!(second.is_dir());
        assert_eq!(
            std::fs::read_to_string(first.join("keep.txt")).unwrap(),
            "user data"
        );
        for index in 3..1000 {
            std::fs::create_dir(root.join(format!("copy-{index}"))).unwrap();
        }
        assert!(reserve_clone_destination(&root, "copy").is_err());
        assert!(unique_worktree_path_strict(&root, "copy").is_err());
        assert_eq!(
            std::fs::read_to_string(first.join("keep.txt")).unwrap(),
            "user data"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dot_names_cannot_resolve_to_a_parent_or_the_destination_root() {
        assert_eq!(sanitize_clone_dir_name("."), "copy");
        assert_eq!(sanitize_clone_dir_name(".."), "copy");
        assert_eq!(sanitize_worktree_name(".."), "worktree");
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlinks_are_occupied_destinations() {
        let root = std::env::temp_dir().join(format!("hopper-clones-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(root.join("missing"), root.join("copy")).unwrap();
        assert_eq!(
            unique_worktree_path_strict(&root, "copy").unwrap(),
            root.join("copy-2")
        );
        assert_eq!(
            reserve_clone_destination(&root, "copy").unwrap(),
            root.join("copy-2")
        );
        assert!(root.join("copy").is_symlink());
        std::fs::remove_dir_all(root).unwrap();
    }
}
