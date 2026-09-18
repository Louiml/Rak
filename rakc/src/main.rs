use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;

const VERSION: &str = "0.7.1";
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
    eprintln!("  build <file>   Build a standalone executable from a Rak script");
    eprintln!("  debug <file>   Run a line-oriented interactive debugger");
    eprintln!("  repl            Start an interactive REPL");
    #[cfg(feature = "lsp")]
    eprintln!("  lsp             Start the language server (stdio)");
    #[cfg(feature = "bindgen")]
    eprintln!("  bindgen <h> -o <out>  Generate Rak bindings from a C header");
    eprintln!("  check <file>   Lex + parse, print diagnostics");
    eprintln!("  lex <file>     Tokenize and print tokens");
    eprintln!("  parse <file>   Parse and print AST");
    eprintln!("  test [file]    Run Rak tests (--filter NAME, --verbose)");
    eprintln!("  version        Print version");
    eprintln!();
    eprintln!("Use - for file to read from stdin");
}

/// Run the `rakc test` command. Discovers `.rak` test files (an explicitly
/// listed file, or `test.rak` / `tests/*.rak`), parses and runs them in the
/// interpreter, and reports each `test "name" { ... }` block. Supports
/// `--filter NAME` (substring) and `--verbose`.
fn cmd_test(args: &[String]) {
    let mut file: Option<String> = None;
    let mut filter: Option<String> = None;
    let mut verbose = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--filter" | "-f" => {
                i += 1;
                filter = args.get(i).cloned();
            }
            "--verbose" | "-v" => verbose = true,
            a if a.starts_with('-') => {
                eprintln!("Unknown test flag: {}", a);
                std::process::exit(1);
            }
            other => {
                if file.is_none() {
                    file = Some(other.to_string());
                }
            }
        }
        i += 1;
    }

    // Discover test files.
    let mut files: Vec<String> = Vec::new();
    if let Some(f) = &file {
        files.push(f.clone());
    } else {
        if let Ok(entries) = fs::read_dir("tests") {
            let mut raks: Vec<String> = entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .map(|x| x == "rak")
                        .unwrap_or(false)
                })
                .map(|e| e.path().to_string_lossy().to_string())
                .collect();
            raks.sort();
            for r in raks {
                files.push(r);
            }
        }
        if files.is_empty() {
            files.push("test.rak".to_string());
        }
    }

    println!("Running Rak tests...");
    println!();

    let mut total_passed = 0usize;
    let mut total_failed = 0usize;

    for f in &files {
        let source = match fs::read_to_string(f) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading {}: {}", f, e);
                total_failed += 1;
                continue;
            }
        };
        let base_dir = std::path::Path::new(f)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());
        let mut interp = rakc::interpreter::Interpreter::with_base_dir(base_dir);
        if let Err(e) = interp.run_source(&source) {
            eprintln!("error in {}: {}", f, e);
            total_failed += 1;
            continue;
        }
        let results = interp.run_collected_tests();
        let file_stem = std::path::Path::new(f)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| f.clone());
        for r in results {
            if let Some(filt) = &filter {
                if !r.name.contains(filt.as_str()) {
                    continue;
                }
            }
            if r.passed {
                total_passed += 1;
                println!("PASS  {}/{}", file_stem, r.name);
            } else {
                total_failed += 1;
                println!("FAIL  {}/{}", file_stem, r.name);
                if verbose {
                    eprintln!("      {}", r.message.as_deref().unwrap_or("(failed)"));
                }
            }
        }
    }

    println!();
    println!("{} passed", total_passed);
    println!("{} failed", total_failed);

    if total_failed > 0 {
        std::process::exit(1);
    }
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
    let payload_start = data.len().saturating_sub(16 + source_len);
    if payload_start >= data.len() {
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
    let exe_name = if cfg!(windows) { "rakc.exe" } else { "rakc" };
    let exe_path = env::current_exe().unwrap_or_else(|_| PathBuf::from(exe_name));
    let exe_data = fs::read(&exe_path).unwrap_or_else(|e| {
        eprintln!("Error reading rakc binary: {}", e);
        std::process::exit(1);
    });
    let source_bytes = source.as_bytes();
    let source_len = source_bytes.len() as u64;
    let ext = if cfg!(windows) { ".exe" } else { "" };
    let output_name = if source_path.ends_with(".rak") {
        format!("{}{}", &source_path[..source_path.len() - 4], ext)
    } else {
        format!("{}{}", source_path, ext)
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
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o755);
        let _ = output.set_permissions(perms);
    }
    println!("Built: {} ({} bytes)", output_name, exe_data.len() + source_bytes.len() + 16);
    println!("  Source embedded: {} bytes", source_bytes.len());
}

