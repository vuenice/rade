use std::fmt;
use crate::lexer::token::Token;

/// Errors that can occur during template lexical analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexerError {
    /// A comment was started with `{{--` but never closed with `--}}`.
    UnclosedComment { line: usize, column: usize },

    /// An echo expression was started with `{{` or `@{{` but never closed with `}}`.
    UnclosedEcho { line: usize, column: usize },

    /// A raw echo expression was started with `{!!` but never closed with `!!}`.
    UnclosedRawEcho { line: usize, column: usize },

    /// A `@rhp` block was started but never closed with `@endrhp`.
    UnclosedRawRhp { line: usize, column: usize },

    /// A directive argument list was started with `(` but never closed with `)`.
    UnclosedDirectiveArgs {
        directive: String,
        line: usize,
        column: usize,
    },
}

impl fmt::Display for LexerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LexerError::UnclosedComment { line, column } => {
                write!(
                    f,
                    "Unclosed comment starting at line {}, column {}",
                    line, column
                )
            }
            LexerError::UnclosedEcho { line, column } => {
                write!(
                    f,
                    "Unclosed echo expression starting at line {}, column {}",
                    line, column
                )
            }
            LexerError::UnclosedRawEcho { line, column } => {
                write!(
                    f,
                    "Unclosed raw echo expression starting at line {}, column {}",
                    line, column
                )
            }
            LexerError::UnclosedRawRhp { line, column } => {
                write!(
                    f,
                    "Unclosed @rhp block starting at line {}, column {}",
                    line, column
                )
            }
            LexerError::UnclosedDirectiveArgs {
                directive,
                line,
                column,
            } => {
                write!(
                    f,
                    "Unclosed arguments for directive '@{}' starting at line {}, column {}",
                    directive, line, column
                )
            }
        }
    }
}

impl std::error::Error for LexerError {}

/// Token scanner / lexer for Rade templates.
pub struct Scanner<'a> {
    input: &'a str,
    cursor: usize,
    line: usize,
    column: usize,
    last_char: Option<char>,
}

