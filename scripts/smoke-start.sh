#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Starts the app binary once on a headless runner to catch load-time
# crashes — a shared lib's static initializer or a missing NEEDED lib kills
# the process before main (v0.4.1 shipped such a SIGSEGV). Without a display
# a healthy binary gets as far as GTK init and panics there. That panic is
# the success marker: it exits with 101 in debug builds but aborts (134)
# under the release profile's `panic = "abort"`, so the exit code alone
# cannot tell a healthy release binary from a crash.
#
# usage: scripts/smoke-start.sh <binary> <dir with libllama/libggml>
set -u
binary=$1
libdir=$2

out=$(env -u DISPLAY -u WAYLAND_DISPLAY LD_LIBRARY_PATH="$libdir" \
  timeout 20 "$binary" 2>&1)
code=$?
printf '%s\n' "$out" | tail -n 15
echo "smoke-start: $binary exited with $code"
if [ "$code" -eq 124 ] || grep -q "Failed to initialize gtk backend" <<<"$out"; then
  echo "smoke-start: reached GTK init — OK"
  exit 0
fi
echo "::error::$binary did not reach GTK init (exit $code)"
exit 1
