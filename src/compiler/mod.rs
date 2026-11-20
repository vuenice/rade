pub mod directives;

pub use directives::{
    compile_directive, compile_else, compile_elseif, compile_endfor, compile_endif, compile_for,
    compile_if, is_builtin_directive, DirectiveEntry, DirectiveKind, DirectiveStack,
};

use std::fmt;
use crate::lexer::{LexerError, Scanner, SpannedToken, Token};

/// Errors that can occur during template compilation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// Lexer error occurred during scanning
    Lexer(LexerError),

    /// A closing directive (@endif, @endfor) was encountered without a matching opener
    UnexpectedClosingDirective {
        directive: String,
        line: usize,
        column: usize,
    },

    /// A closing directive does not match the currently open block
    MismatchedClosingDirective {
        expected: String,
        found: String,
        line: usize,
        column: usize,
    },

    /// The template ended with an unclosed block (@if or @for)
    UnclosedDirective {
        directive: String,
        line: usize,
        column: usize,
    },

    /// A directive appears in an invalid context (e.g. @else or @elseif outside of an @if block)
    UnexpectedDirective {
        directive: String,
        line: usize,
        column: usize,
        message: String,
    },

    /// A directive requires arguments that were not provided or were empty
    MissingDirectiveArgs {
        directive: String,
        line: usize,
        column: usize,
    },

    /// A directive received unexpected arguments
    UnexpectedDirectiveArgs {
        directive: String,
        line: usize,
        column: usize,
    },
}

impl CompileError {
    /// Returns the 1-indexed line number where the error occurred or started.
    pub fn line(&self) -> usize {
        match self {
            CompileError::Lexer(err) => match err {
                LexerError::UnclosedComment { line, .. }
                | LexerError::UnclosedEcho { line, .. }
                | LexerError::UnclosedRawEcho { line, .. }
                | LexerError::UnclosedRawRhp { line, .. }
                | LexerError::UnclosedDirectiveArgs { line, .. } => *line,
            },
            CompileError::UnexpectedClosingDirective { line, .. }
            | CompileError::MismatchedClosingDirective { line, .. }
            | CompileError::UnclosedDirective { line, .. }
            | CompileError::UnexpectedDirective { line, .. }
            | CompileError::MissingDirectiveArgs { line, .. }
            | CompileError::UnexpectedDirectiveArgs { line, .. } => *line,
        }
    }

    /// Returns the 1-indexed column number where the error occurred or started.
    pub fn column(&self) -> usize {
        match self {
            CompileError::Lexer(err) => match err {
                LexerError::UnclosedComment { column, .. }
                | LexerError::UnclosedEcho { column, .. }
                | LexerError::UnclosedRawEcho { column, .. }
                | LexerError::UnclosedRawRhp { column, .. }
                | LexerError::UnclosedDirectiveArgs { column, .. } => *column,
            },
            CompileError::UnexpectedClosingDirective { column, .. }
            | CompileError::MismatchedClosingDirective { column, .. }
            | CompileError::UnclosedDirective { column, .. }
            | CompileError::UnexpectedDirective { column, .. }
            | CompileError::MissingDirectiveArgs { column, .. }
            | CompileError::UnexpectedDirectiveArgs { column, .. } => *column,
        }
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompileError::Lexer(err) => write!(f, "Lexer error: {err}"),
            CompileError::UnexpectedClosingDirective {
                directive,
                line,
                column,
            } => {
                write!(
                    f,
                    "Unexpected closing directive '@{directive}' without matching opener at line {line}, column {column}"
                )
            }
            CompileError::MismatchedClosingDirective {
                expected,
                found,
                line,
                column,
            } => {
                write!(
                    f,
                    "Mismatched closing directive: expected '@{expected}', found '@{found}' at line {line}, column {column}"
                )
            }
            CompileError::UnclosedDirective {
                directive,
                line,
                column,
            } => {
                write!(
                    f,
                    "Unclosed directive '@{directive}' starting at line {line}, column {column}"
                )
            }
            CompileError::UnexpectedDirective {
                directive,
                line,
                column,
                message,
            } => {
                write!(
                    f,
                    "Unexpected directive '@{directive}' at line {line}, column {column}: {message}"
                )
            }
            CompileError::MissingDirectiveArgs {
                directive,
                line,
                column,
            } => {
                write!(
                    f,
                    "Directive '@{directive}' requires arguments at line {line}, column {column}"
                )
            }
            CompileError::UnexpectedDirectiveArgs {
                directive,
                line,
                column,
            } => {
                write!(
                    f,
                    "Directive '@{directive}' does not accept arguments at line {line}, column {column}"
                )
            }
        }
    }
}

impl std::error::Error for CompileError {}

impl From<LexerError> for CompileError {
    fn from(err: LexerError) -> Self {
        CompileError::Lexer(err)
    }
}

/// Compiler for Rade templates that compiles built-in directives, raw RHP blocks,
/// and template tokens into target code.
#[derive(Debug, Clone, Default)]
pub struct Compiler {
    stack: DirectiveStack,
}

impl Compiler {
    /// Creates a new `Compiler` instance with an empty directive stack.
    pub fn new() -> Self {
        Self {
            stack: DirectiveStack::new(),
        }
    }

    /// Compiles a template string into target RHP/template code.
    pub fn compile(&mut self, input: &str) -> Result<String, CompileError> {
        let spanned_tokens = Scanner::scan_spanned(input)?;
        self.compile_spanned_tokens(&spanned_tokens)
    }

    /// Compiles a slice of `SpannedToken`s.
    pub fn compile_spanned_tokens(
        &mut self,
        tokens: &[SpannedToken],
    ) -> Result<String, CompileError> {
        let mut output = String::new();

        for spanned in tokens {
            match &spanned.token {
                Token::Text(text) => {
                    output.push_str(text);
                }
                Token::Comment(_) => {
                    // Blade comments are stripped during compilation
                }
                Token::RawRhp(code) => {
                    // Directly emits the inner RHP code as-is without any wrapping or alteration
                    output.push_str(code);
                }
                Token::Directive { name, args } => {
                    let compiled = compile_directive(
                        name,
                        args.as_deref(),
                        spanned.line,
                        spanned.column,
                        &mut self.stack,
                    )?;
                    output.push_str(&compiled);
                }
                Token::EchoEscaped(expr) => {
                    output.push_str("{{ ");
                    output.push_str(expr);
                    output.push_str(" }}");
                }
                Token::EchoRaw(expr) => {
                    output.push_str("{!! ");
                    output.push_str(expr);
                    output.push_str(" !!}");
                }
                Token::EscapedVerbatim(verbatim) => {
                    output.push_str(verbatim);
                }
            }
        }

        self.stack.finish()?;
        Ok(output)
    }

    /// Compiles a slice of `Token`s, using line 1, column 1 as fallback positions.
    pub fn compile_tokens(&mut self, tokens: &[Token]) -> Result<String, CompileError> {
        let spanned: Vec<SpannedToken> = tokens
            .iter()
            .map(|t| SpannedToken::new(t.clone(), 1, 1))
            .collect();
        self.compile_spanned_tokens(&spanned)
    }
}

/// Convenience helper function to compile a template string in one call.
pub fn compile(input: &str) -> Result<String, CompileError> {
    Compiler::new().compile(input)
}
