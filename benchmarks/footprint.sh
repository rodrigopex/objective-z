#!/bin/bash
# Firmware footprint analysis: OZ vs C++ vs C (OZ-070)
# Builds each benchmark app and extracts ELF section sizes.
#
# Usage: ./benchmarks/footprint.sh [board]
# Default board: nrf52833dk/nrf52833

set -euo pipefail

BOARD="${1:-nrf52833dk/nrf52833}"
SIZE_TOOL="$HOME/.local/zephyr-sdk-1.0.0/gnu/arm-zephyr-eabi/bin/arm-zephyr-eabi-size"

if [ ! -x "$SIZE_TOOL" ]; then
        echo "ERROR: size tool not found at $SIZE_TOOL"
        exit 1
fi

declare -a NAMES=("OZ speed" "C++ speed" "OZ memory" "C++ memory" "C memory")
declare -a APPS=(
        "benchmarks/objc"
        "benchmarks/cpp"
        "benchmarks/memory/objc"
        "benchmarks/memory/cpp"
        "benchmarks/memory/c"
)

echo "============================================"
echo "  Firmware Footprint Analysis (OZ-070)"
echo "  Board: $BOARD"
echo "============================================"
echo ""

for idx in "${!NAMES[@]}"; do
        name="${NAMES[$idx]}"
        app="${APPS[$idx]}"
        echo "--- $name ($app) ---"

        west build -p -b "$BOARD" "$app" >/dev/null 2>&1
        ELF="build/zephyr/zephyr.elf"

        if [ ! -f "$ELF" ]; then
                echo "  ERROR: $ELF not found"
                continue
        fi

        echo "  Sections:"
        "$SIZE_TOOL" -A "$ELF" | grep -E '^\.(text|rodata|data|bss|noinit)\s' | \
                awk '{printf "    %-12s %8d bytes\n", $1, $2}'

        "$SIZE_TOOL" "$ELF" | tail -1 | \
                awk '{printf "  Total: text=%d data=%d bss=%d dec=%d\n", $1, $2, $3, $4}'
        echo ""
done

echo "============================================"
echo "  ROM/RAM reports (last build: C memory)"
echo "============================================"
echo ""
echo "--- ROM Report ---"
west build -t rom_report 2>/dev/null || echo "  (rom_report not available)"
echo ""
echo "--- RAM Report ---"
west build -t ram_report 2>/dev/null || echo "  (ram_report not available)"
