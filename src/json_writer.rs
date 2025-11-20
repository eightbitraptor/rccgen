use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, BufWriter};
use std::path::Path;

/// Represents a single compilation command in the JSON database
/// Follows the Clang JSON Compilation Database specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileCommand {
    pub directory: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    pub file: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

impl CompileCommand {
    /// Creates a new CompileCommand with arguments
    pub fn with_arguments(directory: String, arguments: Vec<String>, file: String) -> Self {
        Self {
            directory,
            arguments: Some(arguments),
            command: None,
            file,
            output: None,
        }
    }

    /// Converts a command string to arguments list
    pub fn from_command_string(directory: String, command: String, file: String) -> Self {
        let arguments = crate::tokenizer::tokenize(&command);
        Self {
            directory,
            arguments: Some(arguments),
            command: None,
            file,
            output: None,
        }
    }
}

/// Writes compilation commands to a JSON file with buffered I/O
pub fn write_compile_commands(path: &Path, commands: &[CompileCommand]) -> io::Result<()> {
    let file = fs::File::create(path)?;
    let writer = BufWriter::new(file);
    serde_json::to_writer_pretty(writer, commands)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_compile_command_serialization() {
        let cmd = CompileCommand::with_arguments(
            "/project".to_string(),
            vec!["gcc".to_string(), "-c".to_string(), "file.c".to_string()],
            "/project/file.c".to_string(),
        );

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"directory\""));
        assert!(json.contains("\"arguments\""));
        assert!(json.contains("\"file\""));
        assert!(!json.contains("\"command\""));

        let deserialized: CompileCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.directory, cmd.directory);
        assert_eq!(deserialized.arguments, cmd.arguments);
        assert_eq!(deserialized.file, cmd.file);
    }

    #[test]
    fn test_write_compile_commands() {
        let temp_dir = std::env::temp_dir();
        let output_path = temp_dir.join("test_compile_commands.json");

        let commands = vec![
            CompileCommand::with_arguments(
                "/project".to_string(),
                vec![
                    "gcc".to_string(),
                    "-c".to_string(),
                    "-Wall".to_string(),
                    "file1.c".to_string(),
                ],
                "/project/file1.c".to_string(),
            ),
            CompileCommand::with_arguments(
                "/project".to_string(),
                vec![
                    "gcc".to_string(),
                    "-c".to_string(),
                    "-Wall".to_string(),
                    "file2.c".to_string(),
                ],
                "/project/file2.c".to_string(),
            ),
        ];

        write_compile_commands(&output_path, &commands).unwrap();

        assert!(output_path.exists());
        let content = fs::read_to_string(&output_path).unwrap();
        let parsed: Vec<CompileCommand> = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].file, "/project/file1.c");
        assert_eq!(parsed[1].file, "/project/file2.c");

        fs::remove_file(&output_path).unwrap_or(());
    }

    #[test]
    fn test_empty_commands() {
        let temp_dir = std::env::temp_dir();
        let output_path = temp_dir.join("test_empty_commands.json");

        let commands: Vec<CompileCommand> = vec![];
        write_compile_commands(&output_path, &commands).unwrap();

        let content = fs::read_to_string(&output_path).unwrap();
        assert_eq!(content.trim(), "[]");

        fs::remove_file(&output_path).unwrap_or(());
    }

    #[test]
    fn test_special_characters_in_command() {
        let cmd = CompileCommand::from_command_string(
            "/project".to_string(),
            "gcc -D\"MACRO=\\\"quoted\\\"\" -I/path\\ with\\ spaces file.c".to_string(),
            "/project/file.c".to_string(),
        );

        let json = serde_json::to_string(&cmd).unwrap();
        let deserialized: CompileCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.arguments, cmd.arguments);

        if let Some(args) = &cmd.arguments {
            assert!(args.contains(&"gcc".to_string()));
            assert!(args.iter().any(|arg| arg.starts_with("-D")));
        }
    }
}
