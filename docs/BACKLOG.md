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

Golden-file snapshot tests for transpiler output stability. Each test = hand-crafted `.ast.json` input + expected C output directory. Colocated with existing pytest suite at `tools/oz_transpile/tests/golden/`.

- [x] Create golden-file test runner (`tools/oz_transpile/tests/golden/`)
  - [x] `conftest.py` — pytest fixture discovery over golden subdirectories
  - [x] `test_golden.py` — parametrized snapshot comparison with unified diff on mismatch
  - [x] `update.py` — regenerate all `expected/` dirs from transpiler output
  - [x] Error tests via `config.json` (`expect_error`, extra `flags`)
- [x] Create 8 golden tests
  - [x] `simple_led` — baseline (inheritance, protocol, ivars, super call)
  - [x] `empty_class` — minimal struct with base fields only
  - [x] `simple_inheritance` — 3-level hierarchy, field embedding
  - [x] `protocol_dispatch` — 2 classes conforming to protocol, vtable switch
  - [x] `multiple_args` — 3-arg method, underscore-separated C signature
  - [x] `object_ivars_arc` — ARC retain/release for object-typed ivars
  - [x] `pool_sizes` — `--pool-sizes` flag via config.json
  - [x] `static_dispatch_only` — no protocol overlap, all direct calls
- [x] justfile target: `update-golden`
- [x] `tools/oz_transpile/tests/golden/README.md` documenting workflow
- Unity vendoring deferred to Phase 2 (not needed for golden-file diffing)

### Phase 2 — Compiled Behavior Tests

Verify transpiled C executes correctly on host via Unity assertions. Transpile → compile → run pipeline. AST dump via Homebrew LLVM with `-fobjc-runtime=gnustep-2.0 --target=arm-none-eabi`.

- [x] Vendor Unity 2.6.0 (`test/lib/unity/`)
- [x] Create behavior test orchestrator (`test/tools/compile_and_run.py`, `gen_test_main.py`)
- [x] Create `OZTestBase.h` — minimal OZObject for Clang AST parsing
- [x] Lifecycle tests (5): alloc, init, dealloc/slab-free, ENOMEM, double-release guard
- [x] Static dispatch tests (5): correct routing, super, override, inherited, class method
- [x] Protocol dispatch tests (4): switch routing, multiple conformance, protocol inheritance, typed var
- [x] Memory management tests (5): retain inc, release dec, free-at-zero, nested, retainCount
- [x] Property tests (5): getter/setter, dot syntax, readonly, strong vs assign, override
- [x] Edge case tests (4): nil-returns-zero, multiple args, empty class, deep inheritance
- [x] justfile target: `test-behavior` (28 tests, clang/O0)
- Sanitizer support: `--sanitize` flag ready, full matrix deferred to Phase 3 CI

### Phase 3 — CI Pipeline, Coverage & Upstream Tests

Automate all tests in GitHub Actions, add coverage, adapt upstream LLVM/GNUstep tests.

- [x] GitHub Actions CI pipeline (`.github/workflows/ci.yml`)
  - [x] Python tests (transpiler + golden files) with coverage
  - [x] Behavior tests (compiler matrix: GCC/Clang × O0/O2)
  - [x] Sanitizer job (ASan + UBSan)
  - [x] C coverage (gcov + Codecov)
- [x] Error/negative tests (5 golden + 2 runtime): try/catch, KVO, forward invocation, circular inheritance, duplicate method
- [x] Transpiler feature rejection: `module.errors` for hard errors, `--strict` for warnings
- [x] Regression test infrastructure (`tools/oz_transpile/tests/golden/regression/`, `test/behavior/cases/regression/`)
- [x] Adapt 5 LLVM/Clang Rewriter tests (`test/adapted/llvm_rewriter/`)
- [x] Adapt 5 GNUstep libobjc2 tests (`test/adapted/gnustep/`)
- [x] Apple objc4 spec-derived behavioral tests (2, `test/adapted/apple_spec/`)
- [x] Extended `compile_and_run.py`: `--compiler`, `--cflags`, `--ldflags`, string `--sanitize`
- [x] justfile targets: `test-adapted`, `test-all-transpiler`

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
