# Standalone copy of the release workflow's "Pin the IDE version" step, kept here
# so the logic can be exercised on a throwaway tree instead of only discovering it
# is wrong when a release is already public.
set -euo pipefail

# Guard rather than letting `set -u` report "unbound variable", which says
# nothing about the actual mistake: a tag that never reached the environment.
if [ -z "${TAG:-}" ]; then
  echo "::error::TAG is not set. This script rewrites the IDE version from the"
  echo "::error::release tag; run it as: TAG=v8.1.1 scripts/pin-ide-version.sh"
  exit 1
fi

VERSION="${TAG#v}"
echo "IDE version: $VERSION"

# No `$` anchor on these patterns. sed treats the carriage return of a CRLF line
# ending as ordinary content, so a pattern that insists the line ends with a
# quote silently matches nothing on a CRLF file -- which is what
# ide/src-tauri/Cargo.toml is, and why the first version of this script reported
# success on an LF test fixture and then failed on the real file. Matching from
# the start of the line and stopping at the closing quote works for both.
sed -i -E "s/^(version[[:space:]]*=[[:space:]]*\")[^\"]*(\")/\1$VERSION\2/" \
  ide/src-tauri/Cargo.toml
sed -i -E "s/(\"version\"[[:space:]]*:[[:space:]]*\")[^\"]*(\")/\1$VERSION\2/" \
  ide/src-tauri/tauri.conf.json ide/package.json

echo "--- resulting versions ---"
grep -H -m1 -E '[0-9]+\.[0-9]+\.[0-9]+' \
  ide/src-tauri/Cargo.toml ide/src-tauri/tauri.conf.json ide/package.json

for f in ide/src-tauri/Cargo.toml ide/src-tauri/tauri.conf.json ide/package.json; do
  grep -q "$VERSION" "$f" || {
    echo "::error::$f did not pick up $VERSION"
    exit 1
  }
done
