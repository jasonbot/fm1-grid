#!/usr/bin/env bash
# Cross-build an fm1 app for the DevTerm A04 (aarch64, Cortex-A53) on this machine
# and install it in ~/bin on the device.
#
#   ./scripts/deploy-devterm.sh                  fm1-grid: build, install, Xvfb smoke
#   ./scripts/deploy-devterm.sh fm1-tty          fm1-tty: build, install, pty smoke
#   ./scripts/deploy-devterm.sh fm1-tty --no-smoke
#
# Env overrides: DEVTERM_HOST (devterm-a04), DEVTERM_USER (devterm),
# DEVTERM_PASS (unset: use SSH keys), DEVTERM_BUILDER (fm1-grid-builder).
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
HOST=${DEVTERM_HOST:-devterm-a04}
REMOTE_USER=${DEVTERM_USER:-devterm}
PASS=${DEVTERM_PASS:-}
IMAGE=${DEVTERM_BUILDER:-fm1-grid-builder}
APP=fm1-grid
SMOKE=1
for arg in "$@"; do
    case $arg in
        fm1-grid|fm1-tty) APP=$arg ;;
        --no-smoke) SMOKE=0 ;;
        -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
        *) echo "unknown argument: $arg" >&2; exit 2 ;;
    esac
done
BIN=target/aarch64-unknown-linux-gnu/release/$APP

command -v podman >/dev/null || { echo "podman is required" >&2; exit 1; }
SSH=(ssh -o StrictHostKeyChecking=accept-new -o ConnectTimeout=10)
SCP=(scp -o StrictHostKeyChecking=accept-new)
if [[ -n $PASS ]]; then
    command -v sshpass >/dev/null || { echo "sshpass is required for DEVTERM_PASS" >&2; exit 1; }
    SSH=(sshpass -p "$PASS" "${SSH[@]}")
    SCP=(sshpass -p "$PASS" "${SCP[@]}")
fi
REMOTE=$REMOTE_USER@$HOST

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

cat >"$WORK/Dockerfile" <<'DOCKER'
FROM --platform=linux/amd64 docker.io/library/debian:trixie
ENV DEBIAN_FRONTEND=noninteractive
RUN dpkg --add-architecture arm64 \
    && sed -i '/^Suites:/a Architectures: amd64 arm64' /etc/apt/sources.list.d/debian.sources \
    && apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential ca-certificates curl crossbuild-essential-arm64 git pkgconf \
        libasound2-dev:arm64 libfontconfig1-dev:arm64 libfreetype-dev:arm64 \
        libwayland-dev:arm64 libx11-dev:arm64 libxkbcommon-dev:arm64 \
        libxkbcommon-x11-dev:arm64 \
    && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
ENV CARGO_NET_GIT_FETCH_WITH_CLI=true
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal \
        --default-toolchain 1.99.1 --target aarch64-unknown-linux-gnu \
    && chmod -R a+rwX /opt/cargo /opt/rustup
DOCKER

echo "==> building cross toolchain image ($IMAGE)"
podman build --arch amd64 -t "$IMAGE" -f "$WORK/Dockerfile" "$WORK"

echo "==> compiling $APP for aarch64 (release, Cortex-A53)"
podman run --rm --userns=keep-id --user "$(id -u):$(id -g)" \
    -v "$ROOT:/src:z" -w /src -e HOME=/tmp \
    -e CARGO_BUILD_TARGET=aarch64-unknown-linux-gnu \
    -e CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    -e CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS='-C target-cpu=cortex-a53' \
    -e CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
    -e CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++ \
    -e AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar \
    -e PKG_CONFIG=pkg-config -e PKG_CONFIG_ALLOW_CROSS=1 \
    -e PKG_CONFIG_LIBDIR=/usr/lib/aarch64-linux-gnu/pkgconfig:/usr/share/pkgconfig \
    -e CARGO_TERM_COLOR=never \
    "$IMAGE" cargo build --release -p "$APP"

echo "==> installing $BIN to $REMOTE:~/bin"
"${SSH[@]}" "$REMOTE" 'mkdir -p ~/bin'
# Rename over the old file: replacing a running binary in place fails with ETXTBSY.
"${SCP[@]}" "$ROOT/$BIN" "$REMOTE:bin/.$APP.new"
"${SSH[@]}" "$REMOTE" "mv -f ~/bin/.$APP.new ~/bin/$APP"

