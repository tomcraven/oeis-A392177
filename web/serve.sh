#!/usr/bin/env bash
# Builds the wasm bundle, spins up a local HTTP server, and opens it in the browser.
#
# Usage:
#   ./web/serve.sh
#   PROFILE=wasm-release ./web/serve.sh
#   WASM_OPT_LEVEL=s ./web/serve.sh
#
# Env:
#   PROFILE          Cargo profile (default: wasm-release-fast).
#   WASM_OPT_LEVEL   If set to s, selects wasm-release unless PROFILE is set.
#   PORT             HTTP port (default 8000).

set -euo pipefail

WEB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${WEB_DIR}/.." && pwd)"
HOST="127.0.0.1"
PORT="${PORT:-8000}"

if [ -n "${PROFILE+x}" ]; then
    :
elif [ "${WASM_OPT_LEVEL:-}" = "s" ]; then
    PROFILE="wasm-release"
else
    PROFILE="wasm-release-fast"
fi
CRATE_NAME="red_black_knights"
TARGET="wasm32-unknown-unknown"

require() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "error: '$1' is not installed. $2" >&2
        exit 1
    fi
}

require cargo "install rust from https://rustup.rs"
require wasm-bindgen "install with: cargo install -f wasm-bindgen-cli"

if ! rustup target list --installed 2>/dev/null | grep -q "^${TARGET}$"; then
    echo "adding rust target ${TARGET}..."
    rustup target add "${TARGET}"
fi

echo "building ${CRATE_NAME} (${PROFILE}) for ${TARGET}..."
(cd "${ROOT_DIR}" && cargo build --profile "${PROFILE}" --target "${TARGET}")

case "${PROFILE}" in
    dev) PROFILE_DIR="debug" ;;
    *)   PROFILE_DIR="${PROFILE}" ;;
esac
WASM_IN="${ROOT_DIR}/target/${TARGET}/${PROFILE_DIR}/${CRATE_NAME}.wasm"

echo "running wasm-bindgen..."
wasm-bindgen \
    --target web \
    --out-dir "${WEB_DIR}" \
    --out-name "${CRATE_NAME}" \
    "${WASM_IN}"

ASSETS_LINK="${WEB_DIR}/assets"
ASSETS_SRC="${ROOT_DIR}/assets"
if [ -d "${ASSETS_SRC}" ]; then
    if [ -L "${ASSETS_LINK}" ] || [ ! -e "${ASSETS_LINK}" ]; then
        ln -sfn "${ASSETS_SRC}" "${ASSETS_LINK}"
    elif [ ! -d "${ASSETS_LINK}" ]; then
        echo "warning: ${ASSETS_LINK} exists and is not a directory or symlink; not touching it" >&2
    fi
else
    echo "warning: no assets/ directory at ${ASSETS_SRC}" >&2
fi

is_port_free() {
    ! lsof -iTCP:"$1" -sTCP:LISTEN -n -P >/dev/null 2>&1
}

while ! is_port_free "$PORT"; do
    echo "port $PORT is in use, trying $((PORT + 1))..."
    PORT=$((PORT + 1))
    if [ "$PORT" -gt 8100 ]; then
        echo "no free port found in 8000-8100" >&2
        exit 1
    fi
done

URL="http://${HOST}:${PORT}/"
echo "serving ${WEB_DIR} at ${URL}"
echo "press ctrl-c to stop"

python3 -c '
import http.server, socketserver, sys
host, port, directory = sys.argv[1], int(sys.argv[2]), sys.argv[3]
class H(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=directory, **kw)
    def end_headers(self):
        self.send_header("Cache-Control", "no-store, max-age=0")
        self.send_header("Pragma", "no-cache")
        super().end_headers()
socketserver.TCPServer.allow_reuse_address = True
with socketserver.TCPServer((host, port), H) as s:
    s.serve_forever()
' "$HOST" "$PORT" "$WEB_DIR" &
SERVER_PID=$!

cleanup() {
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

for _ in $(seq 1 50); do
    if curl -s -o /dev/null "$URL"; then
        break
    fi
    sleep 0.1
done

case "$(uname -s)" in
    Darwin) open "$URL" ;;
    Linux)  xdg-open "$URL" >/dev/null 2>&1 || true ;;
    MINGW*|MSYS*|CYGWIN*) start "$URL" ;;
esac

wait "$SERVER_PID"
