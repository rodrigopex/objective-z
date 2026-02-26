#include <objc/objc.h>

/* Generate references to Object and OZString classes since they are
   needed by the runtime system to run correctly. */
void __objc_linking(void) {
  [Object name];
  [OZString name];
}
