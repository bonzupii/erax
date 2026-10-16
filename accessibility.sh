#!/bin/sh
# accessibility.sh — apply erax accessibility profiles and build.
#
# Usage:
#   ./accessibility.sh <profile>...          apply profiles, then release-build
#   ./accessibility.sh --unapply <profile>...  reverse profiles (no build)
#   ./accessibility.sh --list                list available profiles
#
# Profiles are .patch files in profiles/, one per disability class.
# They compose by class (disjoint ground; application order does not
# matter) and are reversible with --unapply. Applying an already
# applied profile is a no-op; a patch that applies neither forward
# nor reversed means the config drifted, and the script aborts loudly
# rather than leaving .rej files behind. Changing the profile set:
# reverse everything first, then apply the new set.
set -eu
cd "$(dirname "$0")"

# apply_patch <patchfile>: apply unless already applied.
apply_patch() {
    if patch -p1 --forward --dry-run --silent < "$1" >/dev/null 2>&1; then
        patch -p1 --forward < "$1"
    elif patch -p1 -R --dry-run --silent < "$1" >/dev/null 2>&1; then
        echo "already applied, skipping: $1" >&2
    else
        echo "patch does not apply cleanly (config drift?): $1" >&2
        exit 1
    fi
}

# reverse_patch <patchfile>: reverse unless already reversed.
reverse_patch() {
    if patch -p1 -R --forward --dry-run --silent < "$1" >/dev/null 2>&1; then
        patch -R -p1 --forward < "$1"
    elif patch -p1 --forward --dry-run --silent < "$1" >/dev/null 2>&1; then
        echo "already reversed, skipping: $1" >&2
    else
        echo "patch does not reverse cleanly (config drift?): $1" >&2
        exit 1
    fi
}

usage() {
    echo "usage: ./accessibility.sh <profile>... | ./accessibility.sh --unapply <profile>... | ./accessibility.sh --list" >&2
}

APPLY=1
PROFILES=""
while [ $# -gt 0 ]; do
    case "$1" in
        --unapply) APPLY=0; shift ;;
        --list)
            for f in profiles/*.patch; do basename "$f" .patch; done
            exit 0 ;;
        -h|--help) usage; exit 0 ;;
        -*) echo "unknown option: $1" >&2; usage; exit 1 ;;
        *) PROFILES="$PROFILES $1"; shift ;;
    esac
done

if [ -z "$PROFILES" ]; then usage; exit 1; fi

if [ "$APPLY" = 1 ]; then
    EXTRA_FLAGS=""
    for p in $PROFILES; do
        if [ ! -f "profiles/$p.patch" ]; then
            echo "unknown profile: $p (see ./accessibility.sh --list)" >&2
            exit 1
        fi
        apply_patch "profiles/$p.patch"
        if [ -f "profiles/$p.flags" ]; then
            EXTRA_FLAGS="$EXTRA_FLAGS $(cat "profiles/$p.flags")"
        fi
    done
    # shellcheck disable=SC2086
    cargo build --release $EXTRA_FLAGS
else
    REV=""
    for p in $PROFILES; do REV="$p $REV"; done
    for p in $REV; do
        if [ ! -f "profiles/$p.patch" ]; then
            echo "unknown profile: $p (see ./accessibility.sh --list)" >&2
            exit 1
        fi
        reverse_patch "profiles/$p.patch"
    done
    echo "profiles reversed; tree is back to stock, no build run."
fi
