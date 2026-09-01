use std::env;
use std::fs;
use std::io::{self, Read};

const VERSION: &str = "0.1.0";

fn print_usage() {
    eprintln!("rakc {} - Rak language compiler", VERSION);
    eprintln!();
    eprintln!("Usage: rakc <command> [file]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  run <file>     Run a Rak script");
    eprintln!("  lex <file>     Tokenize and print tokens");
    eprintln!("  parse <file>   Parse and print AST");
    eprintln!("  version        Print version");
    eprintln!();
    eprintln!("Use - for file to read from stdin");
}

fn read_source(arg: &str) -> String {
    if arg == "-" {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf).expect("Failed to read from stdin");
        buf
    } else {
        fs::read_to_string(arg).expect("Failed to read file")
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    let cmd = &args[1];

    match cmd.as_str() {
        "--version" | "-V" | "version" => {
            println!("rakc {}", VERSION);
            return;
        }
        "--help" | "-h" | "help" => {
            print_usage();
            return;
        }
        _ => {}
    }

    if args.len() < 3 {
        eprintln!("Missing file argument");
        print_usage();
        std::process::exit(1);
    }

    let file = &args[2];
    let source = read_source(file);

    match cmd.as_str() {
        "run" => {
            match rakc::eval(&source) {
                Ok(output) => {
                    for line in &output {
                        println!("{}", line);
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        "lex" => {
            match rakc::lexer::tokenize(&source) {
                Ok(tokens) => {
                    for tok in tokens {
                        println!("{:?}", tok);
                    }
                }
                Err(e) => eprintln!("Lexer error: {}", e),
            }
        }
        "parse" => {
            match rakc::lexer::tokenize(&source) {
                Ok(tokens) => {
                    match rakc::parser::parse(&tokens) {
                        Ok(ast) => println!("{:#?}", ast),
                        Err(e) => eprintln!("Parser error: {}", e),
                    }
                }
                Err(e) => eprintln!("Lexer error: {}", e),
            }
        }
        _ => {
            eprintln!("Unknown command: {}", cmd);
            print_usage();
            std::process::exit(1);
        }
    }
}