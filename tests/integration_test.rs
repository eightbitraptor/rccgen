use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn get_rccgen_path() -> PathBuf {
    // First try to find debug binary in same directory as test binary
    let mut path = std::env::current_exe().expect("Failed to get current executable path");
    path.pop(); // Remove test binary name
    path.push("rccgen");

    if path.exists() {
        return path;
    }

    // Try debug build location
    path = std::env::current_dir()
        .unwrap()
        .join("target")
        .join("debug")
        .join("rccgen");

    if path.exists() {
        return path;
    }

    // Try release build location
    path = std::env::current_dir()
        .unwrap()
        .join("target")
        .join("release")
        .join("rccgen");

    if path.exists() {
        return path;
    }

    // If no binary found, panic with helpful message
    panic!("rccgen binary not found. Please run 'cargo build' or 'cargo build --release' first");
}

#[test]
fn test_end_to_end_simple_project() {
    let test_dir = std::env::temp_dir().join("rccgen_integration_test");
    fs::create_dir_all(&test_dir).unwrap();

    // Create a simple Makefile
    let makefile_content = r#"
CC = gcc
CFLAGS = -Wall -O2 -I./include
SOURCES = main.c utils.c
OBJECTS = $(SOURCES:.c=.o)
TARGET = program

all: $(TARGET)

$(TARGET): $(OBJECTS)
	$(CC) $(OBJECTS) -o $(TARGET)

%.o: %.c
	$(CC) $(CFLAGS) -c $< -o $@

clean:
	rm -f $(OBJECTS) $(TARGET)
"#;

    fs::write(test_dir.join("Makefile"), makefile_content).unwrap();

    // Create dummy source files
    fs::create_dir_all(test_dir.join("include")).unwrap();
    fs::write(test_dir.join("main.c"), "int main() { return 0; }").unwrap();
    fs::write(test_dir.join("utils.c"), "void util() {}").unwrap();
    fs::write(test_dir.join("include/utils.h"), "void util();").unwrap();

    // Get path to rccgen binary
    let rccgen_path = get_rccgen_path();

    let output = Command::new(&rccgen_path)
        .current_dir(&test_dir)
        .output()
        .expect("Failed to run rccgen");

    if !output.status.success() {
        eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));
        panic!("rccgen failed");
    }

    // Check that compile_commands.json was created
    let compile_commands_path = test_dir.join("compile_commands.json");
    assert!(compile_commands_path.exists(), "compile_commands.json not created");

    // Read and verify the content
    let content = fs::read_to_string(&compile_commands_path).unwrap();
    let commands: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();

    // Should have at least entries for main.c and utils.c
    assert!(commands.len() >= 2, "Expected at least 2 compilation commands");

    // Verify structure of commands
    for cmd in &commands {
        assert!(cmd.get("directory").is_some());
        // Must have either command or arguments (arguments preferred per spec)
        assert!(cmd.get("command").is_some() || cmd.get("arguments").is_some());
        assert!(cmd.get("file").is_some());

        let file = cmd["file"].as_str().unwrap();

        // Verify compiler presence in either command or arguments
        if let Some(command) = cmd.get("command").and_then(|c| c.as_str()) {
            assert!(command.contains("gcc") || command.contains("cc"));
        } else if let Some(arguments) = cmd.get("arguments").and_then(|a| a.as_array()) {
            let first_arg = arguments[0].as_str().unwrap_or("");
            assert!(first_arg.contains("gcc") || first_arg.contains("cc"));
        }

        // Verify file paths
        assert!(file.ends_with(".c") || file.ends_with(".h"));
    }

    // Check for expected files
    let files: Vec<String> = commands
        .iter()
        .filter_map(|cmd| cmd["file"].as_str())
        .map(|s| PathBuf::from(s).file_name().unwrap().to_string_lossy().into_owned())
        .collect();

    assert!(files.iter().any(|f| f == "main.c"), "main.c not found");
    assert!(files.iter().any(|f| f == "utils.c"), "utils.c not found");

    // Cleanup
    fs::remove_dir_all(&test_dir).unwrap_or(());
}

