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
# matter) and are reversible with --unapply. Changing the profile set:
# reverse everything first, then apply the new set.
set -eu
cd "$(dirname "$0")"

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
        patch -p1 --forward < "profiles/$p.patch"
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
        patch -R -p1 --forward < "profiles/$p.patch"
    done
    echo "profiles reversed; tree is back to stock, no build run."
fi
