# Generated Files — Do Not Edit

These files are **oz2c's** output (`tools/oz2c`, the `oz2c` binary),
transpiled from six cases under `tests/behavior/cases/`. They were the Python
pipeline's output until that backend was retired.

To regenerate:

```sh
cargo build --manifest-path tools/oz2c/Cargo.toml
python scripts/regen_zephyr_tests.py
```

The `generated-freshness` CI job runs exactly that and fails if the result
differs from what is committed, so these files cannot drift from the
transpiler that produced them.

Some of these headers are an **ABI shim**: the ztest drivers under
`tests/zephyr/src/` were written against the Python pipeline's generated
naming (`<Class>_ozh.h`, `Class_alloc`, `OZObject_release`), and oz2c
emits one header per origin file with its own spellings. The shim bridges the
two so the drivers compile unmodified — see
`tests/tools/oz2c_build.py::write_abi_shim`.

Any manual edits will be overwritten on regeneration. If the output needs
changing, fix the transpiler.
