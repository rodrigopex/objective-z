# Backlog

## v0.2.0 — Version & Collections

- [x] Add VERSION file and generated version header
  - [x] `objc/VERSION` following Zephyr pattern
  - [x] CMake generates `objc/version.h` with OBJZ_VERSION_* macros
  - [x] Boot banner via CONFIG_OBJZ_BOOT_BANNER
- [ ] Add collections config (CONFIG_OBJZ_COLLECTIONS)
  - [ ] OZArray/OZDictionary available without literals
  - [ ] CONFIG_OBJZ_LITERALS depends on CONFIG_OBJZ_COLLECTIONS
- [ ] Add generics usage in tests and samples

## v0.3.0 — Singleton & Advanced Patterns

- [ ] Add singleton helper (`+shared` via dispatch_once)
  - [ ] dispatch_once implementation
  - [ ] Macro or pattern for declaring shared instances

## v0.4.0 — CoreZephyr (CZ prefix)

- [ ] CoreZephyr module wrapping Zephyr drivers as ObjC classes
  - [ ] CZInput
  - [ ] CZLED
  - [ ] CZGPIO
  - [ ] CZZBus
