# Objective-Z Transpiler Limitations

The OZ transpiler (`oz_transpile`) converts Objective-C `.m` files to plain C via
Clang JSON AST. The following limitations apply to source files targeting the
transpiler pipeline.

## Blocks

- **Non-capturing blocks only.** Block expressions (`^(params){ body }`) are
  transpiled to static C functions with a function-pointer reference.
  Blocks that capture local variables produce a diagnostic error. To share
  state between a block and its caller, use a `static` file-scope variable.

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

- **No `for-in` (fast enumeration).** `for (id obj in array)` is not supported.
  Use an index-based `for` loop with `count` and `objectAtIndex:`.

## Literals and Expressions

- **No subscript syntax.** `array[i]` and `dict[key]` are not transpiled.
  Use `[array objectAtIndex:i]` and `[dict objectForKey:key]` explicitly.

- **Array / dictionary literals cannot reference local variables.** Literal
  arrays (`@[a, b]`) and dictionaries are emitted as file-scope static
  constants. Every element must be a compile-time constant (another literal,
  `@"string"`, or `@42`). Nest literals inline instead of assigning to locals
  first.

- **OZNumber supports 8/16/32-bit integers and float.** `OZNumber` boxes
  `int8_t`, `uint8_t`, `int16_t`, `uint16_t`, `int32_t`, `uint32_t`, `float`,
  and `BOOL`. 64-bit types (`int64_t`, `double`) are not supported. A future
  `OZNumber64` class may address this.

## Types

- **`id` receiver requires explicit cast.** When a variable is typed `id`
  (e.g., a block parameter), calling a class-specific method requires a cast:
  `[(OZNumber *)obj intValue]`.

- **Use `<stdint.h>` types via OZObject.h.** Fixed-width types (`int8_t`,
  `uint32_t`, etc.) are available through `#include <stdint.h>` in the
  OZObject.h stub. Custom stubs should import OZObject.h to inherit this.

## Miscellaneous

- **No `@try` / `@catch` / `@throw`.** Exception handling is not supported.

- **No `@synchronized`.** Use Zephyr synchronization primitives directly.

- **No dynamic dispatch for non-protocol methods.** All non-protocol method
  calls are resolved statically (direct C function calls). Dynamic method
  resolution and `performSelector:` are not available.
