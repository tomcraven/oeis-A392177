#!/usr/bin/env bash
# Build wasm, run wasm-bindgen, copy assets from itch-assets.manifest, zip for itch.io.
#
# Usage:
#   ./web/package-itch.sh
#   PROFILE=wasm-release ./web/package-itch.sh

set -euo pipefail

WEB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${WEB_DIR}/.." && pwd)"
MANIFEST="${WEB_DIR}/itch-assets.manifest"
ASSETS_SRC="${ROOT_DIR}/assets"
CRATE_NAME="red_black_knights"
TARGET="wasm32-unknown-unknown"
OUT_DIR="${ROOT_DIR}/dist"
OUT_ZIP="${OUT_DIR}/red_black_knights-itch.zip"

PROFILE="${PROFILE:-wasm-release-fast}"

require() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "error: '$1' is not installed. $2" >&2
        exit 1
    fi
}

require cargo "install rust from https://rustup.rs"
require wasm-bindgen "install with: cargo install -f wasm-bindgen-cli"
require zip "install zip (e.g. brew install zip)"

if [ ! -f "${MANIFEST}" ]; then
    echo "error: missing ${MANIFEST}" >&2
    exit 1
fi

if ! rustup target list --installed 2>/dev/null | grep -q "^${TARGET}$"; then
    echo "adding rust target ${TARGET}..."
    rustup target add "${TARGET}"
fi

echo "building ${CRATE_NAME} (${PROFILE}) for ${TARGET}..."
(cd "${ROOT_DIR}" && cargo build --profile "${PROFILE}" --target "${TARGET}")

case "${PROFILE}" in
    dev) PROFILE_DIR="debug" ;;
    *) PROFILE_DIR="${PROFILE}" ;;
esac
WASM_IN="${ROOT_DIR}/target/${TARGET}/${PROFILE_DIR}/${CRATE_NAME}.wasm"

STAGE="$(mktemp -d "${TMPDIR:-/tmp}/red-black-knights-itch.XXXXXX")"
cleanup() {
    rm -rf "${STAGE}"
}
trap cleanup EXIT

echo "running wasm-bindgen → ${STAGE}..."
wasm-bindgen \
    --target web \
    --out-dir "${STAGE}" \
    --out-name "${CRATE_NAME}" \
    "${WASM_IN}"

cp "${WEB_DIR}/index.html" "${STAGE}/index.html"

if [ -d "${ASSETS_SRC}" ]; then
    echo "copying assets from ${MANIFEST}..."
    mkdir -p "${STAGE}/assets"

    while IFS= read -r raw || [ -n "${raw}" ]; do
        line="${raw#"${raw%%[![:space:]]*}"}"
        line="${line%"${line##*[![:space:]]}"}"
        [ -z "${line}" ] && continue
        [[ "${line}" == \#* ]] && continue

        if [[ "${line}" == dir:* ]]; then
            sub="${line#dir:}"
            sub="${sub#"${sub%%[![:space:]]*}"}"
            sub="${sub%"${sub##*[![:space:]]}"}"
            sub="${sub#/}"
            sub="${sub%/}"
            src="${ASSETS_SRC}/${sub}"
            if [ ! -d "${src}" ]; then
                echo "error: manifest dir: missing directory ${src}" >&2
                exit 1
            fi
            mkdir -p "${STAGE}/assets/${sub}"
            cp -R "${src}/." "${STAGE}/assets/${sub}/"
        else
            rel="${line}"
            src="${ASSETS_SRC}/${rel}"
            if [ ! -f "${src}" ]; then
                echo "error: manifest file missing: ${src}" >&2
                exit 1
            fi
            d="$(dirname "${rel}")"
            mkdir -p "${STAGE}/assets/${d}"
            cp "${src}" "${STAGE}/assets/${rel}"
        fi
    done < "${MANIFEST}"
fi

mkdir -p "${OUT_DIR}"
rm -f "${OUT_ZIP}"
echo "zipping → ${OUT_ZIP}..."
( cd "${STAGE}" && zip -r -q "${OUT_ZIP}" . )

BYTES="$(wc -c <"${OUT_ZIP}" | tr -d ' ')"
echo "done: ${OUT_ZIP} ($(awk -v b="${BYTES}" 'BEGIN { printf "%.2f MiB\n", b/1024/1024 }'))"
