#compdef oyvey

_oyvey() {
    local -a cmds
    cmds=(
        'new:Generate a new Rak project'
        'init:Initialize an existing directory as a project'
        'add:Add a GitHub package dependency'
        'remove:Remove a dependency'
        'install:Resolve and install dependencies'
        'update:Re-resolve dependencies within constraints'
        'build:Compile the project through rakc'
        'run:Run the project entry point'
        "test:Run the project's tests"
        'clean:Remove build artifacts'
        'list:List installed packages'
        'tree:Print the dependency tree'
        'audit:Verify installed packages against the lockfile'
        'lock:Write oyvey.lock without installing'
        'version:Print version'
        'help:Show help'
    )
    if (( CURRENT == 2 )); then
        _describe 'command' cmds
        return 0
    fi
    case ${words[2]} in
        new)
            _files -/
            ;;
        init)
            _arguments '--lib[generate a library project]'
            ;;
        add)
            _message 'dependency spec (user/repo[@version][#rev])'
            ;;
        remove)
            local -a packages
            packages=(${(f)"$(cd -P ./packages 2>/dev/null && ls -1 2>/dev/null)"})
            _describe 'package' packages
            ;;
        help)
            _describe 'command' cmds
            ;;
    esac
}

_oyvey "$@"
