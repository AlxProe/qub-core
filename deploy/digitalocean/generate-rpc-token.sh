#!/usr/bin/env bash
set -euo pipefail

OUTPUT=${1:-/opt/qub/headless/config/rpc.token}
FORCE=${2:-}

if [ -e "$OUTPUT" ] && [ "$FORCE" != "--force" ]; then
  echo "RPC token already exists: $OUTPUT" >&2
  echo "Refusing to overwrite. Pass --force as the second argument only during an intentional rotation." >&2
  exit 1
fi

umask 077
mkdir -p "$(dirname -- "$OUTPUT")"
TMP="${OUTPUT}.tmp.$$"
trap 'rm -f "$TMP"' EXIT

# 32 random bytes encoded as 64 lowercase hexadecimal characters.
head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n' > "$TMP"
printf '\n' >> "$TMP"
chmod 0600 "$TMP"
mv -f "$TMP" "$OUTPUT"
trap - EXIT

printf 'Created RPC token: %s\n' "$OUTPUT"
stat -c '%A %a %U:%G %n' "$OUTPUT" 2>/dev/null || ls -l "$OUTPUT"
