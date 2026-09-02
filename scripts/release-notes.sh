#!/usr/bin/env bash
# Generate the GitHub Release body for a PassportOS tag.
# Usage:
#   TAG=v0.1.0 COMMIT=abc123 \
#   ELF=dist/passport-os-0.1.0-esp32c3.elf \
#   BIN=dist/passport-os-0.1.0-esp32c3.bin \
#   PART=dist/partitions.csv \
#   SUMS=dist/SHA256SUMS.txt \
#   RUSTC="$(rustc -V)" \
#   IMAGE_BYTES=726144 \
#   ./scripts/release-notes.sh > dist/RELEASE_NOTES.md
set -euo pipefail

: "${TAG:?TAG is required}"
: "${COMMIT:?COMMIT is required}"
: "${ELF:?ELF is required}"
: "${BIN:?BIN is required}"
: "${PART:?PART is required}"
: "${SUMS:?SUMS is required}"

ROOT="$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="${TAG#v}"
DATE_UTC="$(date -u +%Y-%m-%d)"
SHORT="${COMMIT:0:12}"
RUSTC="${RUSTC:-$(rustc -V 2>/dev/null || echo unknown)}"
HOST="${HOST:-$(uname -srm 2>/dev/null || echo unknown)}"
ELF_NAME="$(basename "$ELF")"
BIN_NAME="$(basename "$BIN")"
PART_NAME="$(basename "$PART")"
SUMS_NAME="$(basename "$SUMS")"
IMAGE_BYTES="${IMAGE_BYTES:-$(wc -c < "$BIN" | tr -d ' ')}"
IMAGE_KIB="$(awk -v b="$IMAGE_BYTES" 'BEGIN { printf "%.1f", b/1024 }')"
FACTORY=3145728
PCT="$(awk -v b="$IMAGE_BYTES" -v f="$FACTORY" 'BEGIN { printf "%.2f", (b*100)/f }')"

hash_line() {
  local f="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$f" | awk '{print $1}'
  else
    shasum -a 256 "$f" | awk '{print $1}'
  fi
}

ELF_SHA="$(hash_line "$ELF")"
BIN_SHA="$(hash_line "$BIN")"
PART_SHA="$(hash_line "$PART")"

PREV="$(git describe --tags --abbrev=0 --match 'v*' "${COMMIT}^" 2>/dev/null || true)"
CHANGELOG=""
if [ -n "$PREV" ]; then
  CHANGELOG="$(git log --no-merges --pretty=format:'- %s (`%h`)' "${PREV}..${COMMIT}")"
  RANGE="${PREV} → ${TAG}"
else
  CHANGELOG="- Initial public release of PassportOS."
  RANGE="${TAG} (first tagged release)"
fi
if [ -z "$CHANGELOG" ]; then
  CHANGELOG="- Maintenance release; see the compared commits on GitHub."
fi

cat <<EOF
# PassportOS ${TAG}

**Official firmware release** for the FoloToy AI Passport (ESP32-C3).

| Field | Value |
| --- | --- |
| Product | PassportOS |
| Version | ${VERSION} |
| Git tag | \`${TAG}\` |
| Revision | \`${SHORT}\` |
| Released | ${DATE_UTC} (UTC) |
| License | MIT |
| Source | https://github.com/zh30/PassportOS |

This build is produced by GitHub Actions from the git tag of this release.
Do not flash unofficial binaries to the factory slot if you need official
Recovery restore.

---

## 中文摘要

这是 **PassportOS ${TAG}** 的正式固件。适用于 **FoloToy AI Passport**（ESP32-C3，8 MB Flash，无 PSRAM）。

- 将下面的 \`${ELF_NAME}\` 与 \`${PART_NAME}\` 一并下载。
- 刷写**不要**加 \`--erase-all\`，否则会清掉 \`cardid\`（\`0x356000\`）和官方 Recovery（\`0x700000\`）。
- USB 控制台是 GPIO18/19 的 Serial/JTAG；**不要**把 UART0 当控制台（TX 是背光 GPIO21）。
- 工厂分区 3 MB，本镜像 **${IMAGE_BYTES} 字节**（约 ${IMAGE_KIB} KiB，占工厂槽 ${PCT}%）。

详细刷写、校验和与已知限制见下文英文正文。

---

## Hardware compatibility

| Item | Requirement |
| --- | --- |
| Board | FoloToy AI Passport |
| MCU | ESP32-C3, RV32IMC, **no PSRAM** |
| Flash | **8 MB** |
| Display | ST7789P3 240×320 RGB565, invert-on |
| Input | Three-key ADC ladder on GPIO0 |
| Console | USB Serial/JTAG GPIO18 / GPIO19 |

**Not MCU-owned:** the passive NTAG213 (phone RF only) and the hardware power
button (not a GPIO). Firmware cannot read or write the tag.

HAL: \`esp-hal\` **v1.2.0-rc.0**. Target triple:
\`riscv32imc-unknown-none-elf\`.

## Artifacts

Flash the **ELF** with the published partition table (same path as
\`scripts/flash.sh\`). The **BIN** is the factory-app image at \`0x10000\`
produced by \`espflash save-image\`.

| File | Role | SHA-256 |
| --- | --- | --- |
| \`${ELF_NAME}\` | Stripped release ELF (preferred for \`espflash flash\`) | \`${ELF_SHA}\` |
| \`${BIN_NAME}\` | Factory application image (offset \`0x10000\`) | \`${BIN_SHA}\` |
| \`${PART_NAME}\` | Partition table (do not substitute an unofficial table) | \`${PART_SHA}\` |
| \`${SUMS_NAME}\` | SHA-256 manifest of the files above | see file |

Image size: **${IMAGE_BYTES} bytes** (${IMAGE_KIB} KiB), **${PCT}%** of the
3 145 728-byte factory slot.

Verify on a POSIX host:

\`\`\`bash
sha256sum -c SHA256SUMS.txt
\`\`\`

## Installation

Requires [\`espflash\`](https://github.com/esp-rs/espflash) 4.x.

\`\`\`bash
espflash flash --flash-size 8mb --partition-table partitions.csv \\
  --monitor ${ELF_NAME}
\`\`\`

**Do not pass \`--erase-all\`.** That erases:

| Region | Offset | Size |
| --- | --- | --- |
| factory app (this release) | \`0x10000\` | 3 MB |
| KV (high scores + last clock) | \`0x350000\` | 4 KB |
| \`cardid\` (preserve) | \`0x356000\` | 16 KB |
| Recovery (preserve) | \`0x700000\` | 1 MB |

UART0 TX is the backlight (GPIO21). Never attach a UART console there.

A healthy boot prints:

\`\`\`
[boot] PassportOS r1
[boot] display ST7789P3 240x320 invert-on
[shell] ready workspaces=2 overlay=launcher
[ui] painted overlay=launcher
\`\`\`

\`bat=--\` is valid when the CW2017 fuel gauge is absent.

## What this image contains

Compiled-in launcher apps: **Pulse**, **Tap** (NTAG213 facts), **Flap**,
**Stack**, **Brick**, **System**.

System menu: brightness, appearance, Wi-Fi (scan / pick / 3-key English IME /
join; open networks skip the IME), Bluetooth advertising, sleep, keys, about.
Wi-Fi credentials are RAM-only. Wi-Fi, BLE, and I2S DMA are exclusive.

On-card keys, top → bottom: **UP / DOWN / OK**. Long OK (~800 ms) is home.
While a game is focused, UP/DOWN are not stolen for workspace or tile switching.

## Quality assurance

This tag was gated in CI before the release was published:

1. \`cargo test -p passport-core\` (host: decoder, compositor, shell, games, IME)
2. \`cargo build -p passport-os --release --target riscv32imc-unknown-none-elf\`

Toolchain: \`${RUSTC}\`
Runner: \`${HOST}\`

Live animation is dirty-rect only (\`LIVE_SPI_BUDGET\` 8 KiB). Do not treat a
full 240×298 content fill as a live frame.

## Changes

Range: ${RANGE}

${CHANGELOG}

## Known limitations

- No battery-backed RTC; \`time HH:MM\` is last-set plus uptime, stored in KV.
- NTAG213 has no MCU bus (\`mcu_read\` / \`mcu_write\` return \`NoBus\`).
- ESP32-C3 has no Bluetooth Classic.
- Light theme exists; a full-panel light fill scrambles this ST7789 (花屏).
- WPA3-only access points are not supported by this \`esp-radio\` revision.

## Support

- Source and issues: https://github.com/zh30/PassportOS
- Hardware BSP / pin contract: https://github.com/FoloToy/ai-passport
- App developer guide: \`docs/APP.md\` in this tag

Copyright © 2026 Henry Zhang. MIT License.
EOF
