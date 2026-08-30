#!/usr/bin/env bash
# Normalize TAURI_SIGNING_PRIVATE_KEY into the official `tauri signer generate`
# format: one line of base64 wrapping a two-line minisign secret.
#
# `tauri build` treats TAURI_SIGNING_PRIVATE_KEY as a path if that path exists,
# otherwise as the key string. In both cases the contents must be the generate
# format. A decoded two-line minisign file fails with:
#   failed to decode base64 secret key: Invalid symbol 32
#
# Re-encoding a malformed file, or handing Tauri a second unexpected wrap,
# fails later as:
#   incorrect updater private key password: failed to fill whole buffer
#
# Usage: prepare-tauri-signing-key.sh <output-path>
# Reads the secret from TAURI_SIGNING_PRIVATE_KEY.

set -euo pipefail

OUT="${1:-}"
if [ -z "$OUT" ]; then
  echo "Usage: $0 <output-path>" >&2
  exit 2
fi

RAW="${TAURI_SIGNING_PRIVATE_KEY:-}"
if [ -z "$RAW" ]; then
  echo "❌ TAURI_SIGNING_PRIVATE_KEY is empty" >&2
  exit 1
fi

RAW="${RAW//$'\r'/}"

is_minisign_secret() {
  printf '%s\n' "$1" | head -n1 | grep -q '^untrusted comment:'
}

encode_official() {
  # Official generate format is base64(two-line minisign secret) without wraps.
  if command -v openssl >/dev/null 2>&1; then
    printf '%s' "$1" | openssl base64 -A
    return
  fi
  if base64 -w0 </dev/null >/dev/null 2>&1; then
    printf '%s' "$1" | base64 -w0
    return
  fi
  printf '%s' "$1" | base64 | tr -d '\r\n'
}

write_official() {
  local official="$1"
  official=$(printf '%s' "$official" | tr -d '\r\n\t ')
  if [ -z "$official" ]; then
    echo "❌ normalized signing key was empty" >&2
    exit 1
  fi
  printf '%s' "$official" > "$OUT"
}

decode_b64() {
  printf '%s' "$1" | tr -d '\n\t ' | (
    base64 --decode 2>/dev/null ||
      base64 -D 2>/dev/null ||
      openssl base64 -d -A 2>/dev/null
  )
}

if is_minisign_secret "$RAW"; then
  TWO=$(printf '%s' "$RAW")
  TWO="${TWO%"${TWO##*[![:space:]]}"}"
  TWO="${TWO}"$'\n'
  write_official "$(encode_official "$TWO")"
  echo "normalized: raw-minisign-to-generate-format"
  exit 0
fi

if DECODED=$(decode_b64 "$RAW") && is_minisign_secret "$DECODED"; then
  # Already the official generate format (or an equivalent wrap). Keep the
  # compact single line; do not decode to a two-line file for Tauri to read.
  write_official "$RAW"
  echo "normalized: generate-format"
  exit 0
fi

ONE=$(printf '%s' "$RAW" | tr -d '\n\t ')
if echo "$ONE" | grep -Eq '^[A-Za-z0-9+/=]+$' && [ "${#ONE}" -ge 64 ]; then
  TWO=$(printf '%s\n%s' "untrusted comment: tauri signing key" "$ONE")
  TWO="${TWO}"$'\n'
  write_official "$(encode_official "$TWO")"
  echo "normalized: single-line-secret-to-generate-format"
  exit 0
fi

echo "❌ TAURI_SIGNING_PRIVATE_KEY 格式无法识别：既不是两行原文，也不是其 base64，亦非一行 base64" >&2
exit 1