impl<'a> Scanner<'a> {
    /// Creates a new `Scanner` for the given template string.
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            cursor: 0,
            line: 1,
            column: 1,
            last_char: None,
        }
    }

    /// Convenience helper to tokenize the given input string in one call.
    pub fn scan(input: &'a str) -> Result<Vec<Token>, LexerError> {
        Scanner::new(input).tokenize()
    }

    /// Scans the entire template input and returns a vector of `Token`s.
    pub fn tokenize(&mut self) -> Result<Vec<Token>, LexerError> {
        let mut tokens = Vec::new();
        let mut current_text = String::new();

        while !self.is_eof() {
            let rem = self.remaining();

            // 1. Blade comment: {{-- ... --}}
            if rem.starts_with("{{--") {
                self.flush_text(&mut tokens, &mut current_text);
                let start_line = self.line;
                let start_col = self.column;
                self.advance_str("{{--");

                let content_start = self.cursor;
                let mut found = false;

                while !self.is_eof() {
                    if self.remaining().starts_with("--}}") {
                        let content_end = self.cursor;
                        let raw_content = &self.input[content_start..content_end];
                        self.advance_str("--}}");
                        tokens.push(Token::Comment(raw_content.trim().to_string()));
                        found = true;
                        break;
                    }
                    self.advance();
                }

                if !found {
                    return Err(LexerError::UnclosedComment {
                        line: start_line,
                        column: start_col,
                    });
                }
                continue;
            }

            // 2. Raw echo: {!! ... !!}
            if rem.starts_with("{!!") {
                self.flush_text(&mut tokens, &mut current_text);
                let start_line = self.line;
                let start_col = self.column;
                self.advance_str("{!!");

                let content_start = self.cursor;
                let mut found = false;

                while !self.is_eof() {
                    let ch = self.peek().unwrap();
                    if ch == '"' || ch == '\'' {
                        self.scan_string_literal(ch);
                        continue;
                    }

                    if self.remaining().starts_with("!!}") {
                        let content_end = self.cursor;
                        let raw_content = &self.input[content_start..content_end];
                        self.advance_str("!!}");
                        tokens.push(Token::EchoRaw(raw_content.trim().to_string()));
                        found = true;
                        break;
                    }
                    self.advance();
                }

                if !found {
                    return Err(LexerError::UnclosedRawEcho {
                        line: start_line,
                        column: start_col,
                    });
                }
                continue;
            }

            // 3. Escaped verbatim echo: @{{ ... }}
            if rem.starts_with("@{{") {
                self.flush_text(&mut tokens, &mut current_text);
                let start_line = self.line;
                let start_col = self.column;

                // Skip the '@' prefix
                self.advance(); // '@'
                let content_start = self.cursor; // points to "{{"
                self.advance_str("{{");

                let mut found = false;
                while !self.is_eof() {
                    let ch = self.peek().unwrap();
                    if ch == '"' || ch == '\'' {
                        self.scan_string_literal(ch);
                        continue;
                    }

                    if self.remaining().starts_with("}}") {
                        self.advance_str("}}");
                        let content_end = self.cursor; // points after "}}"
                        let verbatim = &self.input[content_start..content_end];
                        tokens.push(Token::EscapedVerbatim(verbatim.to_string()));
                        found = true;
                        break;
                    }
                    self.advance();
                }

                if !found {
                    return Err(LexerError::UnclosedEcho {
                        line: start_line,
                        column: start_col,
                    });
                }
                continue;
            }

            // 4. Escaped echo: {{ ... }}
            if rem.starts_with("{{") {
                self.flush_text(&mut tokens, &mut current_text);
                let start_line = self.line;
                let start_col = self.column;
                self.advance_str("{{");

                let content_start = self.cursor;
                let mut found = false;

                while !self.is_eof() {
                    let ch = self.peek().unwrap();
                    if ch == '"' || ch == '\'' {
                        self.scan_string_literal(ch);
                        continue;
                    }

                    if self.remaining().starts_with("}}") {
                        let content_end = self.cursor;
                        let raw_content = &self.input[content_start..content_end];
                        self.advance_str("}}");
                        tokens.push(Token::EchoEscaped(raw_content.trim().to_string()));
                        found = true;
                        break;
                    }
                    self.advance();
                }

                if !found {
                    return Err(LexerError::UnclosedEcho {
                        line: start_line,
                        column: start_col,
                    });
                }
                continue;
            }

            // 5. Escaped @ (e.g. @@directive or @@click): emits '@' as plain text
            if rem.starts_with("@@") {
                current_text.push('@');
                self.advance(); // first '@'
                self.advance(); // second '@'
                continue;
            }

            // 6. Directives and RawRhp blocks starting with '@'
            if rem.starts_with('@') {
                // If preceded by a word character (like test@example.com), it's not a directive.
                let preceded_by_word_char = self
                    .last_char
                    .map_or(false, |c| c.is_ascii_alphanumeric() || c == '_');

                if preceded_by_word_char {
                    current_text.push('@');
                    self.advance();
                    continue;
                }

                // Check if followed by a valid identifier start char
                let mut chars_after_at = rem[1..].chars();
                let first_after_at = chars_after_at.next();
                let is_ident_start =
                    first_after_at.map_or(false, |c| c.is_ascii_alphabetic() || c == '_');

                if !is_ident_start {
                    // E.g. "@ 123" or "@" at EOF or "@123"
                    current_text.push('@');
                    self.advance();
                    continue;
                }

                // Check for @rhp ... @endrhp block
                if rem.starts_with("@rhp") {
                    let char_after_rhp = rem["@rhp".len()..].chars().next();
                    let is_ident_continue =
                        char_after_rhp.map_or(false, |c| c.is_ascii_alphanumeric() || c == '_');

                    if !is_ident_continue {
                        self.flush_text(&mut tokens, &mut current_text);
                        let start_line = self.line;
                        let start_col = self.column;
                        self.advance_str("@rhp");

                        let content_start = self.cursor;
                        let mut found = false;

                        while !self.is_eof() {
                            if self.remaining().starts_with("@endrhp") {
                                let content_end = self.cursor;
                                let raw_content = &self.input[content_start..content_end];
                                self.advance_str("@endrhp");
                                tokens.push(Token::RawRhp(raw_content.trim().to_string()));
                                found = true;
                                break;
                            }
                            self.advance();
                        }

                        if !found {
                            return Err(LexerError::UnclosedRawRhp {
                                line: start_line,
                                column: start_col,
                            });
                        }
                        continue;
                    }
                }

                // Regular directive: @name or @name(...)
                self.flush_text(&mut tokens, &mut current_text);
                let start_line = self.line;
                let start_col = self.column;

                // Advance past '@'
                self.advance();

                // Read directive name
                let name_start = self.cursor;
                while let Some(ch) = self.peek() {
                    if ch.is_ascii_alphanumeric() || ch == '_' {
                        self.advance();
                    } else {
                        break;
                    }
                }
                let name = self.input[name_start..self.cursor].to_string();

                // Check for arguments in parentheses: e.g. @if(...) or @if (...)
                // Only allow horizontal whitespace (spaces/tabs) on the same line before '('
                let mut peek_chars = self.remaining().chars();
                let mut has_paren = false;
                let mut ws_len = 0;

                for ch in peek_chars.by_ref() {
                    if ch == ' ' || ch == '\t' {
                        ws_len += ch.len_utf8();
                    } else if ch == '(' {
                        has_paren = true;
                        break;
                    } else {
                        break;
                    }
                }

                let args = if has_paren {
                    // Advance past horizontal whitespace
                    for _ in 0..ws_len {
                        self.advance();
                    }
                    // Advance past opening '('
                    self.advance();

                    let arg_start = self.cursor;
                    let mut paren_depth = 1;
                    let mut arg_end = self.cursor;

                    while !self.is_eof() {
                        let ch = self.peek().unwrap();
                        if ch == '"' || ch == '\'' {
                            self.scan_string_literal(ch);
                            continue;
                        }

                        if ch == '(' {
                            paren_depth += 1;
                            self.advance();
                        } else if ch == ')' {
                            paren_depth -= 1;
                            if paren_depth == 0 {
                                arg_end = self.cursor;
                                self.advance(); // advance past closing ')'
                                break;
                            } else {
                                self.advance();
                            }
                        } else {
                            self.advance();
                        }
                    }

                    if paren_depth != 0 {
                        return Err(LexerError::UnclosedDirectiveArgs {
                            directive: name,
                            line: start_line,
                            column: start_col,
                        });
                    }

                    Some(self.input[arg_start..arg_end].trim().to_string())
                } else {
                    None
                };

                tokens.push(Token::Directive { name, args });
                continue;
            }

            // 7. Regular character: accumulate into current_text
            let ch = self.advance().unwrap();
            current_text.push(ch);
        }

        self.flush_text(&mut tokens, &mut current_text);
        Ok(tokens)
    }

    /// Flushes accumulated text into a `Token::Text` if non-empty.
    fn flush_text(&self, tokens: &mut Vec<Token>, current_text: &mut String) {
        if !current_text.is_empty() {
            tokens.push(Token::Text(std::mem::take(current_text)));
        }
    }

    /// Returns the remaining unconsumed slice of input.
    fn remaining(&self) -> &'a str {
        &self.input[self.cursor..]
    }

    /// Returns the next character without advancing the cursor.
    fn peek(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    /// Advances the cursor by one character, updating line, column, and last_char.
    fn advance(&mut self) -> Option<char> {
        if let Some(ch) = self.peek() {
            self.cursor += ch.len_utf8();
            if ch == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
            self.last_char = Some(ch);
            Some(ch)
        } else {
            None
        }
    }

    /// Advances the cursor across a known ASCII substring.
    fn advance_str(&mut self, s: &str) {
        for ch in s.chars() {
            if ch == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
            self.last_char = Some(ch);
        }
        self.cursor += s.len();
    }

    /// Consumes a string literal enclosed by `quote` (`'` or `"`), respecting backslash escapes.
    fn scan_string_literal(&mut self, quote: char) {
        self.advance(); // consume opening quote
        let mut escaped = false;

        while !self.is_eof() {
            let ch = self.peek().unwrap();
            if escaped {
                escaped = false;
                self.advance();
            } else if ch == '\\' {
                escaped = true;
                self.advance();
            } else if ch == quote {
                self.advance(); // consume closing quote
                break;
            } else {
                self.advance();
            }
        }
    }

    /// Returns true if the cursor has reached the end of the input.
    fn is_eof(&self) -> bool {
        self.cursor >= self.input.len()
    }
}
