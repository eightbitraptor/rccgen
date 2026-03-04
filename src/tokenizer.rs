/// Tokenizes shell command strings, handling quotes and escapes
pub fn tokenize(command: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = ' ';
    let mut escape_next = false;

    for c in command.chars() {
        if escape_next {
            current.push(c);
            escape_next = false;
            continue;
        }

        match c {
            '\\' => {
                if in_quotes {
                    current.push(c);
                } else {
                    escape_next = true;
                }
            }
            '"' | '\'' => {
                if !in_quotes {
                    in_quotes = true;
                    quote_char = c;
                } else if c == quote_char {
                    in_quotes = false;
                } else {
                    current.push(c);
                }
            }
            ' ' | '\t' if !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => {
                current.push(c);
            }
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_simple() {
        let tokens = tokenize("gcc -c -o file.o file.c");
        assert_eq!(tokens, vec!["gcc", "-c", "-o", "file.o", "file.c"]);
    }

    #[test]
    fn test_tokenize_with_double_quotes() {
        let tokens = tokenize("gcc -D\"MACRO=value with spaces\" file.c");
        assert_eq!(tokens, vec!["gcc", "-DMACRO=value with spaces", "file.c"]);
    }

    #[test]
    fn test_tokenize_with_single_quotes() {
        let tokens = tokenize("gcc '-I/path with spaces' file.c");
        assert_eq!(tokens, vec!["gcc", "-I/path with spaces", "file.c"]);
    }

    #[test]
    fn test_tokenize_escaped_spaces() {
        let tokens = tokenize("gcc -I/path\\ with\\ spaces file.c");
        assert_eq!(tokens, vec!["gcc", "-I/path with spaces", "file.c"]);
    }

    #[test]
    fn test_tokenize_mixed_quotes() {
        let tokens = tokenize("gcc -D'SINGLE=\"double\"' file.c");
        assert_eq!(tokens, vec!["gcc", "-DSINGLE=\"double\"", "file.c"]);
    }

    #[test]
    fn test_tokenize_empty_string() {
        let tokens = tokenize("");
        assert_eq!(tokens, Vec::<String>::new());
    }

    #[test]
    fn test_tokenize_only_spaces() {
        let tokens = tokenize("   \t  ");
        assert_eq!(tokens, Vec::<String>::new());
    }

    #[test]
    fn test_tokenize_tabs() {
        let tokens = tokenize("gcc\t-c\tfile.c");
        assert_eq!(tokens, vec!["gcc", "-c", "file.c"]);
    }
}
