#!/bin/sh
# Flash PassportOS into the factory app slot. Does not --erase-all (keeps cardid + Recovery).
set -eu
ROOT="$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/riscv32imc-unknown-none-elf/release/passport-os"
if [ ! -f "$BIN" ]; then
  echo "build first: cargo build -p passport-os --release --target riscv32imc-unknown-none-elf" >&2
  exit 1
fi
exec espflash flash --flash-size 8mb --partition-table "$ROOT/partitions.csv" --monitor "$BIN"
