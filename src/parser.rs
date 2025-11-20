use regex::Regex;
use std::io;
use std::path::{Path, PathBuf};

use crate::compiler::CompilerDetector;
use crate::json_writer::CompileCommand;
use crate::tokenizer;
use crate::validation;

/// Parses Make output to extract compilation commands
pub struct MakeOutputParser {
    working_dir: PathBuf,
    dir_enter_regex: Regex,
    dir_leave_regex: Regex,
}

impl MakeOutputParser {
    pub fn new(working_dir: PathBuf) -> io::Result<Self> {
        let dir_enter_regex = Regex::new(r"(?:Entering|Entering directory) [`']([^'`]+)[`']")
            .map_err(|e| io::Error::other(format!("Failed to compile enter regex: {}", e)))?;
        let dir_leave_regex = Regex::new(r"(?:Leaving|Leaving directory)")
            .map_err(|e| io::Error::other(format!("Failed to compile leave regex: {}", e)))?;

        Ok(Self {
            working_dir,
            dir_enter_regex,
            dir_leave_regex,
        })
    }

    pub fn parse(&mut self, output: &str, detector: &CompilerDetector) -> io::Result<Vec<CompileCommand>> {
        let mut commands = Vec::new();
        let mut dir_stack = vec![self.working_dir.clone()];

        for line in output.lines() {
            let line = line.trim();

            if !validation::validate_shell_command(line) {
                eprintln!("rccgen: Skipping invalid command line");
                continue;
            }

            if line.contains("Entering") {
                if let Some(captures) = self.dir_enter_regex.captures(line) {
                    if let Some(dir_match) = captures.get(1) {
                        let mut new_dir = PathBuf::from(dir_match.as_str());
                        if !new_dir.is_absolute() {
                            if let Some(base) = dir_stack.last() {
                                new_dir = base.join(new_dir);
                            } else {
                                new_dir = self.working_dir.join(new_dir);
                            }
                        }
                        dir_stack.push(new_dir);
                    }
                }
            } else if self.dir_leave_regex.is_match(line) && dir_stack.len() > 1 {
                dir_stack.pop();
            }

            let current_dir = dir_stack.last().cloned().unwrap_or_else(|| self.working_dir.clone());

            if detector.is_compilation_command(line) {
                let parsed = self.parse_compilation_command(line, &current_dir, detector);
                commands.extend(parsed);
            }
        }

        Ok(commands)
    }

    fn parse_compilation_command(
        &self,
        line: &str,
        working_dir: &Path,
        detector: &CompilerDetector,
    ) -> Vec<CompileCommand> {
        let tokens = tokenizer::tokenize(line);
        if tokens.is_empty() {
            return Vec::new();
        }

        let mut source_files = Vec::new();
        let mut i = 1;

        while i < tokens.len() {
            let token = &tokens[i];

            if self.is_flag_with_argument(token) && i + 1 < tokens.len() {
                i += 2;
                continue;
            }

            if detector.is_source_file(token) && !token.starts_with("-") {
                source_files.push(token.clone());
            }

            i += 1;
        }

        let mut results = Vec::new();
        for file in source_files {
            let file_path = self.resolve_path(&file, working_dir);
            let normalized_tokens = self.normalize_tokens_for_file(&tokens, working_dir, detector, &file);

            results.push(CompileCommand::with_arguments(
                working_dir.to_string_lossy().into_owned(),
                normalized_tokens,
                file_path,
            ));
        }

        results
    }

    fn normalize_tokens_for_file(
        &self,
        tokens: &[String],
        working_dir: &Path,
        detector: &CompilerDetector,
        target_source: &str,
    ) -> Vec<String> {
        let mut normalized = Vec::new();

        let mut i = 0;
        while i < tokens.len() {
            let token = &tokens[i];

            if i == 0 {
                normalized.push(token.clone());
                i += 1;
                continue;
            }

            if token == "-I" && i + 1 < tokens.len() {
                normalized.push("-I".to_string());
                let include_path = &tokens[i + 1];
                let resolved = self.resolve_path(include_path, working_dir);
                normalized.push(resolved);
                i += 2;
            } else if let Some(include_path) = token.strip_prefix("-I") {
                let resolved = self.resolve_path(include_path, working_dir);
                normalized.push(format!("-I{}", resolved));
                i += 1;
            } else if detector.is_source_file(token) && !token.starts_with('-') {
                if token == target_source {
                    let resolved = self.resolve_path(token, working_dir);
                    normalized.push(resolved);
                }
                i += 1;
            } else {
                normalized.push(token.clone());
                i += 1;
            }
        }

        normalized
    }

