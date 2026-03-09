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

## v0.4.0 — Transpiler Testing & Platform Abstraction

### Phase 0 — Platform Abstraction Layer (PAL)

Decouple transpiler-generated C from Zephyr so output compiles on any POSIX host. Zero-cost: all PAL functions are `static inline`, vanish at `-O1+`.

- [x] Assess existing runtime test suites (Bucket A/B/C classification)
  - [x] `test/ASSESSMENT.md` with every test file classified
  - [x] Rename `tests/` → `tests/objc-reference/` with README
- [x] Create PAL headers (`include/oz/platform/`)
  - [x] `oz_platform_types.h` — return codes, shared types (no Zephyr/POSIX includes)
  - [x] `oz_platform_host.h` — malloc slab, C11 atomics, printf
  - [x] `oz_platform_zephyr.h` — k_mem_slab, Zephyr atomics, printk pass-through
  - [x] `oz_platform.h` — ifdef router (`OZ_PLATFORM_ZEPHYR` / `OZ_PLATFORM_HOST`)
- [x] Update `emit.py` to use PAL
  - [x] Replace all `#include <zephyr/...>` with `#include "oz/platform/oz_platform.h"`
  - [x] Replace `K_MEM_SLAB_DEFINE` → `OZ_SLAB_DEFINE`, `k_mem_slab_*` → `oz_slab_*`
  - [x] Replace `atomic_*` → `oz_atomic_*`, `printk` → `oz_platform_print`
  - [x] Generated code compiles with `-DOZ_PLATFORM_HOST` on host
  - [x] Generated code still compiles with `-DOZ_PLATFORM_ZEPHYR` in Zephyr build
- [x] Smoke test: transpile → compile → run on host, print output, exit 0
- [x] Zero-cost verification: `objdump` confirms no `oz_` symbols in Zephyr binary

### Phase 1 — Test Infrastructure (Golden-File Tests)

Golden-file tests for transpiler output stability. Each test = `.m` input + expected `.c`/`.h` output.

- [ ] Vendor Unity test framework (`test/lib/unity/`)
- [ ] Create golden-file test runner (`test/runner.py`)
  - [ ] pytest-compatible, discovers `test/golden/` subdirectories
  - [ ] `--update-golden` flag to regenerate expected files
  - [ ] Error tests via `config.json` (`expect_error`, `expected_stderr`)
- [ ] Create 12–15 golden tests
  - [ ] `empty_class`, `class_with_method`, `class_with_property`
  - [ ] `simple_inheritance`, `protocol_conformance`
  - [ ] `message_send_static`, `message_send_protocol`
  - [ ] `retain_release`, `nil_receiver`, `singleton_pattern`
  - [ ] `category_merge`, `multiple_args`
  - [ ] Error tests: `error_unsupported_feature`, `error_missing_method`
- [ ] justfile targets: `test-golden`, `update-golden`, `smoke`
- [ ] `test/golden/README.md` documenting workflow

### Phase 2 — Compiled Behavior Tests

Verify transpiled C executes correctly on host via Unity assertions. Transpile → compile → run pipeline.

- [ ] Create behavior test orchestrator (`test/tools/compile_and_run.py`, `gen_test_main.py`)
- [ ] Lifecycle tests (5): alloc, init, dealloc/slab-free, ENOMEM, double-release guard
- [ ] Static dispatch tests (5): correct routing, super, override, inherited, class method
- [ ] Protocol dispatch tests (4): switch routing, multiple conformance, protocol inheritance, typed var
- [ ] Memory management tests (5): retain inc, release dec, free-at-zero, nested, retainCount
- [ ] Property tests (5): getter/setter, dot syntax, readonly, strong vs assign, override
- [ ] Edge case tests (4): nil-returns-zero, multiple args, empty class, deep inheritance
- [ ] Compiler matrix: GCC + Clang × O0 + O2
- [ ] Sanitizer support: ASan + UBSan

### Phase 3 — CI Pipeline, Coverage & Upstream Tests

Automate all tests in GitHub Actions, add coverage, adapt upstream LLVM/GNUstep tests.

- [ ] GitHub Actions CI pipeline (`.github/workflows/ci.yml`)
  - [ ] Python tests (transpiler + golden files) with coverage
  - [ ] Behavior tests (compiler matrix: GCC/Clang × O0/O2)
  - [ ] Sanitizer job (ASan + UBSan)
  - [ ] C coverage (gcov + Codecov)
- [ ] Error/negative tests (8–10): blocks, try/catch, dynamic typing, KVO, forward invocation, circular inheritance, etc.
- [ ] Regression test infrastructure (`test/golden/regression/`, `test/behavior/cases/regression/`)
- [ ] Adapt 5 LLVM/Clang Rewriter tests (`test/adapted/llvm_rewriter/`)
- [ ] Adapt 5 GNUstep libobjc2 tests (`test/adapted/gnustep/`)
- [ ] Apple objc4 spec-derived behavioral tests (2–3, `test/adapted/apple_spec/`)

### Phase 4 — Zephyr Integration Tests

Validate transpiled C on real Zephyr kernel via `native_sim` + `ztest` + `twister`.

- [ ] Zephyr test project structure (`tests/zephyr/`, CMake, prj.conf, testcase.yaml)
- [ ] Transpile test classes into `tests/zephyr/generated/`
- [ ] Write 12+ ztest cases across 4 suites (lifecycle, dispatch, memory, protocol)
- [ ] Zephyr CI job (`native_sim` + twister)
- [ ] Hardware build-verification job (compile-only for Cortex-M, PAL inlining check)
- [ ] Generated file freshness check in CI

## v0.5.0 — CoreZephyr (CZ prefix)

- [ ] CoreZephyr module wrapping Zephyr drivers as ObjC classes
  - [ ] CZInput
  - [ ] CZLED
  - [ ] CZGPIO
  - [ ] CZZBus
