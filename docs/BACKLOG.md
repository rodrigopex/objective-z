# Backlog

## v0.2.0 — Version & Collections

- [x] Add VERSION file and generated version header
  - [x] `objc/VERSION` following Zephyr pattern
  - [x] CMake generates `objc/version.h` with `OBJZ_VERSION_*` macros
  - [x] Boot banner via CONFIG_OBJZ_BOOT_BANNER
- [x] Generated slabs to reduce heap usage and do not require manual sizing from the developer.
  - [x] Remove MRR support. Force user to use only ARC + (generated pools)
- [x] Add collections config (CONFIG_OBJZ_COLLECTIONS)
  - [x] OZArray/OZDictionary available without literals
  - [x] CONFIG_OBJZ_LITERALS depends on CONFIG_OBJZ_COLLECTIONS && CONFIG_OBJZ_NUMBERS
- [x] Add generics usage in tests and samples
- [x] Auto-compute runtime table sizes via tree-sitter source analysis
  - [x] objz_gen_table_sizes.py using tree-sitter queries (no Clang AST dumps)
  - [x] Kconfig defaults to 0 (auto), non-zero overrides
  - [x] Deferred CMake target for multiple objz_target_sources() calls
- [x] Per-class dispatch table sizing via OZ_DEFINE_DTABLE
  - [x] `objc/include/objc/dtable.h`: `OZ_DEFINE_DTABLE(ClassName, cls_size, meta_size)` macro
  - [x] Registry-based lookup with heap fallback for unregistered classes
  - [x] `objz_gen_table_sizes.py` auto-generates `dtable_pool.c` per-class entries
- [ ] Add singleton helper (`+shared` via dispatch_once)
  - [ ] dispatch_once implementation
  - [ ] Macro or pattern for declaring shared instances

## v0.3.0 — Singleton & Advanced Patterns

- [ ] Add support to RISCV architecture
- [ ] CoreZephyr module wrapping Zephyr drivers as ObjC classes
  - [ ] CZInput
  - [ ] CZLED
  - [ ] CZGPIO
  - [ ] CZZBus

## v0.4.0 — CoreZephyr (CZ prefix)

Unplanned yet.
