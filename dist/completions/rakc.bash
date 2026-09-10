# bash completion for rakc
_rakc_completions() {
    local cur="${COMP_WORDS[COMP_CWORD]}"
    local cmds="run vm build bench check lex parse repl lsp bindgen --version --help"
    if [ "$COMP_CWORD" -eq 1 ]; then
        COMPREPLY=( $(compgen -W "$cmds" -- "$cur") )
    else
        case "${COMP_WORDS[1]}" in
            run|vm|build|bench|check|lex|parse|bindgen)
                COMPREPLY=( $(compgen -f -- "$cur") )
                ;;
            *)
                COMPREPLY=()
                ;;
        esac
    fi
}
complete -F _rakc_completions rakc
