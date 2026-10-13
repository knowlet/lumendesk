#!/bin/sh
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
tsc=${TSC:-tsc}
"$tsc" --project "$here/tsconfig.json"
if "$tsc" --project "$here/tsconfig.invalid.json"; then
  echo "Expected invalid TypeScript fixture to fail compilation" >&2
  exit 1
fi
echo "Positive TypeScript fixture passed; invalid fixture failed as expected."
