#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Starts the app binary once on a headless runner to catch load-time
# crashes — a shared lib's static initializer or a missing NEEDED lib kills
# the process before main (v0.4.1 shipped such a SIGSEGV). Without a display
# a healthy binary gets as far as GTK init and exits with Tauri's panic
# (101); a signal (exit > 128) or a loader error (126/127) fails.
#
# usage: scripts/smoke-start.sh <binary> <dir with libllama/libggml>
set -u
binary=$1
libdir=$2

env -u DISPLAY -u WAYLAND_DISPLAY LD_LIBRARY_PATH="$libdir" timeout 20 "$binary"
code=$?
echo "smoke-start: $binary exited with $code"
if [ "$code" -ge 126 ] && [ "$code" -ne 124 ]; then
  if [ "$code" -gt 128 ]; then
    echo "::error::$binary was killed by signal $((code - 128)) while starting"
  else
    echo "::error::$binary could not be loaded (exit $code)"
  fi
  exit 1
fi