    fn resolve_path(&self, path: &str, working_dir: &Path) -> String {
        let sanitized = validation::sanitize_path(path);
        if !validation::validate_path(&sanitized) {
            eprintln!("rccgen: Warning: Invalid path in compilation command: {}", path);
            return working_dir.to_string_lossy().into_owned();
        }

        if Path::new(&sanitized).is_absolute() {
            sanitized
        } else {
            working_dir
                .join(&sanitized)
                .canonicalize()
                .unwrap_or_else(|_| working_dir.join(sanitized))
                .to_string_lossy()
                .into_owned()
        }
    }

    fn is_flag_with_argument(&self, flag: &str) -> bool {
        matches!(
            flag,
            "-o" | "-I"
                | "-D"
                | "-U"
                | "-include"
                | "-imacros"
                | "-isystem"
                | "-idirafter"
                | "-iprefix"
                | "-iwithprefix"
                | "-iwithprefixbefore"
                | "-isysroot"
                | "-MF"
                | "-MT"
                | "-MQ"
                | "-Xlinker"
                | "-Wl"
                | "-framework"
                | "-arch"
                | "-target"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_parser() -> MakeOutputParser {
        MakeOutputParser::new(PathBuf::from("/project")).unwrap()
    }

    fn create_test_detector() -> CompilerDetector {
        CompilerDetector::new().unwrap()
    }

    #[test]
    fn test_parse_compilation_command() {
        let parser = create_test_parser();
        let detector = create_test_detector();
        let working_dir = PathBuf::from("/project");

        let cmds =
            parser.parse_compilation_command("gcc -c -Wall -I../include -o file.o file.c", &working_dir, &detector);

        assert_eq!(cmds.len(), 1);
        let cmd = &cmds[0];
        assert_eq!(cmd.directory, "/project");
        assert!(cmd.file.ends_with("file.c"));
        assert!(cmd.arguments.is_some());
        if let Some(args) = &cmd.arguments {
            assert!(args.contains(&"-Wall".to_string()));
        }
    }

    #[test]
    fn test_parse_compilation_command_multiple_sources() {
        let parser = create_test_parser();
        let detector = create_test_detector();
        let working_dir = PathBuf::from("/project");

        let cmds = parser.parse_compilation_command("gcc -c file1.c file2.c -o output.o", &working_dir, &detector);

        assert_eq!(cmds.len(), 2);
        assert!(cmds[0].file.ends_with("file1.c"));
        assert!(cmds[1].file.ends_with("file2.c"));
    }

    #[test]
    fn test_directory_tracking() {
        let mut parser = create_test_parser();
        let detector = create_test_detector();

        let make_output = r#"
make[1]: Entering directory `/project/src'
gcc -c file.c
make[1]: Leaving directory `/project/src'
        "#;

        let commands = parser.parse(make_output, &detector).unwrap();
        assert_eq!(commands.len(), 1);
        assert!(commands[0].directory.contains("/project/src"));
    }

    #[test]
    fn test_skip_non_compilation_commands() {
        let mut parser = create_test_parser();
        let detector = create_test_detector();

        let make_output = r#"
echo "Building project"
rm -f *.o
mkdir -p build
gcc -c file.c -o file.o
ar rcs libproject.a file.o
        "#;

        let commands = parser.parse(make_output, &detector).unwrap();
        assert_eq!(commands.len(), 1);
        assert!(commands[0].arguments.is_some());
        if let Some(ref args) = commands[0].arguments {
            assert!(args[0].contains("gcc"));
        }
    }

    #[test]
    fn test_resolve_path_absolute() {
        let parser = create_test_parser();
        let working_dir = PathBuf::from("/project");

        let resolved = parser.resolve_path("/absolute/path/file.c", &working_dir);
        assert_eq!(resolved, "/absolute/path/file.c");
    }

    #[test]
    fn test_resolve_path_relative() {
        let parser = create_test_parser();
        let working_dir = PathBuf::from("/project");

        let resolved = parser.resolve_path("src/file.c", &working_dir);
        assert!(resolved.contains("/project/src/file.c"));
    }
}
