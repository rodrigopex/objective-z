# Objective-Z Transpiler Limitations

The OZ transpiler (`oz_transpile`) converts Objective-C `.m` files to plain C via
Clang JSON AST. The following limitations apply to source files targeting the
transpiler pipeline.

## Blocks

- **Non-capturing blocks only.** Block expressions (`^(params){ body }`) are
  transpiled to static C functions with a function-pointer reference.
  Blocks that capture local variables produce a diagnostic error. To share
  state between a block and its caller, use a `static` file-scope variable
  or a `__block` variable (see below).

- **`__block` becomes file-scope `static`.** Variables declared with the
  `__block` qualifier are promoted to file-scope static variables by the
  transpiler. This differs from ObjC `__block` semantics: the variable is
  initialized once (at program start), not each time the enclosing function
  runs. This is a design decision for the transpiler, not a limitation.

- **File-scope variables must be `static`.** Because non-capturing blocks become
  standalone C functions, any file-scope variable they reference must be declared
  `static`. A plain global (`int sum = 0;`) is not collected by the transpiler
  and will be absent from the generated code. Use `static int sum = 0;` instead.

- **Function-local `static` variables not supported.** A `static` variable
  declared inside a function body is not collected by the transpiler. It will
  be emitted as a plain local, invisible to block functions. Declare shared
  `static` variables at file scope instead.

## Control Flow

- **No `switch` / `case` statements.** The transpiler does not handle
  `SwitchStmt` or `CaseStmt` AST nodes. Use `if` / `else if` chains instead.

- **`for-in` uses IteratorProtocol, not NSFastEnumeration.** The transpiler
  lowers `for (id obj in collection)` to a scoped iterator loop via
  `IteratorProtocol` (`iter`/`next`), not Apple's `countByEnumeratingWithState:`.
  The collection must conform to `IteratorProtocol` (OZArray does by default).

## Literals and Expressions

- **No boxed expressions (`@(expr)`).** `ObjCBoxedExpr` AST nodes are not
  transpiled. The generated code will contain a `/* TODO: ObjCBoxedExpr */`
  comment and will not compile. Use explicit `OZNumber` initializers instead
  (e.g., `[OZNumber numberWithInt:42]`).

- **OZNumber supports 8/16/32-bit integers and float.** `OZNumber` boxes
  `int8_t`, `uint8_t`, `int16_t`, `uint16_t`, `int32_t`, `uint32_t`, `float`,
  and `BOOL`. 64-bit types (`int64_t`, `double`) are not supported. A future
  `OZNumber64` class may address this.

## Properties

- **`@synthesize` requires explicit ivar form.** Use `@synthesize propName = _propName;`
  (explicit ivar assignment). The bare form `@synthesize propName;` is supported but
  creates an ivar with the bare property name (no underscore prefix), which differs
  from the modern ObjC implicit synthesis convention. A diagnostic warning is emitted
  when the bare form is detected.

## Types

- **File-scope statics with ObjC types need care.** A file-scope `static` variable
  with an ObjC class type (e.g., `static MyClass *shared = nil;`) requires the
  initializer to be a simple literal or `NULL`. Complex initializers like `nil`
  (when represented as a non-literal AST node) may not be recognized. Prefer
  declaring without initializer and assigning in `+initialize`:
  `static MyClass *shared;` then `shared = [MyClass alloc];` in the method body.

- **No `typedef`.** `typedef` declarations are not collected by the transpiler.
  Use explicit types instead. For block types, use inline block syntax
  (`int (^)(void)`) rather than a typedef alias.

- **`id` receiver requires explicit cast.** When a variable is typed `id`
  (e.g., a block parameter), calling a class-specific method requires a cast:
  `[(OZNumber *)obj intValue]`.

- **Use `<stdint.h>` types via OZObject.h.** Fixed-width types (`int8_t`,
  `uint32_t`, etc.) are available through `#include <stdint.h>` in the
  OZObject.h stub. Custom stubs should import OZObject.h to inherit this.

## Miscellaneous

- **No `@try` / `@catch` / `@throw`.** Exception handling is not supported.

- **No dynamic dispatch for non-protocol methods.** All non-protocol method
  calls are resolved statically (direct C function calls). Dynamic method
  resolution and `performSelector:` are not available.
