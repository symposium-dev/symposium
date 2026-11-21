#!/usr/bin/env sh
set -eu
fixture=$(dirname "$0")
printf '%s\n' "$1" >> "$fixture/attempts"
if [ ! -f "$fixture/succeed-on" ]; then
    printf '%s\n' 'simulated replacement failure' >&2
    exit 23
fi
succeed=$(cat "$fixture/succeed-on")
if [ "$succeed" != both ] && [ "$succeed" != "$1" ]; then
    printf '%s\n' 'simulated replacement failure' >&2
    exit 23
fi
if [ -f "$fixture/omit-binary" ]; then
    exit 0
fi
while [ "$#" -gt 0 ]; do
    if [ "$1" = --root ]; then
        shift
        mkdir -p "$1/bin"
        cp "$fixture/payload" "$1/bin/cached-tool"
        exit 0
    fi
    shift
done
exit 23
