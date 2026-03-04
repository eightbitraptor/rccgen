pub mod compiler;
pub mod json_writer;
pub mod parser;
pub mod tokenizer;
pub mod validation;

use std::collections::HashSet;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use walkdir::WalkDir;

use rayon::prelude::*;

use crate::compiler::CompilerDetector;
use crate::json_writer::CompileCommand;
use crate::parser::MakeOutputParser;

/// The main struct for generating compile_commands.json files.
///
/// RccGen analyzes Make-based build systems by running `make -n` (dry run)
/// and parsing the output to extract compilation commands. It then generates
/// a compile_commands.json file that can be used by language servers and
/// other development tools.
pub struct RccGen {
    working_dir: PathBuf,
    compile_commands: Vec<CompileCommand>,
    processed_files: HashSet<String>,
    compiler_detector: CompilerDetector,
}

impl RccGen {
    /// Creates a new RccGen instance.
    ///
    /// Initializes the generator with the current working directory and
    /// sets up the compiler detection system.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The current working directory cannot be determined
    /// - The compiler detector cannot be initialized
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rccgen::RccGen;
    ///
    /// let mut generator = RccGen::new().expect("Failed to initialize");
    /// ```
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            working_dir: std::env::current_dir()?,
            compile_commands: Vec::new(),
            processed_files: HashSet::new(),
            compiler_detector: CompilerDetector::new()?,
        })
    }

    /// Runs the complete compilation database generation process.
    ///
    /// This method performs the following steps:
    /// 1. Checks for a Makefile in the current directory
    /// 2. Runs `make -n -B` to get a dry run of all compilation commands
    /// 3. Parses the Make output to extract compilation commands
    /// 4. Discovers and adds header files to the compilation database
    /// 5. Writes the compile_commands.json file
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No Makefile is found in the current directory
    /// - The make command fails or produces no output
    /// - File I/O operations fail
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rccgen::RccGen;
    ///
    /// let mut generator = RccGen::new().expect("Failed to initialize");
    /// generator.run().expect("Failed to generate compile_commands.json");
    /// ```
    pub fn run(&mut self) -> io::Result<()> {
        eprintln!("rccgen: Analyzing build system...");

        if !self.has_makefile() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "No Makefile found in current directory. Run your project's configure step first.",
            ));
        }

        self.run_make_and_parse()?;
        self.discover_headers()?;
        self.write_compile_commands()?;

        eprintln!(
            "rccgen: Generated compile_commands.json with {} entries",
            self.compile_commands.len()
        );

        Ok(())
    }

    fn has_makefile(&self) -> bool {
        self.working_dir.join("Makefile").exists()
            || self.working_dir.join("makefile").exists()
            || self.working_dir.join("GNUmakefile").exists()
    }

    fn run_make_and_parse(&mut self) -> io::Result<()> {
        eprintln!("rccgen: Running make in dry-run mode...");
        eprintln!("rccgen: Parsing compilation commands...");

        let base_args = ["-n", "-B", "-w", "--print-directory", "-j1"];
        let (status, commands, stderr) = self.run_make_dry_attempt(&base_args)?;
        if status.success() {
            self.add_compile_commands(commands);
            return Ok(());
        }

        if !commands.is_empty() {
            eprintln!(
                "rccgen: Warning: make dry-run {} but produced commands; using partial output",
                self.fmt_status(status)
            );
            self.add_compile_commands(commands);
            return Ok(());
        }

        eprintln!("rccgen: make dry-run failed, retrying with explicit 'all' target...");

        let mut fallback_args = base_args.to_vec();
        fallback_args.push("all");

        let (fallback_status, fallback_commands, fallback_stderr) = self.run_make_dry_attempt(&fallback_args)?;

        if fallback_status.success() {
            self.add_compile_commands(fallback_commands);
            return Ok(());
        }

        if !fallback_commands.is_empty() {
            eprintln!(
                "rccgen: Warning: make dry-run with 'all' {} but produced commands; using partial output",
                self.fmt_status(fallback_status)
            );
            self.add_compile_commands(fallback_commands);
            return Ok(());
        }

        if !stderr.trim().is_empty() {
            eprintln!("rccgen: Initial make stderr:\n{}", stderr.trim());
        }

        Err(self.make_failed(&fallback_args, fallback_status, fallback_stderr.as_bytes()))
    }

    fn run_make_dry_attempt(&self, args: &[&str]) -> io::Result<(ExitStatus, Vec<CompileCommand>, String)> {
        let mut child = Command::new("make")
            .args(args)
            .current_dir(&self.working_dir)
            .env("MAKEFLAGS", "")
            .env("MFLAGS", "")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("failed to capture make stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("failed to capture make stderr"))?;

        let stderr_thread = thread::spawn(move || -> io::Result<String> {
            let mut reader = BufReader::new(stderr);
            let mut stderr_buf = String::new();
            reader.read_to_string(&mut stderr_buf)?;
            Ok(stderr_buf)
        });

        let mut parser = MakeOutputParser::new(self.working_dir.clone())?;
        let parse_result = parser.parse_reader(BufReader::new(stdout), &self.compiler_detector);
        let wait_result = child.wait();

        let stderr_result = stderr_thread
            .join()
            .map_err(|_| io::Error::other("stderr reader thread panicked"))?;

        let commands = parse_result?;
        let status = wait_result?;
        let stderr_output = stderr_result?;

        Ok((status, commands, stderr_output))
    }

    #[cfg(test)]
    fn parse_make_output(&mut self, output: &str) -> io::Result<()> {
        eprintln!("rccgen: Parsing compilation commands...");

        let mut parser = MakeOutputParser::new(self.working_dir.clone())?;
        let commands = parser.parse(output, &self.compiler_detector)?;
        self.add_compile_commands(commands);

        Ok(())
    }

    fn add_compile_commands(&mut self, commands: Vec<CompileCommand>) {
        for cmd in commands {
            let file_path = cmd.file.clone();
            if self.processed_files.insert(file_path) {
                self.compile_commands.push(cmd);
            }
        }
    }

    fn discover_headers(&mut self) -> io::Result<()> {
        eprintln!("rccgen: Discovering header files...");

        let mut include_paths = HashSet::new();
        let mut base_flags = None;

        for cmd in &self.compile_commands {
            let tokens = if let Some(ref args) = cmd.arguments {
                args.clone()
            } else if let Some(ref command) = cmd.command {
                tokenizer::tokenize(command)
            } else {
                continue;
            };

            if !tokens.is_empty() && base_flags.is_none() {
                let mut flags = vec![tokens[0].clone()];
                let mut i = 1;
                while i < tokens.len() {
                    let token = &tokens[i];
                    if token == "-I" && i + 1 < tokens.len() {
                        let include_dir = &tokens[i + 1];
                        include_paths.insert(self.resolve_path(include_dir, Path::new(&cmd.directory)));
                        flags.push("-I".to_string());
                        flags.push(tokens[i + 1].clone());
                        i += 2;
                    } else if let Some(include_dir) = token.strip_prefix("-I") {
                        include_paths.insert(self.resolve_path(include_dir, Path::new(&cmd.directory)));
                        flags.push(token.clone());
                        i += 1;
                    } else if self.is_compiler_flag(token) {
                        flags.push(token.clone());
                        if self.flag_has_argument(token) && i + 1 < tokens.len() {
                            flags.push(tokens[i + 1].clone());
                            i += 2;
                        } else {
                            i += 1;
                        }
                    } else {
                        i += 1;
                    }
                }
                base_flags = Some(flags);
            }

            if let Some(parent) = Path::new(&cmd.file).parent() {
                include_paths.insert(parent.to_path_buf());
            }
        }

        include_paths.insert(self.working_dir.join("include"));
        include_paths.insert(self.working_dir.join("src"));
        include_paths.insert(self.working_dir.clone());

        let include_dirs: Vec<PathBuf> = include_paths
            .into_iter()
            .filter(|dir| dir.exists() && dir.is_dir())
            .collect();

        let header_results: Vec<Vec<PathBuf>> = include_dirs
            .par_iter()
            .map(|dir| Self::collect_headers_in_dir(dir))
            .collect();

        let mut headers = Vec::new();
        for mut found in header_results {
            headers.append(&mut found);
        }

        if let Some(flags) = base_flags {
            for header_path in headers {
                let header_str = header_path.to_string_lossy().into_owned();
                if self.processed_files.insert(header_str.clone()) {
                    let mut cmd_flags = flags.clone();
                    cmd_flags.push("-c".to_string());
                    cmd_flags.push("-x".to_string());

                    let lang =
                        if header_str.ends_with(".hpp") || header_str.ends_with(".hxx") || header_str.ends_with(".h++")
                        {
                            "c++-header"
                        } else {
                            "c-header"
                        };
                    cmd_flags.push(lang.to_string());
                    cmd_flags.push(header_str.clone());

                    self.compile_commands.push(CompileCommand::with_arguments(
                        self.working_dir.to_string_lossy().into_owned(),
                        cmd_flags,
                        header_str,
                    ));
                }
            }
        }

        Ok(())
    }

    fn collect_headers_in_dir(dir: &Path) -> Vec<PathBuf> {
        let mut headers = Vec::new();
        for entry in WalkDir::new(dir)
            .max_depth(5)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| {
                if !entry.file_type().is_dir() {
                    return true;
                }

                let name = entry.file_name().to_str().unwrap_or("");
                !((name.starts_with('.') && name != ".ext") || name == "build" || name == "CMakeFiles")
            })
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if matches!(ext.to_str(), Some("h" | "hpp" | "hxx" | "hh" | "H")) {
                        headers.push(path.to_path_buf());
                    }
                }
            }
        }

        headers
    }

    fn write_compile_commands(&self) -> io::Result<()> {
        let output_path = self.working_dir.join("compile_commands.json");
        json_writer::write_compile_commands(&output_path, &self.compile_commands)?;

        let file_name = output_path
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_else(|| "compile_commands.json".into());
        let parent_dir = output_path
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| self.working_dir.display().to_string());

        eprintln!("rccgen: Wrote {} to {}", file_name, parent_dir);

        Ok(())
    }

    fn resolve_path(&self, path: &str, working_dir: &Path) -> PathBuf {
        let sanitized = validation::sanitize_path(path);
        if !validation::validate_path(&sanitized) {
            eprintln!("rccgen: Warning: Invalid path detected: {}", path);
            return working_dir.to_path_buf();
        }

        if Path::new(&sanitized).is_absolute() {
            validation::normalize_path(Path::new(&sanitized))
        } else {
            validation::normalize_path(&working_dir.join(sanitized))
        }
    }

    fn is_compiler_flag(&self, token: &str) -> bool {
        token.starts_with("-D")
            || token.starts_with("-U")
            || token.starts_with("-std=")
            || token.starts_with("-m")
            || token.starts_with("-f")
            || token.starts_with("-W")
            || token == "-pthread"
            || token == "-fPIC"
            || token == "-fpic"
    }

    fn flag_has_argument(&self, flag: &str) -> bool {
        matches!(flag, "-D" | "-U")
    }

    #[cfg(test)]
    fn contains_compile_commands(&self, output: &str) -> bool {
        output
            .lines()
            .any(|line| self.compiler_detector.is_compilation_command(line.trim()))
    }

    fn make_failed(&self, args: &[&str], status: std::process::ExitStatus, stderr: &[u8]) -> io::Error {
        let joined_args = args.join(" ");
        let status_msg = match status.code() {
            Some(code) => format!("exit status {}", code),
            None => "terminated by signal".to_string(),
        };
        let stderr_str = String::from_utf8_lossy(stderr).trim().to_owned();
        let detail = if stderr_str.is_empty() {
            "no stderr output captured".to_string()
        } else {
            stderr_str
        };

        io::Error::other(format!("make {} {}: {}", joined_args, status_msg, detail))
    }

    fn fmt_status(&self, status: std::process::ExitStatus) -> String {
        match status.code() {
            Some(code) => format!("exited with status {}", code),
            None => "was terminated by signal".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("{}_{}", prefix, unique));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_has_makefile() {
        let temp_dir = std::env::temp_dir().join("rccgen_lib_test");
        fs::create_dir_all(&temp_dir).unwrap();

        let mut rccgen = RccGen::new().unwrap();
        rccgen.working_dir = temp_dir.clone();
        assert!(!rccgen.has_makefile());

        fs::write(temp_dir.join("Makefile"), "all:\n\techo test").unwrap();
        assert!(rccgen.has_makefile());

        fs::remove_dir_all(&temp_dir).unwrap_or(());
    }

    #[test]
    fn test_resolve_path() {
        let rccgen = RccGen::new().unwrap();
        let working_dir = PathBuf::from("/project");

        let resolved = rccgen.resolve_path("/absolute/path", &working_dir);
        assert_eq!(resolved, PathBuf::from("/absolute/path"));

        let resolved = rccgen.resolve_path("relative/path", &working_dir);
        assert!(resolved.to_string_lossy().contains("project"));
        assert!(resolved.to_string_lossy().contains("relative"));
    }

    #[test]
    fn test_resolve_path_invalid_falls_back() {
        let rccgen = RccGen::new().unwrap();
        let working_dir = PathBuf::from("/tmp/project");
        let long_path = "a".repeat(6000);

        let resolved = rccgen.resolve_path(&long_path, &working_dir);
        assert_eq!(resolved, working_dir);
    }

    #[test]
    fn test_parse_make_output_deduplicates() {
        let dir = temp_dir("parse_make_output");
        let mut rccgen = RccGen::new().unwrap();
        rccgen.working_dir = dir.clone();

        let make_output = "gcc -c main.c -o main.o";
        rccgen.parse_make_output(make_output).unwrap();
        assert_eq!(rccgen.compile_commands.len(), 1);

        rccgen.parse_make_output(make_output).unwrap();
        assert_eq!(rccgen.compile_commands.len(), 1);

        fs::remove_dir_all(dir).unwrap_or(());
    }

    #[test]
    fn test_discover_headers_adds_commands() {
        let dir = temp_dir("discover_headers");
        let include_dir = dir.join("include");
        let src_dir = dir.join("src");
        fs::create_dir_all(&include_dir).unwrap();
        fs::create_dir_all(&src_dir).unwrap();

        let header_path = include_dir.join("extra.h");
        fs::write(&header_path, "// header").unwrap();
        let source_path = src_dir.join("main.c");
        fs::write(&source_path, "int main() { return 0; }").unwrap();

        let relative_source = source_path.strip_prefix(&dir).unwrap().to_string_lossy().into_owned();

        let mut rccgen = RccGen::new().unwrap();
        rccgen.working_dir = dir.clone();
        rccgen.compile_commands.push(CompileCommand::with_arguments(
            dir.to_string_lossy().into_owned(),
            vec!["gcc".into(), "-I".into(), "include".into(), relative_source.clone()],
            source_path.to_string_lossy().into_owned(),
        ));
        rccgen
            .processed_files
            .insert(source_path.to_string_lossy().into_owned());

        rccgen.discover_headers().unwrap();

        assert!(
            rccgen.compile_commands.iter().any(|cmd| cmd.file.ends_with("extra.h")),
            "expected header compilation command to be added"
        );

        fs::remove_dir_all(dir).unwrap_or(());
    }

    #[test]
    fn test_contains_compile_commands_detection() {
        let rccgen = RccGen::new().unwrap();
        assert!(rccgen.contains_compile_commands("echo\nclang -c foo.c\n"));
        assert!(!rccgen.contains_compile_commands("echo only"));
    }

    #[test]
    fn test_compiler_flag_helpers() {
        let rccgen = RccGen::new().unwrap();
        assert!(rccgen.is_compiler_flag("-DDEBUG"));
        assert!(rccgen.is_compiler_flag("-Wextra"));
        assert!(!rccgen.is_compiler_flag("-o"));
        assert!(rccgen.flag_has_argument("-D"));
        assert!(!rccgen.flag_has_argument("-Wextra"));
    }

    #[cfg(unix)]
    fn exited_status(code: i32) -> std::process::ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw((code & 0xff) << 8)
    }

    #[cfg(unix)]
    fn signaled_status(signal: i32) -> std::process::ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(signal & 0x7f)
    }

    #[cfg(unix)]
    #[test]
    fn test_make_failed_formats_error() {
        let rccgen = RccGen::new().unwrap();
        let status = exited_status(2);
        let err = rccgen.make_failed(&["-n"], status, b"boom");
        let message = err.to_string();
        assert!(message.contains("-n"));
        assert!(message.contains("exit status 2"));
        assert!(message.contains("boom"));
        assert_eq!(rccgen.fmt_status(status), "exited with status 2");
    }

    #[cfg(unix)]
    #[test]
    fn test_fmt_status_for_signal() {
        let rccgen = RccGen::new().unwrap();
        let status = signaled_status(9);
        assert_eq!(rccgen.fmt_status(status), "was terminated by signal");
    }
}
