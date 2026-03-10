#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Merge ObjC compile commands into CMake's compile_commands.json."""

import json
import sys
from pathlib import Path


def main():
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <compile_commands.json> <objc_commands.json>")
        sys.exit(1)

    db_path = Path(sys.argv[1])
    objc_path = Path(sys.argv[2])

    db = json.loads(db_path.read_text()) if db_path.exists() else []
    objc = json.loads(objc_path.read_text()) if objc_path.exists() else []

    if not objc:
        return

    seen = {e["file"] for e in objc}
    merged = [e for e in db if e["file"] not in seen] + objc

    db_path.write_text(json.dumps(merged, indent=2) + "\n")


if __name__ == "__main__":
    main()
