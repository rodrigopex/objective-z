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
- [x] Add singleton helper (`+shared` via `+initialize`)
  - [x] `+initialize` pattern (called once on first class message, zero runtime code)
  - [x] Demo in `samples/arc_demo` with `AppConfig` singleton
- [x] Add support to RISCV architecture

## v0.3.0 — Dual-Arch, Flat Dispatch & Benchmarks

- [x] RISC-V architecture support (ARM + RISC-V dual-arch)
  - [x] Unified RV32/RV64 `objc_msgSend` trampoline
  - [x] `objc_msg_lookup_sender` slot-based dispatch for RISC-V Clang codegen
  - [x] Auto-detect Homebrew LLVM when Apple Clang lacks RISC-V backend
  - [x] ARC RVO ARM-only; RISC-V always autorelease/retain
- [x] Global flat dispatch table (`CONFIG_OBJZ_FLAT_DISPATCH`)
  - [x] Single 1D BSS table indexed by `(class_id << SEL_BITS) | sel_id`
  - [x] Pointer-hash sel_id cache (64-entry, 512 B BSS)
  - [x] Inheritance flattened at init
- [x] ARC block support (`objc_retain`/`objc_release` handle blocks)
- [x] Build-time retain cycle detection (`CONFIG_OBJZ_CYCLE_CHECK`)
- [x] GPIO wrapper classes (OZGPIOPin, OZGPIOOutput, OZGPIOInput)
- [x] Benchmarks: C++, Rust, Zig, C3 (dispatch + memory comparisons)
- [x] `compile_commands.json` support for ObjC files

## v0.4.0 — CoreZephyr (CZ prefix)

- [ ] CoreZephyr module wrapping Zephyr drivers as ObjC classes
  - [ ] CZInput
  - [ ] CZLED
  - [ ] CZGPIO
  - [ ] CZZBus
