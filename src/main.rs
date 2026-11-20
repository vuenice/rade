use std::env;
use std::fs;
use std::process;
use rade::tokenize;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        println!("rade - RHP templating engine");
        println!("Usage: rade <template_file> or pass template via argument");
        return;
    }

    let input = match fs::read_to_string(&args[1]) {
        Ok(content) => content,
        Err(_) => args[1].clone(),
    };

    match tokenize(&input) {
        Ok(tokens) => {
            println!("Tokens ({}):", tokens.len());
            for (i, token) in tokens.iter().enumerate() {
                println!("  [{i}] {token:?}");
            }
        }
        Err(err) => {
            eprintln!("Lexer error: {err}");
            process::exit(1);
        }
    }
}
