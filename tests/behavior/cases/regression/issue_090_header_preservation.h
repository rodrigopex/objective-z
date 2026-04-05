/*
 * Companion header for regression test issue #090.
 *
 * All non-ObjC content below must survive transpilation and appear
 * in the generated _ozh.h header.
 */
#import "OZTestBase.h"

/* --- enum not used by @interface --- */
enum sensor_state {
	SENSOR_IDLE = 0,
	SENSOR_SAMPLING,
	SENSOR_ERROR,
};

/* --- struct not used by @interface --- */
struct sensor_msg {
	enum sensor_state state;
	int value;
};

/* --- union not used by @interface --- */
union sensor_data {
	int raw;
	float calibrated;
};

/* --- #define constant --- */
#define SENSOR_MAX_CHANNELS 8

/* --- function-like #define macro --- */
#define SENSOR_CLAMP(v, lo, hi) ((v) < (lo) ? (lo) : (v) > (hi) ? (hi) : (v))

/* --- static inline helper --- */
static inline int sensor_scale(int raw, int factor)
{
	return raw * factor;
}

/* --- ObjC interface (should NOT appear as verbatim) --- */
@interface SensorCtrl : OZObject {
	int _reading;
}
- (int)reading;
- (void)setReading:(int)val;
@end