echo "==> checking device runtime dependencies"
"${SSH[@]}" "$REMOTE" "APP=$(printf %q "$APP") SUDO_PASS=$(printf %q "$PASS") bash -s" <<'REMOTE'
set -euo pipefail
if [[ -n ${SUDO_PASS:-} ]]; then
    sudo() { printf '%s\n' "$SUDO_PASS" | command sudo -S -p '' "$@"; }
fi
apt_install() {
    sudo apt-get install -y --no-install-recommends "$@" \
        || sudo apt-get install -y --no-install-recommends -t trixie-backports "$@"
}
case $APP in
    fm1-grid) pkgs=(libxkbcommon-x11-0 mesa-vulkan-drivers) ;;
    *) pkgs=() ;;
esac
missing=()
for pkg in "${pkgs[@]}"; do
    dpkg -s "$pkg" >/dev/null 2>&1 || missing+=("$pkg")
done
if ((${#missing[@]})); then
    echo "installing: ${missing[*]}"
    apt_install "${missing[@]}"
fi
if ldd ~/bin/"$APP" | grep -q 'not found'; then
    echo "unresolved libraries:" >&2
    ldd ~/bin/"$APP" | grep 'not found' >&2
    exit 1
fi
REMOTE

if ((SMOKE)); then
    if [[ $APP == fm1-grid ]]; then
        echo "==> smoke test under Xvfb"
        "${SSH[@]}" "$REMOTE" "SUDO_PASS=$(printf %q "$PASS") bash -s" <<'REMOTE'
set -euo pipefail
if [[ -n ${SUDO_PASS:-} ]]; then
    sudo() { printf '%s\n' "$SUDO_PASS" | command sudo -S -p '' "$@"; }
fi
apt_install() {
    sudo apt-get install -y --no-install-recommends "$@" \
        || sudo apt-get install -y --no-install-recommends -t trixie-backports "$@"
}
for pkg in xvfb x11-utils; do
    dpkg -s "$pkg" >/dev/null 2>&1 || need=1
done
if [[ -n ${need:-} ]]; then
    apt_install xvfb x11-utils
fi
export DISPLAY=:99
Xvfb :99 -screen 0 1280x480x24 -nolisten tcp >/tmp/fm1-xvfb.log 2>&1 &
xpid=$!
trap 'kill "$xpid" 2>/dev/null || true' EXIT
sleep 2
timeout 60 ~/bin/fm1-grid >/tmp/fm1-smoke.log 2>&1 &
apid=$!
found=0
for _ in $(seq 1 30); do
    if xwininfo -root -tree 2>/dev/null | grep -q '"Synth Firmware Zoo"'; then
        found=1
        break
    fi
    kill -0 "$apid" 2>/dev/null || break
    sleep 1
done
kill "$apid" 2>/dev/null || true
if ((found)); then
    echo "smoke test passed: app window opened"
else
    echo "smoke test failed; app output:" >&2
    tail -30 /tmp/fm1-smoke.log >&2
    exit 1
fi
REMOTE
    else
        echo "==> smoke test in a pty"
        "${SSH[@]}" "$REMOTE" "APP=$(printf %q "$APP") bash -s" <<'REMOTE'
set -euo pipefail
command -v script >/dev/null || { echo "script(1) is required" >&2; exit 1; }
export TERM=xterm-256color
script -qec "stty rows 24 cols 80; exec ~/bin/$APP" /dev/null >/tmp/fm1-smoke.log 2>&1 &
pid=$!
sleep 6
alive=0
kill -0 "$pid" 2>/dev/null && alive=1
kill "$pid" 2>/dev/null || true
sleep 1
pkill -f "$HOME/bin/$APP" 2>/dev/null || true
wait "$pid" 2>/dev/null || true
if ((alive)) && grep -aqE 'LIBRARY|DEVICES|KEYBOARD' /tmp/fm1-smoke.log; then
    echo "smoke test passed: $APP drew its UI"
else
    echo "smoke test failed; app output:" >&2
    tail -c 2000 /tmp/fm1-smoke.log >&2
    exit 1
fi
REMOTE
    fi
fi

echo "==> done: $REMOTE:~/bin/$APP"
