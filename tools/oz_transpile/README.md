# OZ Transpile

Objective-C to plain C transpiler for Objective-Z. Converts `.m` files to GCC-compilable C via Clang AST analysis.

## Pipeline

```
source.m -> clang -ast-dump=json -> oz_transpile -> .h + .c files
```

No ObjC runtime needed for transpiled code.

## Usage

```bash
# 1. Generate Clang AST
clang -Xclang -ast-dump=json -fsyntax-only source.m > source.ast.json

# 2. Transpile
just transpile --input source.ast.json --outdir generated/

# Or directly:
PYTHONPATH=tools python3 -m oz_transpile --input source.ast.json --outdir generated/
```

## CLI Flags

| Flag | Description |
|------|-------------|
| `--input` | Path to Clang JSON AST file (required) |
| `--outdir` | Output directory for generated files (required) |
| `--root-class` | Root class name (default: `OZObject`) |
| `--pool-sizes` | Comma-separated `ClassName=N` pairs |
| `--verbose` | Print diagnostic warnings |
| `--strict` | Treat diagnostics as errors |

## Generated Files

| File | Content |
|------|---------|
| `oz_dispatch.h` | Class ID enum, BOOL typedef, forward decls, OZ_SEND macros, dispatch_free |
| `oz_dispatch.c` | Protocol vtable array definitions, class name/superclass tables |
| `ClassName_ozh.h` | Struct definition, method prototypes, alloc/free inlines, slab extern |
| `ClassName_ozm.c` | Method implementations, OZ_SLAB_DEFINE |

## Supported Language Features

- Class/instance methods, inheritance, protocols
- `@property` / `@synthesize` (atomic, strong, custom getter/setter, ivar names)
- `@synchronized` (RAII spinlock via OZSpinLock)
- Subscript syntax (`array[i]`, `dict[key]`)
- String, boxed, array, and dictionary literals (`@"..."`, `@42`, `@[...]`, `@{...}`)
- Non-capturing blocks with `__block` file-scope promotion
- Fast enumeration (`for-in`) via IteratorProtocol
- Compile-time ARC (scope tracking, auto-dealloc, break/continue cleanup)
- Build-time retain cycle detection (`--strict`)

See [LIMITATIONS.md](../../docs/LIMITATIONS.md) for unsupported features.

## Dispatch Strategy

- **STATIC**: selector has single implementation -> direct `ClassName_sel()` call
- **PROTOCOL**: selector in a protocol or overridden by multiple classes -> direct call when receiver type known, `OZ_PROTOCOL_SEND_sel()` macro (const vtable dispatch) otherwise

## Tests

```bash
just test-transpiler
# or
python3 -m pytest tools/oz_transpile/tests/ -v
```
