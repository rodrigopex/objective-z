# Plan: Source-stem-based output filenames

## Context

Generated C files use class names (`AccDataProducer_ozm.c`) instead of the original source filename (`Producer_ozm.c`), breaking traceability. Class-less `.m` files (e.g. `main.m`) get their functions stuffed into `OZObject_ozm.c` instead of a standalone `main_ozm.c`.

## Goal

Output files named after the original `.m` source, not the class name. `Producer.m` → `Producer_ozh.h` + `Producer_ozm.c`. `main.m` → `main_ozm.c`. Stub classes (OZObject, OZString, etc.) keep class name as stem.

## Status

- [x] Phase 1: `_ozh.h`/`_ozm.c` suffix rename
- [x] Phase 2: Eliminate oz_functions, fold into class files
- [ ] Phase 3: Source-stem-based filenames (this phase)

## Phase 3 Changes

### 3.1 CLI: `--sources` argument (`__main__.py`)

Add `--sources` — original `.m` paths, same order as `--input`. Extract stem: `Producer.m` → `Producer`. Set `module.source_stem` after `collect()`.

### 3.2 Model (`model.py`)

- `OZModule.source_stem: str = ""` — stem of the `.m` file
- `OZClass.source_stem: str = ""` — stamped from module before merge

### 3.3 Stamp + associate (`__main__.py`)

After `collect()`, before merge:
- Stamp each class: `cls.source_stem = module.source_stem`
- Associate module-level items with primary class (existing logic)
- If no class: create `OrphanSource(stem, functions, statics, verbatim, includes)` → append to `module.orphan_sources`

### 3.4 Merge (`collect.py`)

- Preserve `source_stem` on classes during merge
- Merge `orphan_sources` lists

### 3.5 Emit by source stem (`emit.py`)

- Group classes by `source_stem`
- Per group: one `<stem>_ozh.h` + one `<stem>_ozm.c`
- Fallback: stub classes with no `source_stem` → use class name as stem
- Orphan sources: emit `<stem>_ozm.c` with functions/verbatim only (include `oz_dispatch.h`)

### 3.6 Template include references

- `oz_dispatch.c.j2`: `#include "{{ cls.source_stem }}_ozh.h"` (deduplicated)
- `oz_mem_slabs.h.j2`: same
- `class_source.c.j2`: `#include "{{ source_stem }}_ozh.h"`
- `class_header.h.j2`: superclass → `{{ superclass_stem }}_ozh.h`

### 3.7 CMake (`oz_transpile.cmake`)

Add `--sources ${_abs_sources}` to both configure-time and build-time transpiler commands.

### 3.8 Tests

- Update expected filenames in assertions
- New test: class name ≠ file stem → output uses file stem
- New test: class-less `.m` → standalone `<stem>_ozm.c`

## Files to modify

| File | Change |
|---|---|
| `model.py` | Add `source_stem` to `OZModule` + `OZClass`, add `OrphanSource` |
| `__main__.py` | Add `--sources`, stamp classes, handle orphans |
| `collect.py` | Preserve `source_stem` in `merge_modules()`, merge orphans |
| `emit.py` | Group by stem, emit per-stem files |
| `class_header.h.j2` | Superclass include by stem |
| `class_source.c.j2` | Self include by stem |
| `oz_dispatch.c.j2` | Include by stem (deduplicated) |
| `oz_mem_slabs.h.j2` | Include by stem (deduplicated) |
| `oz_transpile.cmake` | Pass `--sources` |
| Tests | Update filenames + new cases |

## Verification

1. `just test-transpiler` — all pytest pass
2. `just test` — twister 9/10 (hello_category pre-existing failure)
3. `just test-behavior` — 32/32 pass
4. `just test-adapted` — 12/12 pass
5. Manual: `build/oz_generated/` shows `Producer_ozm.c`, `main_ozm.c`, not `AccDataProducer_ozm.c`