fn cmd_run_debug(file: &str, source: &str) {
    use rakc::vm::{Vm, VmDebugAction};
    let base_dir = std::path::Path::new(file).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| ".".to_string());

    let tokens = match rakc::lexer::tokenize(source) {
        Ok(t) => t,
        Err(e) => { eprintln!("Lexer error: {}", e); std::process::exit(1); }
    };
    let ast = match rakc::parser::parse(&tokens, source) {
        Ok(a) => a,
        Err(e) => { eprintln!("Parser error: {}", e); std::process::exit(1); }
    };
    let chunk = match rakc::compiler::compile_module_in(&ast, &base_dir) {
        Ok(c) => c,
        Err(e) => { eprintln!("Compile error: {}", e); std::process::exit(1); }
    };

    // Shared breakpoint set / stepping state consulted by the handler.
    let breakpoints: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<u32>>> =
        std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
    let stepping: std::sync::Arc<std::sync::Mutex<bool>> = std::sync::Arc::new(std::sync::Mutex::new(true));

    // The disassembler needs the compiled chunk; share it via Arc.
    let chunk_arc = std::sync::Arc::new(chunk);

    let mut vm = Vm::new();
    let bps = breakpoints.clone();
    let stp = stepping.clone();
    let chunk_d = chunk_arc.clone();
    vm.debugger(std::collections::HashSet::new(), move |line, locals, _globals, callstack| {
        // Fast path: don't pause unless stepping or on a breakpoint.
        if !*stp.lock().unwrap() && !bps.lock().unwrap().contains(&line) {
            return VmDebugAction::Continue;
        }
        *stp.lock().unwrap() = false;
        println!("\n[stopped] line {}", line);
        loop {
            print!("dbg> ");
            use std::io::Write as _;
            let _ = std::io::stdout().flush();
            let mut input = String::new();
            if std::io::stdin().read_line(&mut input).unwrap_or(0) == 0 {
                // EOF on stdin: run to completion.
                return VmDebugAction::Continue;
            }
            let trimmed = input.trim().to_string();
            if trimmed.is_empty() {
                for (k, v) in &locals {
                    println!("  {} = {}", k, v);
                }
                continue;
            }
            let lower = trimmed.to_lowercase();
            match lower.as_str() {
                ":help" | "help" | "h" => {
                    println!("break <line|file:line> | continue/c | step/s | next/n | finish | locals | stack/bt | backtrace | print <name> | disassemble | frame | quit/q");
                    continue;
                }
                "c" | "continue" => return VmDebugAction::Continue,
                "s" | "step" | "n" | "next" => {
                    *stp.lock().unwrap() = true;
                    return VmDebugAction::Step;
                }
                "finish" => {
                    // Run until the next breakpoint or program end.
                    return VmDebugAction::Continue;
                }
                "locals" => {
                    if locals.is_empty() {
                        println!("(no locals)");
                    }
                    for (k, v) in &locals {
                        println!("  {} = {}", k, v);
                    }
                    continue;
                }
                "stack" | "bt" | "backtrace" => {
                    println!("  #0 <main> (line {})", line);
                    for (i, name) in callstack.iter().enumerate() {
                        println!("  #{} {}", i + 1, name);
                    }
                    if callstack.is_empty() {
                        println!("  (no nested calls)");
                    }
                    continue;
                }
                "frame" => {
                    println!("current line {}, stack depth {}", line, callstack.len() + 1);
                    continue;
                }
                "quit" | "q" => return VmDebugAction::Quit,
                _ => {}
            }
            if let Some(rest) = lower.strip_prefix("break ") {
                // Accept both `break 42` and `break file.rak:42`.
                let num_part = rest.trim().rsplit(':').next().unwrap_or(rest.trim());
                if let Ok(l) = num_part.trim().parse::<u32>() {
                    bps.lock().unwrap().insert(l);
                    println!("breakpoint set at line {}", l);
                } else {
                    println!("break <line|file:line> expects a number");
                }
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("print ") {
                let name = rest.trim().to_string();
                match locals.iter().find(|(k, _)| *k == name) {
                    Some((_, v)) => println!("{}", v),
                    None => println!("(no such local '{}'; try 'locals')", name),
                }
                continue;
            }
            if lower == "disassemble" || lower == "dis" {
                println!("bytecode: {} bytes, {} constants, {} line markers", chunk_d.code.len(), chunk_d.constants.len(), chunk_d.lines.len());
                let mut off = 0usize;
                while off < chunk_d.code.len() {
                    if let Some(op) = rakc::bytecode::Op::from_u8(chunk_d.code[off]) {
                        let l = chunk_d.lines.get(off).copied().unwrap_or(0);
                        let width = match op {
                            rakc::bytecode::Op::LoadConst
                            | rakc::bytecode::Op::LoadGlobal
                            | rakc::bytecode::Op::StoreGlobal
                            | rakc::bytecode::Op::Jump
                            | rakc::bytecode::Op::JumpIfFalse
                            | rakc::bytecode::Op::JumpIfTrue => 2,
                            rakc::bytecode::Op::LoadLocal
                            | rakc::bytecode::Op::StoreLocal
                            | rakc::bytecode::Op::Call
                            | rakc::bytecode::Op::BuildModule => 1,
                            _ => 0,
                        };
                        let operand = if width == 2 {
                            format!("{}", chunk_d.read_u16(off + 1))
                        } else if width == 1 {
                            format!("{}", chunk_d.code[off + 1])
                        } else {
                            String::new()
                        };
                        println!("  {:04}  line {:>3}  {:?} {}", off, l, op, operand);
                        off += 1 + width;
                    } else {
                        println!("  {:04}  <bad opcode> {}", off, chunk_d.code[off]);
                        off += 1;
                    }
                }
                continue;
            }
            println!("unknown command '{}' (:help)", input);
        }
    });

    let result = vm.run(&chunk_arc);
    match result {
        Ok(out) => {
            for l in &out {
                println!("{}", l);
            }
        }
        Err(e) => {
            eprintln!("VM error: {}", e);
            std::process::exit(1);
        }
    }
    std::process::exit(0);
}

