#!/usr/bin/env bash
# Normalize TAURI_SIGNING_PRIVATE_KEY into a two-line minisign secret-key file.
#
# Tauri CLI accepts either a file path or the raw two-line minisign contents.
# It does NOT accept a second base64 wrap of that file. Passing the wrapped
# blob produces:
#   failed to decode secret key: incorrect updater private key password:
#   failed to fill whole buffer
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

write_key() {
  # Keep the comment + key lines; force a trailing newline.
  printf '%s\n' "$1" > "$OUT"
  if [ -n "$(tail -c1 "$OUT" 2>/dev/null || true)" ]; then
    printf '\n' >> "$OUT"
  fi
}

if is_minisign_secret "$RAW"; then
  write_key "$RAW"
  echo "normalized: raw-minisign-file"
  exit 0
fi

decode_b64() {
  printf '%s' "$1" | tr -d '\n\t ' | (
    base64 --decode 2>/dev/null ||
      base64 -D 2>/dev/null ||
      openssl base64 -d -A 2>/dev/null
  )
}

if DECODED=$(decode_b64 "$RAW") && is_minisign_secret "$DECODED"; then
  write_key "$DECODED"
  echo "normalized: base64-wrapped-minisign-file"
  exit 0
fi

ONE=$(printf '%s' "$RAW" | tr -d '\n\t ')
if echo "$ONE" | grep -Eq '^[A-Za-z0-9+/=]+$' && [ "${#ONE}" -ge 64 ]; then
  printf '%s\n%s\n' "untrusted comment: tauri signing key" "$ONE" > "$OUT"
  echo "normalized: single-line-base64"
  exit 0
fi

echo "❌ TAURI_SIGNING_PRIVATE_KEY 格式无法识别：既不是两行原文，也不是其 base64，亦非一行 base64" >&2
exit 1
