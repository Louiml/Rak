use std::env;
use std::fs;
use std::io::{self, Read};

const VERSION: &str = "0.2.0";

fn print_usage() {
    eprintln!("rakc {} - Rak language compiler", VERSION);
    eprintln!();
    eprintln!("Usage: rakc <command> [file]");
    eprintln!();
    eprintln!("Commands:");
        eprintln!("  run <file>     Run a Rak script (interpreter)");
        eprintln!("  vm <file>      Run a Rak script on the bytecode VM");
        eprintln!("  bench <file>   Benchmark interpreter vs VM");
        eprintln!("  check <file>   Lex + parse, print diagnostics");
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
        "check" => {
            match rakc::lexer::tokenize(&source) {
                Ok(tokens) => match rakc::parser::parse(&tokens, &source) {
                    Ok(_) => println!("{}: no errors", file),
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                },
                Err(e) => {
                    eprintln!("{}", e);
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
                    match rakc::parser::parse(&tokens, &source) {
                        Ok(ast) => println!("{:#?}", ast),
                        Err(e) => eprintln!("Parser error: {}", e),
                    }
                }
                Err(e) => eprintln!("Lexer error: {}", e),
            }
        }
        "vm" => {
            match rakc::lexer::tokenize(&source) {
                Ok(tokens) => {
                    match rakc::parser::parse(&tokens, &source) {
                        Ok(ast) => {
                            match rakc::compiler::compile_module(&ast) {
                                Ok(chunk) => {
                                    let mut vm = rakc::vm::Vm::new();
                                    match vm.run(&chunk) {
                                        Ok(out) => {
                                            for line in &out {
                                                println!("{}", line);
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("VM error: {}", e);
                                            std::process::exit(1);
                                        }
                                    }
                                }
                                Err(e) => eprintln!("Compile error: {}", e),
                            }
                        }
                        Err(e) => eprintln!("Parser error: {}", e),
                    }
                }
                Err(e) => eprintln!("Lexer error: {}", e),
            }
        }
        "bench" => {
            let tokens = rakc::lexer::tokenize(&source).expect("lex");
            let ast = rakc::parser::parse(&tokens, &source).expect("parse");
            let t0 = std::time::Instant::now();
            let _ = rakc::eval(&source);
            let interp_ms = t0.elapsed().as_millis();
            let chunk = rakc::compiler::compile_module(&ast).expect("compile");
            let t1 = std::time::Instant::now();
            let mut vm = rakc::vm::Vm::new();
            vm.run(&chunk).expect("vm run");
            let vm_ms = t1.elapsed().as_millis();
            println!("interpreter: {} ms", interp_ms);
            println!("vm:           {} ms", vm_ms);
        }
        _ => {
            eprintln!("Unknown command: {}", cmd);
            print_usage();
            std::process::exit(1);
        }
    }
}