fn main() {
    if let Some(embedded_source) = check_embedded_payload() {
        let script_args: Vec<String> = env::args().skip(1).collect();
        match rakc::eval_cli(&embedded_source, &script_args) {
            Ok((output, code)) => {
                for line in &output {
                    println!("{}", line);
                }
                std::process::exit(code);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
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
        "repl" => {
            rakc::repl::run();
            return;
        }
        "test" => {
            cmd_test(&args[2..]);
            return;
        }
        #[cfg(feature = "lsp")]
        "lsp" => {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(rakc::lsp::start());
            return;
        }
        #[cfg(feature = "bindgen")]
        "bindgen" => {
            if args.len() < 3 {
                eprintln!("Usage: rakc bindgen <header.h> [-o output.rak]");
                std::process::exit(1);
            }
            let header = &args[2];
            let mut output = "-".to_string();
            for i in 3..args.len() {
                if args[i] == "-o" && i + 1 < args.len() {
                    output = args[i + 1].clone();
                }
            }
            if let Err(e) = rakc::bindgen::generate(header, &output) {
                eprintln!("bindgen error: {}", e);
                std::process::exit(1);
            }
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
            let base_dir = std::path::Path::new(file).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| ".".to_string());
            // argv for a `fn main(args)` entry = everything after the file.
            let script_args: Vec<String> = args.iter().skip(3).cloned().collect();
            match rakc::eval_in_cli(&source, &base_dir, &script_args) {
                Ok((output, code)) => {
                    for line in &output {
                        println!("{}", line);
                    }
                    if code != 0 {
                        std::process::exit(code);
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        "debug" => {
            cmd_run_debug(file, &source);
        }
        "build" => {
            build_exe(file);
        }
        "check" => {
            match rakc::lexer::tokenize(&source) {
                Ok(tokens) => match rakc::parser::parse(&tokens, &source) {
                    Ok(ast) => {
                        let mut tc = rakc::typecheck::TypeChecker::new(&source, file);
                        let diagnostics = tc.check_module(&ast);
                        if diagnostics.is_empty() {
                            println!("{}: no errors", file);
                        } else {
                            for d in &diagnostics {
                                eprintln!("{}", d.render());
                            }
                            std::process::exit(1);
                        }
                    }
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
            let base_dir = std::path::Path::new(file).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| ".".to_string());
            match rakc::lexer::tokenize(&source) {
                Ok(tokens) => {
                    match rakc::parser::parse(&tokens, &source) {
                        Ok(ast) => {
                            match rakc::compiler::compile_module_in(&ast, &base_dir) {
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