#[test]
fn test_no_makefile_error() {
    let test_dir = std::env::temp_dir().join("rccgen_no_makefile_test");
    fs::create_dir_all(&test_dir).unwrap();

    // Get path to rccgen binary
    let rccgen_path = get_rccgen_path();

    let output = Command::new(&rccgen_path)
        .current_dir(&test_dir)
        .output()
        .expect("Failed to run rccgen");

    // Should fail with appropriate error
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("No Makefile found"));

    // Cleanup
    fs::remove_dir_all(&test_dir).unwrap_or(());
}

#[test]
fn test_complex_flags() {
    let test_dir = std::env::temp_dir().join("rccgen_complex_flags_test");
    fs::create_dir_all(&test_dir).unwrap();

    // Create a Makefile with complex flags
    let makefile_content = r#"
CC = gcc
CFLAGS = -Wall -Wextra -O2 -g -std=c11 -D_GNU_SOURCE -DVERSION=\"1.0\" \
         -I./include -I/usr/local/include -pthread -fPIC
SOURCES = test.c
OBJECTS = $(SOURCES:.c=.o)

all: $(OBJECTS)

%.o: %.c
	$(CC) $(CFLAGS) -c $< -o $@
"#;

    fs::write(test_dir.join("Makefile"), makefile_content).unwrap();
    fs::write(test_dir.join("test.c"), "int test() { return 0; }").unwrap();

    // Get path to rccgen binary
    let rccgen_path = get_rccgen_path();

    let output = Command::new(&rccgen_path)
        .current_dir(&test_dir)
        .output()
        .expect("Failed to run rccgen");

    assert!(output.status.success());

    // Read and verify compile_commands.json
    let content = fs::read_to_string(test_dir.join("compile_commands.json")).unwrap();
    let commands: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();

    assert!(!commands.is_empty());

    // Check flags in either command or arguments
    let has_flag = |flag: &str| -> bool {
        if let Some(command) = commands[0].get("command").and_then(|c| c.as_str()) {
            command.contains(flag)
        } else if let Some(arguments) = commands[0].get("arguments").and_then(|a| a.as_array()) {
            arguments
                .iter()
                .any(|arg| arg.as_str().map_or(false, |s| s.contains(flag)))
        } else {
            false
        }
    };

    // Verify various flags are preserved
    assert!(has_flag("-Wall"));
    assert!(has_flag("-Wextra"));
    assert!(has_flag("-O2"));
    assert!(has_flag("-std=c11"));
    assert!(has_flag("-D_GNU_SOURCE"));
    assert!(has_flag("-DVERSION"));
    assert!(has_flag("-pthread"));
    assert!(has_flag("-fPIC"));

    // Cleanup
    fs::remove_dir_all(&test_dir).unwrap_or(());
}

#[test]
fn test_make_fallback_to_all_target() {
    let test_dir = std::env::temp_dir().join("rccgen_fallback_all_test");
    fs::create_dir_all(&test_dir).unwrap();

    let makefile_content = r#"
.PHONY: fail all build

CFLAGS = -Wall -O2

fail:
	$(error Intentional failure to test fallback)

all: build

build:
	$(CC) $(CFLAGS) -c main.c -o main.o
"#;

    fs::write(test_dir.join("Makefile"), makefile_content).unwrap();
    fs::write(test_dir.join("main.c"), "int main(void) { return 0; }\n").unwrap();

    // Get path to rccgen binary
    let rccgen_path = get_rccgen_path();

    let output = Command::new(&rccgen_path)
        .current_dir(&test_dir)
        .output()
        .expect("Failed to run rccgen");

    if !output.status.success() {
        eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));
        panic!("rccgen failed");
    }

    let compile_commands_path = test_dir.join("compile_commands.json");
    assert!(compile_commands_path.exists(), "compile_commands.json not created");

    let content = fs::read_to_string(&compile_commands_path).unwrap();
    let commands: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
    assert_eq!(commands.len(), 1);

    assert!(commands.iter().any(|cmd| {
        cmd.get("file")
            .and_then(|f| f.as_str())
            .map(|f| f.ends_with("main.c"))
            .unwrap_or(false)
    }));

    fs::remove_dir_all(&test_dir).unwrap_or(());
}
