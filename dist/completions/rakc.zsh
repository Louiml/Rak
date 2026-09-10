#compdef rakc

_rakc() {
    local -a cmds
    cmds=(
        'run:Run a Rak script on the interpreter'
        'vm:Run on the bytecode VM'
        'build:Build a standalone executable'
        'bench:Benchmark interpreter vs VM'
        'check:Lex and parse, print diagnostics'
        'lex:Print tokens'
        'parse:Print the AST'
        'repl:Start an interactive REPL'
        'lsp:Start the language server (stdio)'
        'bindgen:Generate Rak bindings from a C header'
    )
    if (( CURRENT == 2 )); then
        _describe 'command' cmds
    else
        case ${words[2]} in
            run|vm|build|bench|check|lex|parse|bindgen)
                _files
                ;;
        esac
    fi
}

_rakc "$@"
