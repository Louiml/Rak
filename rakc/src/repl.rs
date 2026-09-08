use crate::interpreter::Interpreter;

pub fn run() {
    println!("Rak REPL v0.3.0");
    println!("Type :help for a list of commands. Ctrl-D or :quit to exit.");
    println!();

    let mut interp = Interpreter::new();
    let mut history: Vec<String> = Vec::new();
    let mut buffer = String::new();

    let mut rl = rustyline::DefaultEditor::new().unwrap_or_else(|_| rustyline::DefaultEditor::new().unwrap());
    let _ = rl.load_history("~/.rak_history");

    loop {
        let prompt = if buffer.is_empty() { "rak> " } else { "  .. " };
        match rl.readline(prompt) {
            Ok(line) => {
                let _ = rl.add_history_entry(line.as_str());
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if trimmed.starts_with(':') {
                    let cmd = trimmed.trim_start_matches(':');
                    let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
                    match parts[0] {
                        "quit" | "q" => break,
                        "help" => print_help(),
                        "history" => {
                            for (i, h) in history.iter().enumerate() {
                                println!("{}: {}", i + 1, h);
                            }
                        }
                        "clear" => {
                            interp = Interpreter::new();
                            buffer.clear();
                            println!("State cleared");
                        }
                        _ => {
                            let rest = parts.get(1).copied().unwrap_or("");
                            handle_repl_command(&mut interp, parts[0], rest, &mut buffer);
                        }
                    }
                    continue;
                }
                if buffer.is_empty() {
                    history.push(line.clone());
                }
                buffer.push_str(&line);
                buffer.push('\n');

                if is_complete(&buffer) {
                    let src = buffer.clone();
                    buffer.clear();
                    match interp.run_source(&src) {
                        Ok(out) => {
                            for l in out {
                                println!("{}", l);
                            }
                        }
                        Err(e) => println!("error: {}", e),
                    }
                }
            }
            Err(rustyline::error::ReadlineError::Interrupted) => {
                println!("(Ctrl-C typed: :quit to exit)");
                buffer.clear();
            }
            Err(rustyline::error::ReadlineError::Eof) => {
                println!();
                break;
            }
            Err(err) => {
                eprintln!("readline error: {}", err);
                break;
            }
        }
    }
    let _ = rl.save_history("~/.rak_history");
}

fn print_help() {
    println!("Commands (prefixed with :):");
    println!("  :help          show this help");
    println!("  :history       show input history");
    println!("  :clear         reset interpreter state");
    println!("  :vars          show all defined variables");
    println!("  :ast <expr>    print the AST of an expression");
    println!("  :bytecode <expr>  print the compiled bytecode");
    println!("  :trace <expr>  print the execution trace");
    println!("  :quit / :q     exit");
    println!();
    println!("Any other line is evaluated as Rak source.");
    println!("Multi-line: unfinished blocks continue on the next line.");
}

fn handle_repl_command(interp: &mut Interpreter, cmd: &str, rest: &str, buffer: &mut String) {
    match cmd {
        "vars" => {
            println!("vars command: not yet implemented (need interpreter scope dump)");
            let _ = (interp, rest);
        }
        "ast" => {
            match crate::lexer::tokenize(rest) {
                Ok(tokens) => match crate::parser::parse(&tokens, rest) {
                    Ok(ast) => println!("{:#?}", ast),
                    Err(e) => println!("parse error: {}", e),
                },
                Err(e) => println!("lex error: {}", e),
            }
        }
        "bytecode" => {
            match crate::lexer::tokenize(rest) {
                Ok(tokens) => match crate::parser::parse(&tokens, rest) {
                    Ok(ast) => match crate::compiler::compile_module(&ast) {
                        Ok(chunk) => print_chunk(&chunk),
                        Err(e) => println!("compile error: {}", e),
                    },
                    Err(e) => println!("parse error: {}", e),
                },
                Err(e) => println!("lex error: {}", e),
            }
        }
        "trace" => {
            println!("trace command: not yet implemented");
            let _ = (interp, rest, buffer);
        }
        _ => {
            println!("unknown command: :{}", cmd);
            println!("type :help for a list");
        }
    }
}

fn print_chunk(chunk: &crate::bytecode::Chunk) {
    println!("constants: {:?}", chunk.constants);
    println!("code: {:?}", chunk.code);
    println!("lines: {:?}", chunk.lines);
}

fn is_complete(src: &str) -> bool {
    let mut brace_depth: i32 = 0;
    let mut paren_depth: i32 = 0;
    let mut bracket_depth: i32 = 0;
    for c in src.chars() {
        match c {
            '{' => brace_depth += 1,
            '}' => brace_depth -= 1,
            '(' => paren_depth += 1,
            ')' => paren_depth -= 1,
            '[' => bracket_depth += 1,
            ']' => bracket_depth -= 1,
            _ => {}
        }
    }
    brace_depth <= 0 && paren_depth <= 0 && bracket_depth <= 0
}