use std::path::{Component, Path, PathBuf};

/// Guardrails for path and command lengths we consider safe to process.
const MAX_SAFE_PATH_LEN: usize = 4096;
const MAX_SHELL_COMMAND_LEN: usize = 32_768;

pub fn validate_path(path: &str) -> bool {
    if path.contains('\0') {
        return false;
    }

    if path.len() > MAX_SAFE_PATH_LEN {
        return false;
    }

    true
}

pub fn validate_shell_command(command: &str) -> bool {
    if command.contains('\0') {
        return false;
    }

    if command.len() > MAX_SHELL_COMMAND_LEN {
        return false;
    }

    true
}

pub fn sanitize_path(path: &str) -> String {
    let cleaned: String = path.chars().filter(|c| !c.is_control() || *c == '\t').collect();

    cleaned.trim().to_string()
}

pub fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    let mut anchored = false;

    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                anchored = true;
                normalized.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() && !anchored {
                    normalized.push("..");
                }
            }
            Component::Normal(_) => normalized.push(component.as_os_str()),
        }
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn test_validate_path() {
        assert!(validate_path("src/main.rs"));
        assert!(validate_path("/absolute/path/file.c"));
        assert!(validate_path("relative/path/file.c"));
        assert!(validate_path("../../../etc/passwd"));
        assert!(validate_path("src/../../etc/passwd"));
        assert!(validate_path("src/../.."));

        assert!(!validate_path("file\0.c"));

        let long_path = "a".repeat(5000);
        assert!(!validate_path(&long_path));
    }

    #[test]
    fn test_validate_shell_command() {
        assert!(validate_shell_command("gcc -c file.c"));
        assert!(validate_shell_command("make all"));

        assert!(!validate_shell_command("gcc\0 -c file.c"));

        let long_command = "gcc ".to_string() + &"-D".repeat(20000);
        assert!(!validate_shell_command(&long_command));
    }

    #[test]
    fn test_sanitize_path() {
        assert_eq!(sanitize_path("  path/to/file  "), "path/to/file");
        assert_eq!(sanitize_path("path\0to\0file"), "pathtofile");
        assert_eq!(sanitize_path("path\nto\rfile"), "pathtofile");
        assert_eq!(sanitize_path("path\tto\tfile"), "path\tto\tfile");
    }

    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path(Path::new("/project/./src/../file.c")), PathBuf::from("/project/file.c"));
        assert_eq!(normalize_path(Path::new("src/./dir/../file.c")), PathBuf::from("src/file.c"));
        assert_eq!(normalize_path(Path::new("../src/../file.c")), PathBuf::from("../file.c"));
    }
}
