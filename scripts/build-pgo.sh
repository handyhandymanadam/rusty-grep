#!/bin/bash
# Build rg with profile-guided optimization (PGO), AVX2-level CPU target and fat LTO.
#
#   scripts/build-pgo.sh [OUTPUT_PATH]      (default: target-pgo/release-lto/rg)
#
# Measured on a Ryzen 7800X3D machine, 2026-10-03, vs a plain `cargo build --release`:
# large-file searches 5-28% faster (-v 0.75x, print-all 0.72x, dense -n 0.77x,
# -i 0.89x); tree searches within noise (they are limited by system calls).
# The profile must be regenerated whenever the code changes, so this script always
# retrains. Needs llvm-profdata from the same LLVM major version as rustc.
#
# Training data: any large text file plus a source tree. Set TRAIN_FILE and TRAIN_TREE
# (defaults below are the ones used for the numbers above).
set -euo pipefail
cd "$(dirname "$0")/.."
OUT=${1:-target-pgo/release-lto/rg}
TRAIN_FILE=${TRAIN_FILE:-$HOME/work/ripgrep-fork-data/big.txt}
TRAIN_TREE=${TRAIN_TREE:-$HOME/work/ripgrep-fork-data/linux}
WORDS=${WORDS:-$HOME/work/ripgrep-fork-data/w400.txt}
PROF=${KEEP_PROFILE:-$(mktemp -d)}
mkdir -p "$PROF"
[ -n "${KEEP_PROFILE:-}" ] || trap 'rm -rf "$PROF"' EXIT   # KEEP_PROFILE=DIR keeps the profile data
FEATURES=${FEATURES:-}            # e.g. FEATURES=pcre2 to include the PCRE2 engine (-P)
CPU="-C target-cpu=x86-64-v3"   # AVX2: runs on common x86-64 machines; "native" was not faster

echo "== 1/3 instrumented build"
RUSTFLAGS="$CPU -C profile-generate=$PROF" CARGO_TARGET_DIR=target-pgo-gen \
    cargo build --profile release-lto ${FEATURES:+--features $FEATURES} 2>&1 | tail -1
G=target-pgo-gen/release-lto/rg

echo "== 2/3 training run"
for _ in 1 2; do
    $G TODO "$TRAIN_TREE" | wc -l >/dev/null
    $G '\w+_lock\(' "$TRAIN_TREE" | wc -l >/dev/null
    $G -i suspend "$TRAIN_TREE" | wc -l >/dev/null
    $G -l EXPORT_SYMBOL "$TRAIN_TREE" | wc -l >/dev/null
    $G --files "$TRAIN_TREE" | wc -l >/dev/null
    $G -C2 TODO "$TRAIN_TREE" | wc -l >/dev/null
    $G static "$TRAIN_FILE" | wc -l >/dev/null
    $G '[A-Z]+_[A-Z]+\(' "$TRAIN_FILE" | wc -l >/dev/null
    $G -i return "$TRAIN_FILE" | wc -l >/dev/null
    $G -c include "$TRAIN_FILE" >/dev/null
    $G -v e "$TRAIN_FILE" | wc -l >/dev/null
    $G '' "$TRAIN_FILE" | wc -l >/dev/null
    $G -n e "$TRAIN_FILE" | wc -l >/dev/null
    $G -w size_t "$TRAIN_FILE" | wc -l >/dev/null
    $G --all-of -e static -e inline "$TRAIN_FILE" | wc -l >/dev/null
    [ -f "$WORDS" ] && $G -F -f "$WORDS" "$TRAIN_FILE" | wc -l >/dev/null
    case "$FEATURES" in *pcre2*) $G -P '(\w+) \1' "$TRAIN_FILE" | wc -l >/dev/null ;; esac
done 2>/dev/null
llvm-profdata merge -o "$PROF/merged.profdata" "$PROF"/*.profraw

echo "== 3/3 optimized build"
RUSTFLAGS="$CPU -C profile-use=$PROF/merged.profdata" CARGO_TARGET_DIR=target-pgo \
    cargo build --profile release-lto ${FEATURES:+--features $FEATURES} 2>&1 | tail -1
[ "$OUT" = target-pgo/release-lto/rg ] || install -m 755 target-pgo/release-lto/rg "$OUT"
echo "built: $OUT"
