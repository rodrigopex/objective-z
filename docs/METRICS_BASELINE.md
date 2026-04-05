# Test Metrics — Phase 5 Expand Coverage

## Baseline (captured 2026-04-05, v0.5.87)

| Metric                    | Value |
|---------------------------|-------|
| Behavior test .m files    | 46    |
| Unity test_* functions    | 175   |
| Behavior categories       | 9     |
| Adapted upstream tests    | 12    |
| Adapted test sources      | 3     |
| Transpiler unit tests     | 517   |
| Zephyr ZTEST cases        | 21    |
| CI jobs                   | 6     |
| Leak detection            | none  |

## After Phase 5

| Metric                    | Value | Delta  |
|---------------------------|-------|--------|
| Behavior test .m files    | 72    | +26    |
| Unity test_* functions    | 209   | +34    |
| Behavior categories       | 16    | +7 new |
| Adapted upstream tests    | 12    | —      |
| Adapted test sources      | 3     | —      |
| Transpiler unit tests     | 517   | —      |
| Zephyr ZTEST cases        | 21    | —      |
| CI jobs                   | 6     | —      |
| Leak detection            | infra | new    |

### New categories added
synchronized (5), forin (4), blocks (3), enum (3), edge +3, macro (2), inline (2), arc (4)

### Deferred items
- Multi-file transpilation tests (Step 4) — requires compile_and_run.py extension
- Zephyr integration additions (Step 11) — requires west build pipeline
