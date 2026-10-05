use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;

use rakc::verify_bounded;

// From the manifest, so the banner cannot drift from what was built -- the
// copy of this string was a literal here and drifted at least once.
const VERSION: &str = env!("CARGO_PKG_VERSION");
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
    eprintln!("  dap <file>     Speak the Debug Adapter Protocol on stdio (editors/IDEs)");
    eprintln!("  repl            Start an interactive REPL");
    #[cfg(feature = "lsp")]
    eprintln!("  lsp             Start the language server (stdio)");
    #[cfg(feature = "bindgen")]
    eprintln!("  bindgen <h> -o <out>  Generate Rak bindings from a C header");
    eprintln!("  check <file>   Lex + parse, print diagnostics");
    eprintln!("  fmt <file>     Format source (--write, --check)");
    eprintln!("  lint <file>    Advisory lint checks (--deny)");
    eprintln!("  fuzz <target>  Property-based fuzz a parser (--runs, --seed, --list)");
    eprintln!(
        "  verify <file>  Run under resource limits, check contracts (--steps, --depth, --iters)"
    );
    eprintln!("  lex <file>     Tokenize and print tokens");
    eprintln!("  parse <file>   Parse and print AST");
    eprintln!("  test [file]    Run Rak tests (--filter NAME, --verbose)");
    eprintln!("  version        Print version");
    eprintln!(
        "
Limits (run/vm): --max-depth N   cap function-call recursion (default 256)"
    );
    eprintln!();
    eprintln!("Use - for file to read from stdin");
    eprintln!();
    eprintln!("Sandbox (run/vm/debug/test): --sandbox [--allow net,fs_write,process,ffi,raw,gui,secrets|all]");
}

/// Run the `rakc verify` command: execute a script under finite resource
/// limits, checking every `requires` / `ensures` clause it reaches.
///
/// The command has three outcomes and reports them differently, because
/// conflating them would be actively misleading:
///
///   0  completed within budget, every contract held
///   1  the program failed: a contract was violated, an assert tripped, or a
///      runtime error was raised
///   2  a resource limit was reached, so the run proved nothing
///
/// Exit code 2 is the one that matters. A liveness bound was hit, so treat it
/// as "not checked" and raise the budget, never as "looks fine".
fn cmd_verify(args: &[String]) -> i32 {
    let mut file: Option<String> = None;
    let mut max_steps: u64 = 1_000_000;
    let mut max_depth: u32 = 256;
    let mut max_iters: u64 = 1_000_000;
    let mut quiet = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--steps" | "-s" => {
                i += 1;
                max_steps = args
                    .get(i)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(max_steps);
            }
            "--depth" | "-d" => {
                i += 1;
                max_depth = args
                    .get(i)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(max_depth);
            }
            "--iters" | "-n" => {
                i += 1;
                max_iters = args
                    .get(i)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(max_iters);
            }
            "--quiet" | "-q" => quiet = true,
            // A bare `-` is the stdin filename, not a flag: it starts with `-`, so
//     classifying by prefix alone made `rakc run -` unreachable.
            //     `is_stdin_marker` is the single place that decides.
            other if !is_flag_token(other) && file.is_none() => {
                file = Some(other.to_string())
            }
            _ => {}
        }
        i += 1;
    }
    let Some(file) = file else {
        eprintln!("Usage: rakc verify <file> [--steps N] [--depth N] [--iters N] [--quiet]");
        return 2;
    };
    let source = read_source(&file);
    let base_dir = std::path::Path::new(&file)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());

    eprintln!(
        "verify: {} (budget: {} steps, depth {}, {} iters/loop)",
        file, max_steps, max_depth, max_iters
    );

    let outcome = verify_bounded(&source, max_steps, max_depth, max_iters);
    match outcome {
        rakc::VerifyOutcome::Completed { steps, output } => {
            if !quiet {
                for line in &output {
                    println!("{}", line);
                }
            }
            println!("PASS  completed in {} steps, all contracts held", steps);
            0
        }
        rakc::VerifyOutcome::Failed {
            message,
            steps,
            output,
        } => {
            if !quiet {
                for line in &output {
                    println!("{}", line);
                }
            }
            // Print the contract failure last so it is the last thing on
            // screen and survives being piped through head.
            eprintln!("FAIL  after {} steps: {}", steps, message);
            1
        }
        rakc::VerifyOutcome::Inconclusive { reason, steps } => {
            eprintln!("SKIP  inconclusive after {} steps: {}", steps, reason);
            eprintln!(
                "      nothing was proved. raise the budget, e.g. --steps {} or --depth {}",
                max_steps.saturating_mul(4),
                max_depth.saturating_mul(2)
            );
            2
        }
    }
}

