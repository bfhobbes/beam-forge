#!/usr/bin/env bash
# Checks the built-in simulator ships in each Linux bundle where the app looks
# for it (next to its own executable, see src-tauri/src/machine/launch.rs),
# and that it starts the way the app starts it: it reports its address, answers
# a GRBL status query, and exits when its stdin closes.
#
#   scripts/check-linux-sidecar.sh [bundle-dir]   (default target/release/bundle)
#
# Needs a display and a session bus; in CI run it under dbus-run-session and xvfb-run.
set -euo pipefail

bundles=$(realpath "${1:-target/release/bundle}")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

check() {
  local kind=$1 root=$2
  local bin=$root/usr/bin
  [ -x "$bin/beam-forge" ] || { echo "::error::$kind: no usr/bin/beam-forge"; exit 1; }
  [ -x "$bin/bf-simulator" ] || { echo "::error::$kind: no usr/bin/bf-simulator next to the app"; exit 1; }

  local fifo=$work/$kind.stdin out=$work/$kind.out
  mkfifo "$fifo"
  (
    if [ "$kind" = appimage ]; then
      # What the AppImage's AppRun sets up before starting the app, which the
      # simulator inherits: the bundled libraries, the GTK hook's variables, and
      # usr/ as the working directory (WebKit finds its helpers relative to it).
      export APPDIR=$root
      export LD_LIBRARY_PATH=$root/usr/lib:$root/usr/lib/x86_64-linux-gnu:${LD_LIBRARY_PATH:-}
      set +u
      for hook in "$root"/apprun-hooks/*.sh; do
        # shellcheck source=/dev/null
        source "$hook"
      done
      set -u
      cd "$root/usr"
    fi
    exec "$bin/bf-simulator" --launched-by-app < "$fifo" > "$out"
  ) &
  local pid=$!
  exec 3> "$fifo" # the app's end of stdin

  local address=""
  for _ in $(seq 100); do
    address=$(sed -n 's/^BF_SIM_LISTENING \(.*\)$/\1/p' "$out")
    [ -n "$address" ] && break
    kill -0 "$pid" 2> /dev/null || break
    sleep 0.2
  done
  [ -n "$address" ] || { echo "::error::$kind: the simulator didn't report its address"; cat "$out"; exit 1; }

  local status="" line
  exec 4<> "/dev/tcp/${address%:*}/${address##*:}"
  printf '?' >&4
  # Skip anything before the status report, such as the welcome banner.
  for _ in $(seq 5); do
    read -r -t 5 line <&4 || break
    if [[ $line == "<"* ]]; then
      status=${line%$'\r'}
      break
    fi
  done
  exec 4>&-
  [[ $status == "<"* ]] || { echo "::error::$kind: no GRBL status from $address (got '$status')"; exit 1; }

  exec 3>&- # close stdin: the simulator should exit
  local waited=0
  while kill -0 "$pid" 2> /dev/null && [ $waited -lt 600 ]; do
    sleep 0.1
    waited=$((waited + 1))
  done
  if kill -0 "$pid" 2> /dev/null; then
    echo "::error::$kind: the simulator didn't exit when its stdin closed"
    for task in /proc/"$pid"/task/*; do
      echo "$(cat "$task/comm") $(cat "$task/wchan" 2> /dev/null)"
    done
    kill -9 "$pid"
    exit 1
  fi
  echo "$kind: exited $((waited / 10)).$((waited % 10))s after stdin closed"
  echo "$kind: the simulator launched from the bundle ($address, $status)"
}

shopt -s nullglob
appimages=("$bundles"/appimage/*.AppImage)
debs=("$bundles"/deb/*.deb)
[ ${#appimages[@]} -eq 1 ] && [ ${#debs[@]} -eq 1 ] || {
  echo "::error::expected one AppImage and one .deb under $bundles"
  exit 1
}

(cd "$work" && "${appimages[0]}" --appimage-extract > /dev/null)
check appimage "$work/squashfs-root"

dpkg-deb -x "${debs[0]}" "$work/deb"
check deb "$work/deb"
