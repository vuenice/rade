pub mod lexer;

pub use lexer::{LexerError, Scanner, Token};

/// Tokenizes a template string into a vector of tokens.
pub fn tokenize(input: &str) -> Result<Vec<Token>, LexerError> {
    Scanner::scan(input)
}
