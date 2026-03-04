use regex::Regex;
use std::io;

/// Detects and analyzes compiler commands in build output.
///
/// This struct provides functionality to identify compiler invocations
/// (gcc, g++, clang, etc.) and determine if a command line represents
/// a compilation command.
pub struct CompilerDetector {
    compiler_regex: Regex,
}

const COMPILER_NAMES: &[&str] = &["gcc", "g++", "clang", "clang++", "cc", "c++"];

impl CompilerDetector {
    pub fn new() -> io::Result<Self> {
        let compiler_pattern = r"(?:^|\s|/)(?:gcc|g\+\+|clang|clang\+\+|cc|c\+\+)(?:\s|$)";
        let compiler_regex = Regex::new(compiler_pattern).map_err(io::Error::other)?;

        Ok(Self { compiler_regex })
    }

    pub fn is_compilation_command(&self, line: &str) -> bool {
        if !self.compiler_regex.is_match(line) {
            return false;
        }

        let tokens = crate::tokenizer::tokenize(line);
        self.is_compilation_tokens(&tokens)
    }

    pub fn is_compilation_tokens(&self, tokens: &[String]) -> bool {
        if tokens.is_empty() {
            return false;
        }

        let first_token = &tokens[0];
        let compiler_name = first_token
            .rsplit(|c| c == '/' || c == '\\')
            .next()
            .unwrap_or(first_token);
        if !COMPILER_NAMES.contains(&compiler_name) {
            return false;
        }

        let has_compile_flag = tokens.iter().any(|t| t == "-c");
        let has_source = tokens.iter().any(|t| self.is_source_file(t));
        has_compile_flag || has_source
    }

    pub fn is_source_file(&self, path: &str) -> bool {
        path.ends_with(".c")
            || path.ends_with(".cc")
            || path.ends_with(".cpp")
            || path.ends_with(".cxx")
            || path.ends_with(".C")
            || path.ends_with(".c++")
            || path.ends_with(".m")
            || path.ends_with(".mm")
            || path.ends_with(".S")
            || path.ends_with(".s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_compilation_command_gcc() {
        let detector = CompilerDetector::new().unwrap();
        assert!(detector.is_compilation_command("gcc -c file.c"));
        assert!(detector.is_compilation_command("/usr/bin/gcc -c file.c"));
        assert!(detector.is_compilation_command("gcc -Wall -O2 file.c -o file.o"));
    }

    #[test]
    fn test_is_compilation_command_clang() {
        let detector = CompilerDetector::new().unwrap();
        assert!(detector.is_compilation_command("clang -c file.c"));
        assert!(detector.is_compilation_command("clang++ -std=c++11 file.cpp"));
    }

    #[test]
    fn test_is_compilation_command_negative() {
        let detector = CompilerDetector::new().unwrap();
        assert!(!detector.is_compilation_command("rm -f file.o"));
        assert!(!detector.is_compilation_command("echo Building..."));
        assert!(!detector.is_compilation_command("mkdir -p build"));
        assert!(!detector.is_compilation_command("gcc"));
        assert!(!detector.is_compilation_command("ar rcs lib.a file.o"));
    }

    #[test]
    fn test_is_source_file() {
        let detector = CompilerDetector::new().unwrap();
        assert!(detector.is_source_file("file.c"));
        assert!(detector.is_source_file("file.cc"));
        assert!(detector.is_source_file("file.cpp"));
        assert!(detector.is_source_file("file.cxx"));
        assert!(detector.is_source_file("file.c++"));
        assert!(detector.is_source_file("file.C"));
        assert!(detector.is_source_file("file.m"));
        assert!(detector.is_source_file("file.mm"));
        assert!(detector.is_source_file("file.S"));
        assert!(detector.is_source_file("file.s"));

        assert!(!detector.is_source_file("file.h"));
        assert!(!detector.is_source_file("file.o"));
        assert!(!detector.is_source_file("file.txt"));
    }
}