/// Run the `rakc test` command. Discovers `.rak` test files (an explicitly
/// listed file, or `test.rak` / `tests/*.rak`), parses and runs them in the
/// interpreter, and reports each `test "name" { ... }` block. Supports
/// `--filter NAME` (substring) and `--verbose`.
fn cmd_test(args: &[String]) {
    let mut file: Option<String> = None;
    let mut filter: Option<String> = None;
    let mut verbose = false;
    let mut sandbox = false;
    let mut sandbox_allow = String::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--filter" | "-f" => {
                i += 1;
                filter = args.get(i).cloned();
            }
            "--verbose" | "-v" => verbose = true,
            "--sandbox" => sandbox = true,
            "--allow" => {
                i += 1;
                sandbox_allow = args.get(i).cloned().unwrap_or_default();
            }
            a if is_flag_token(a) => {
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

    if sandbox {
        rakc::caps::enable(&sandbox_allow);
        println!(
            "sandbox: active (allow: {})",
            if sandbox_allow.is_empty() {
                "none".to_string()
            } else {
                sandbox_allow.clone()
            }
        );
    }

    // Discover test files.
    let mut files: Vec<String> = Vec::new();
    if let Some(f) = &file {
        files.push(f.clone());
    } else {
        if let Ok(entries) = fs::read_dir("tests") {
            let mut raks: Vec<String> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map(|x| x == "rak").unwrap_or(false))
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
        // On the big stack, with the same depth cap every other entry point
        // applies. This used to run on the main thread with
        // `Limits::default()`, which is all-`None`: on Windows the main
        // thread's stack is 1 MiB, so a recursive test died with
        // STATUS_STACK_OVERFLOW instead of the clean "recursion depth
        // exceeded" error that `run`, `vm` and `verify` give.
        let (run_result, results) = rakc::run_on_big_stack(move || {
            let mut interp = rakc::interpreter::Interpreter::with_base_dir(base_dir);
            interp.set_max_depth(rakc::interpreter::DEFAULT_MAX_DEPTH);
            let run = interp.run_source(&source);
            (run, interp.run_collected_tests())
        });
        if let Err(e) = run_result {
            eprintln!("error in {}: {}", f, e);
            total_failed += 1;
            continue;
        }
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

/// True for a command-line *flag*, false for the stdin filename `-`.
///
/// The usage text documents `Use - for file to read from stdin`, and
/// `read_source` implements it, but the argument scanner classified anything
/// starting with `-` as a flag. `-` starts with `-`, so `rakc run -` consumed
/// the only filename candidate and failed with "Missing file argument (only
/// flags were given)" -- the documented stdin path was unreachable, and with it
/// any way to pipe a script into `run`, `check`, `vm`, `lint` or `fmt`.
///
/// Length is the discriminator, not the character: `--` and `-x` are flags,
/// the single character `-` is not.
fn is_stdin_marker(arg: &str) -> bool {
    arg == "-"
}

fn is_flag_token(arg: &str) -> bool {
    arg.starts_with('-') && !is_stdin_marker(arg)
}

fn read_source(arg: &str) -> String {
    if arg == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .expect("Failed to read from stdin");
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
    println!(
        "Built: {} ({} bytes)",
        output_name,
        exe_data.len() + source_bytes.len() + 16
    );
    println!("  Source embedded: {} bytes", source_bytes.len());
}

fn cmd_run_debug(file: &str, source: &str) {
    use rakc::vm::{Vm, VmDebugAction};
    let base_dir = std::path::Path::new(file)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());

    let tokens = match rakc::lexer::tokenize(source) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Lexer error: {}", e);
            std::process::exit(1);
        }
    };
    let ast = match rakc::parser::parse(&tokens, source) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Parser error: {}", e);
            std::process::exit(1);
        }
    };
    let chunk = match rakc::compiler::compile_module_in(&ast, &base_dir) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Compile error: {}", e);
            std::process::exit(1);
        }
    };

    // Shared breakpoint set / stepping state consulted by the handler.
    let breakpoints: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<u32>>> =
        std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
    let stepping: std::sync::Arc<std::sync::Mutex<bool>> =
        std::sync::Arc::new(std::sync::Mutex::new(true));

    // The disassembler needs the compiled chunk; share it via Arc.
    let chunk_arc = std::sync::Arc::new(chunk);

    // On the big stack, with the same call-depth cap `rakc vm` applies.
    // This used to run on the main thread uncapped, so stepping into a
    // recursive program took the debugger down with it -- the same bug
    // `main.rs` documents for `vm` and fixes there.
    let result = rakc::run_on_big_stack(move || {
        let mut vm = Vm::new();
        vm.set_max_call_depth(rakc::interpreter::DEFAULT_MAX_DEPTH);
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
                                // `IterItems` has a 1-byte "indexed for" flag. Leaving
                                // it out made `:dis` resync by one byte and print
                                // nonsense for every instruction after a `for` loop.
                                | rakc::bytecode::Op::IterItems
                                | rakc::bytecode::Op::BuildModule => 1,
                                // The module opcodes carry a const-index operand, so
                                // leaving them out of this table made `:dis` decode
                                // their operands as opcodes and print nonsense for
                                // every instruction after the first import.
                                rakc::bytecode::Op::MakeModule
                                | rakc::bytecode::Op::BindModule => 2,
                                // Two const indices (the namespace's global, then the
                                // exported name) plus a 1-byte mutability flag.
                                rakc::bytecode::Op::ModulePublish => 5,
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

        vm.run(&chunk_arc)
    });
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
        "fuzz" => {
            // Takes no file. Must run from a debug build: the harness relies on
            // catch_unwind, and the release profile uses panic = "abort".
            let code = rakc::fuzz::run(&args[2..]);
            std::process::exit(code);
        }
        "test" => {
            cmd_test(&args[2..]);
            return;
        }
        "verify" => {
            let code = cmd_verify(&args[2..]);
            std::process::exit(code);
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

    // Split the tail into flags and the file, so `rakc fmt --check f.rak` and
    // `rakc lint --deny f.rak` find `f.rak` rather than trying to open
    // `--check`. The usage text spells the flags after the file and that order
    // worked, but a flag-first invocation panicked on a read of the flag rather
    // than reporting a bad argument — and flag-first is the natural way to write
    // it, and what a CI invocation tends to end up as.
    //
    // `--allow` takes a value, so the argument after it is a flag's value and not
    // the filename; without that, `rakc run --sandbox --allow csv f.rak` would try
    // to run `csv`.
    let mut flag_slots: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut i = 2;
    while i < args.len() {
        if is_flag_token(&args[i]) {
            flag_slots.insert(i);
            // A flag whose value is the next argument. Without listing it here the
            // flag is stripped and its value is left behind as a stray script
            // argument -- which is exactly what happened to `--max-depth`: the
            // scanner removed the flag, `parse_cli` never saw it, and the number
            // was handed to the program instead.
            if args[i] == "--allow" || args[i] == "-A" || args[i] == "--max-depth" {
                i += 1;
                if i < args.len() {
                    flag_slots.insert(i);
                }
            }
        }
        i += 1;
    }
    // Read `--max-depth` straight out of `args`, here, rather than out of
    // `script_args` further down.
    //
    // The scanner above has already removed the flag and its value from
    // `script_args`, which is where `parse_cli` looks -- so anything parsed from
    // there can never see a flag written after the filename, which is the order the
    // usage text documents. Parsing from `args` covers both orders, since this loop
    // scans the whole thing.
    // Samples per phase for `rakc bench`.
    let mut bench_repeat: usize = 5;
    let mut max_depth_override: Option<u32> = None;
    for w in args.windows(2) {
        if w[0] == "--repeat" {
            bench_repeat = w[1].parse::<usize>().unwrap_or(0);
        }
        if w[0] == "--max-depth" {
            match w[1].parse::<u32>() {
                Ok(d) if d > 0 => max_depth_override = Some(d),
                _ => eprintln!("--max-depth needs a positive number, e.g. --max-depth 512"),
            }
        }
    }

    let file_index = match (2..args.len()).find(|i| !flag_slots.contains(i)) {
        Some(i) => i,
        None => {
            eprintln!("Missing file argument (only flags were given)");
            print_usage();
            std::process::exit(1);
        }
    };
    let file = &args[file_index];
    let source = read_source(file);

    // Everything after the file, minus the flags that preceded it: those belong
    // to `rakc`, not to the script.
    let script_args: Vec<String> = args[file_index + 1..]
        .iter()
        .enumerate()
        .filter(|(k, _)| !flag_slots.contains(&(file_index + 1 + k)))
        .map(|(_, a)| (*a).clone())
        .collect();

    // Sandbox flags: --sandbox [--allow csv]. Stripped from script argv.
    let (sandbox_on, sandbox_allow, parsed_depth, clean_args) = rakc::caps::parse_cli(&script_args);
    let max_depth = max_depth_override.or(parsed_depth);
    if sandbox_on {
        rakc::caps::enable(&sandbox_allow);
        eprintln!(
            "sandbox: active (allow: {})",
            if sandbox_allow.is_empty() {
                "none".to_string()
            } else {
                sandbox_allow.clone()
            }
        );
    }

    match cmd.as_str() {
        "run" => {
            let base_dir = std::path::Path::new(file)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| ".".to_string());
            // argv for a `fn main(args)` entry = everything after the file.
            let script_args: Vec<String> = clean_args;
            #[cfg(feature = "gui")]
            let result = rakc::gui::eval_in_cli_with_gui(&source, &base_dir, &script_args);
            #[cfg(not(feature = "gui"))]
            let result = rakc::eval_in_cli(&source, &base_dir, &script_args, max_depth);
            match result {
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
        "dap" => {
            rakc::dap::run(file, &source);
        }
        "build" => {
            build_exe(file);
        }
        "check" => match rakc::lexer::tokenize(&source) {
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
        },
        "lex" => match rakc::lexer::tokenize(&source) {
            Ok(tokens) => {
                for tok in tokens {
                    println!("{:?}", tok);
                }
            }
            Err(e) => eprintln!("Lexer error: {}", e),
        },
        "parse" => match rakc::lexer::tokenize(&source) {
            Ok(tokens) => match rakc::parser::parse(&tokens, &source) {
                Ok(ast) => println!("{:#?}", ast),
                Err(e) => eprintln!("Parser error: {}", e),
            },
            Err(e) => eprintln!("Lexer error: {}", e),
        },
        "fmt" => {
            // `rakc fmt file [--write|--check]`
            // Flags may sit before or after the file, so collect every one of them.
            let flags: Vec<&String> = args
                .iter()
                .enumerate()
                .filter(|(i, _)| flag_slots.contains(i))
                .map(|(_, a)| a)
                .collect();
            let write = flags
                .iter()
                .any(|f| f.as_str() == "--write" || f.as_str() == "-w");
            let check = flags
                .iter()
                .any(|f| f.as_str() == "--check" || f.as_str() == "-c");
            match rakc::fmt::format_source(&source) {
                Ok(formatted) => {
                    if check {
                        if formatted == source {
                            println!("{}: formatted", file);
                        } else {
                            eprintln!("{}: not formatted (run `rakc fmt {} --write`)", file, file);
                            std::process::exit(1);
                        }
                    } else if write {
                        if formatted != source {
                            std::fs::write(file, &formatted)
                                .map_err(|e| format!("fmt: cannot write '{}': {}", file, e))
                                .unwrap();
                            println!("{}: formatted", file);
                        } else {
                            println!("{}: already formatted", file);
                        }
                    } else {
                        print!("{}", formatted);
                    }
                }
                Err(e) => {
                    eprintln!("fmt: {}", e);
                    std::process::exit(1);
                }
            }
        }
        "lint" => {
            // `rakc lint file [--deny] [--audit]`
            // Flags may sit before or after the file, so collect every one of them.
            let flags: Vec<&String> = args
                .iter()
                .enumerate()
                .filter(|(i, _)| flag_slots.contains(i))
                .map(|(_, a)| a)
                .collect();
            let deny = flags
                .iter()
                .any(|f| f.as_str() == "--deny" || f.as_str() == "-d");
            let audit = flags
                .iter()
                .any(|f| f.as_str() == "--audit" || f.as_str() == "-a");
            match rakc::lint::lint_source_full(&source) {
                Ok(report) => {
                    // --audit answers "which safety exemptions does this
                    // program claim, and what does it say about each one".
                    // It is the review artifact, so it prints even when the
                    // file is otherwise clean.
                    if audit {
                        println!("{}", rakc::lint::format_audit(file, &report.unsafe_sites));
                    }
                    if report.findings.is_empty() && !audit {
                        println!("{}: no warnings", file);
                    } else {
                        for f in &report.findings {
                            println!("warning[{}]: {}", f.rule, f.message);
                        }
                        if deny && !report.findings.is_empty() {
                            eprintln!("{}: {} warnings (denied)", file, report.findings.len());
                            std::process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("lint: {}", e);
                    std::process::exit(1);
                }
            }
        }
        "vm" => {
            let base_dir = std::path::Path::new(file)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| ".".to_string());
            match rakc::lexer::tokenize(&source) {
                Ok(tokens) => {
                    match rakc::parser::parse(&tokens, &source) {
                        Ok(ast) => {
                            match rakc::compiler::compile_module_in(&ast, &base_dir) {
                                Ok(chunk) => {
                                    // On the big stack, like the interpreter.
                                    //
                                    // `run` has always used `run_on_big_stack` and
                                    // `vm` has always run on the main thread, which on
                                    // Windows defaults to 1 MiB. That made the same
                                    // recursive program survive `rakc run` and abort
                                    // `rakc vm` -- not a VM frame cost difference, a
                                    // stack size difference, and it fired before the
                                    // depth guard could turn it into an error.
                                    let md = max_depth;
                                    let outcome = rakc::run_on_big_stack(move || {
                                        let mut vm = rakc::vm::Vm::new();
                                        if let Some(d) = md {
                                            vm.set_max_call_depth(d);
                                        }
                                        vm.run(&chunk)
                                    });
                                    match outcome {
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
            // Five phases, measured separately.
            //
            // The old version timed `rakc::eval(&source)` against `vm.run()` and
            // nothing else. That is not a comparison of two backends: the
            // interpreter side includes lexing, parsing and setup, while the VM side
            // is bytecode execution with the chunk already compiled, and compile time
            // was attributed to nobody. The reported gap was therefore mostly
            // front-end cost -- and `as_millis()` meant anything under a millisecond
            // printed `0 ms`.
            //
            // Now each phase is measured over `--repeat` samples after an unmeasured
            // warm-up, and the two execution numbers are finally like for like.
            let repeat = if bench_repeat == 0 {
                eprintln!("--repeat needs a positive number");
                std::process::exit(1);
            } else {
                bench_repeat
            };

            let tokens = match rakc::lexer::tokenize(&source) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("lex: {}", e);
                    std::process::exit(1);
                }
            };
            let ast = match rakc::parser::parse(&tokens, &source) {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("parse: {}", e);
                    std::process::exit(1);
                }
            };
            let chunk = match rakc::compiler::compile_module(&ast) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("compile: {}", e);
                    std::process::exit(1);
                }
            };

            // One unmeasured run first, so page faults, allocator growth and the CPU's
            // frequency ramp are not charged to sample 1.
            let _ = rakc::eval(&source);
            let _ = rakc::vm::Vm::new().run(&chunk);

            let mut v: Vec<Vec<u64>> = vec![Vec::new(); 5];
            for _ in 0..repeat {
                let t = std::time::Instant::now();
                let toks = rakc::lexer::tokenize(&source).expect("lex");
                v[0].push(t.elapsed().as_nanos() as u64);

                let t = std::time::Instant::now();
                let parsed = rakc::parser::parse(&toks, &source).expect("parse");
                v[1].push(t.elapsed().as_nanos() as u64);

                let t = std::time::Instant::now();
                let c = rakc::compiler::compile_module(&parsed).expect("compile");
                v[2].push(t.elapsed().as_nanos() as u64);
                std::hint::black_box(&c);

                let t = std::time::Instant::now();
                let out = rakc::eval(&source);
                v[3].push(t.elapsed().as_nanos() as u64);
                std::hint::black_box(&out);

                let t = std::time::Instant::now();
                let out = rakc::vm::Vm::new().run(&chunk);
                v[4].push(t.elapsed().as_nanos() as u64);
                std::hint::black_box(&out);
            }

            // Median, not mean: one scheduler interrupt in twenty samples moves a
            // mean by 5% and a median by nothing.
            fn median(v: &mut Vec<u64>) -> f64 {
                v.sort_unstable();
                v[v.len() / 2] as f64 / 1_000_000.0
            }
            let lex = median(&mut v[0]);
            let parse = median(&mut v[1]);
            let compile = median(&mut v[2]);
            let interp = median(&mut v[3]);
            let vm = median(&mut v[4]);

            println!("{} ({} samples, median)", file, repeat);
            println!("  lex      {:>9.3} ms", lex);
            println!("  parse    {:>9.3} ms", parse);
            println!("  compile  {:>9.3} ms", compile);
            println!("  ---");
            println!("  interp   {:>9.3} ms  (execution only)", interp);
            println!("  vm       {:>9.3} ms  (execution only)", vm);
            let front = lex + parse + compile;
            let total = interp + front;
            println!("  ---");
            if total > 0.0 {
                println!(
                    "  front end (lex+parse+compile) {:>7.3} ms, {:.0}% of the interpreter path",
                    front,
                    100.0 * front / total
                );
            }
            if vm > 0.0 {
                println!("  execution speedup (vm vs interp): {:.2}x", interp / vm);
            }
        }
        _ => {
            eprintln!("Unknown command: {}", cmd);
            print_usage();
            std::process::exit(1);
        }
    }
}
