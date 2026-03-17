# OZ-NNN: [URGENT] Short title

- **status:** open
- **filed-by:** DEV | QA
- **date:** 2026-03-15
- **blocking:** YES
- **assigned-to:** (empty until MAINTAINER picks it up)
- **resolved-by:** (empty until fixed)
- **resolved-date:** (empty until fixed)
- **qa-review:** (empty | QR-NNN reference | PASS | WAIVED)
- **verified-by:** (empty until DEV confirms)
- **commit:** (empty until fixed)

## Context

Trying to add state machine in `src/services/PXDeviceManager.m`.

## Input

```objc
switch ([self state]) {
    case SensorStateIdle: ...
}
```

## Observed

Build fails — transpiler emits `/* TODO: SwitchStmt */` comment. GCC then
fails on the incomplete generated C.

## Expected

Transpiled C equivalent using if/else chain or jump table.

## Workaround

Rewrote as if/else chain manually. See WORKAROUNDS.md entry WA-001.

## Discussion

- **MAINTAINER (2026-03-15):** Is this a collect.py gap or emit.py gap?
- **RODRIGO (2026-03-15):** collect.py — `SwitchStmt` is not walked at all.
  Add a handler in `_walk_stmt()` that decomposes into chained `IfStmt`.
- **MAINTAINER (2026-03-15):** Got it, implementing.

## Resolution

(filled by MAINTAINER when fixed)
