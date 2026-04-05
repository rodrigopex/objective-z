/*
 * Regression test for issue #090.
 *
 * Bug: Transpiler drops struct/union/enum/macro definitions from companion
 *      headers when they are not referenced by ObjC interface members.
 * Fix: Scan companion .h with tree-sitter; preserve all non-ObjC content
 *      as header_verbatim_lines emitted in the generated _ozh.h.
 * Commit: TBD
 */
/* oz-pool: SensorCtrl=2 */
#import "issue_090_header_preservation.h"

@implementation SensorCtrl
- (int)reading {
	return _reading;
}
- (void)setReading:(int)val {
	_reading = val;
}
@end
