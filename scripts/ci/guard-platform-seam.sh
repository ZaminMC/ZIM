#!/usr/bin/env bash
# Platform-seam guard (ADR-0008).
#
# OS-conditional code may exist only in crates/zamin-core/src/platform and
# crates/zamin-ipc. Process spawning may only happen in those same areas.
# Test code is exempt from the cfg check: platform-conditional tests exist to
# verify platform-bound behavior (TESTING.md) and run in their matching CI
# lane. Anything else fails the build.

set -u

hits=0

fail() {
    echo "platform-seam guard: $1" >&2
    hits=$((hits + 1))
}

# 1. cfg(windows) / cfg(unix) / cfg(target_os) outside the platform seam.
#    Production src trees only — tests are deliberately exempt (see header).
cfg_hits=$(grep -rnE '#\[cfg\((windows|unix|target_os)' \
    --include='*.rs' crates/*/src crates/testing/*/src apps/*/src 2>/dev/null \
    | grep -v '^crates/zamin-core/src/platform/' \
    | grep -v '^crates/zamin-ipc/src/' \
    || true)
if [ -n "$cfg_hits" ]; then
    echo "OS-conditional code outside the platform seam:" >&2
    echo "$cfg_hits" >&2
    hits=$((hits + $(echo "$cfg_hits" | wc -l)))
fi

# 2. Process spawning outside the platform seam (and tests, which spawn
#    the fake server through the supervisor, not directly).
spawn_hits=$(grep -rnE '(std::process::Command|process::Command)::new' \
    --include='*.rs' crates/*/src crates/testing/*/src 2>/dev/null \
    | grep -v '^crates/zamin-core/src/platform/' \
    | grep -v '^crates/zamin-ipc/src/' \
    || true)
if [ -n "$spawn_hits" ]; then
    echo "process spawning outside the platform seam:" >&2
    echo "$spawn_hits" >&2
    hits=$((hits + $(echo "$spawn_hits" | wc -l)))
fi

if [ "$hits" -gt 0 ]; then
    echo "platform-seam guard: $hits violation(s)" >&2
    exit 1
fi
echo "platform-seam guard: ok"
