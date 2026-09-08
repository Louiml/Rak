use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;

const VERSION: &str = "0.3.0";
const PAYLOAD_MAGIC: u64 = 0x52414B5F50434B; // "RAK_PCK" as u64

fn print_usage() {
    eprintln!("rakc {} - Rak language compiler", VERSION);
    eprintln!();
    eprintln!("Usage: rakc <command> [file]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  run <file>     Run a Rak script (interpreter)");
    eprintln!("  vm <file>      Run a Rak script on the bytecode VM");
    eprintln!("  bench <file>   Benchmark interpreter vs VM");
    eprintln!("  build <file>   Build a standalone .exe from a Rak script");
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

fn check_embedded_payload() -> Option<String> {
    let exe_path = env::current_exe().ok()?;
    let data = fs::read(&exe_path).ok()?;
    if data.len() < 16 {
        return None;
    }
    let magic_bytes = &data[data.len() - 8..];
    let magic = u64::from_be_bytes(magic_bytes.try_into().ok()?);
    if magic != PAYLOAD_MAGIC {
        return None;
    }
    let len_bytes = &data[data.len() - 16..data.len() - 8];
    let source_len = u64::from_be_bytes(len_bytes.try_into().ok()?) as usize;
    let payload_start = data.len() - 16 - source_len;
    if payload_start < 0 || payload_start >= data.len() {
        return None;
    }
    let payload = &data[payload_start..data.len() - 16];
    String::from_utf8(payload.to_vec()).ok()
}

fn build_exe(source_path: &str) {
    let source = match fs::read_to_string(source_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading source: {}", e);
            std::process::exit(1);
        }
    };
    let exe_path = env::current_exe().unwrap_or_else(|_| PathBuf::from("rakc.exe"));
    let exe_data = fs::read(&exe_path).unwrap_or_else(|e| {
        eprintln!("Error reading rakc binary: {}", e);
        std::process::exit(1);
    });
    let source_bytes = source.as_bytes();
    let source_len = source_bytes.len() as u64;
    let output_name = if source_path.ends_with(".rak") {
        format!("{}.exe", &source_path[..source_path.len() - 4])
    } else {
        format!("{}.exe", source_path)
    };
    let mut output = fs::File::create(&output_name).unwrap_or_else(|e| {
        eprintln!("Error creating output: {}", e);
        std::process::exit(1);
    });
    output.write_all(&exe_data).unwrap();
    output.write_all(source_bytes).unwrap();
    output.write_all(&source_len.to_be_bytes()).unwrap();
    output.write_all(&PAYLOAD_MAGIC.to_be_bytes()).unwrap();
    output.flush().unwrap();
    println!("Built: {} ({} bytes)", output_name, exe_data.len() + source_bytes.len() + 16);
    println!("  Source embedded: {} bytes", source_bytes.len());
}

fn main() {
    if let Some(embedded_source) = check_embedded_payload() {
        match rakc::eval(&embedded_source) {
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
        return;
    }

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
        "build" => {
            build_exe(file);
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
