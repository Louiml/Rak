# bash completion for oyvey
_oyvey_completions() {
    local cur="${COMP_WORDS[COMP_CWORD]}"
    local prev="${COMP_WORDS[COMP_CWORD-1]}"
    local cmds="new init add remove install update build run test clean list tree audit lock version help"
    local flags="--help -h --version -V"

    # `oyvey remove <package>` completes the vendored package names.
    if [ "$COMP_CWORD" -eq 2 ] && [ "$prev" = "remove" ]; then
        local packages=""
        local dir
        for dir in ./packages ../packages ../../packages; do
            if [ -d "$dir" ]; then
                packages="$packages $(ls -1 "$dir" 2>/dev/null)"
            fi
        done
        COMPREPLY=( $(compgen -W "$packages" -- "$cur") )
        return 0
    fi

    if [ "$COMP_CWORD" -eq 1 ]; then
        COMPREPLY=( $(compgen -W "$cmds $flags" -- "$cur") )
    else
        case "${COMP_WORDS[1]}" in
            new)
                COMPREPLY=( $(compgen -f -- "$cur") $(compgen -W "--lib" -- "$cur") )
                ;;
            init)
                COMPREPLY=( $(compgen -W "--lib $flags" -- "$cur") )
                ;;
            help)
                COMPREPLY=( $(compgen -W "$cmds" -- "$cur") )
                ;;
            *)
                COMPREPLY=()
                ;;
        esac
    fi
}
complete -F _oyvey_completions oyvey
