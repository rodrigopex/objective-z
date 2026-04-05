/*
 * Behavior test for issue #090: all non-ObjC header content must compile
 * and work correctly after transpilation.
 */
#include "unity.h"
#include "SensorCtrl_ozh.h"

/* ---- enum preserved ---- */
void test_enum_values_accessible(void)
{
	enum sensor_state s = SENSOR_IDLE;
	TEST_ASSERT_EQUAL_INT(0, s);
	s = SENSOR_SAMPLING;
	TEST_ASSERT_EQUAL_INT(1, s);
	s = SENSOR_ERROR;
	TEST_ASSERT_EQUAL_INT(2, s);
}

/* ---- struct preserved ---- */
void test_struct_usable(void)
{
	struct sensor_msg msg;
	msg.state = SENSOR_SAMPLING;
	msg.value = 42;
	TEST_ASSERT_EQUAL_INT(SENSOR_SAMPLING, msg.state);
	TEST_ASSERT_EQUAL_INT(42, msg.value);
}

/* ---- union preserved ---- */
void test_union_usable(void)
{
	union sensor_data d;
	d.raw = 100;
	TEST_ASSERT_EQUAL_INT(100, d.raw);
	d.calibrated = 3.14f;
	TEST_ASSERT_FLOAT_WITHIN(0.01f, 3.14f, d.calibrated);
}

/* ---- #define constant preserved ---- */
void test_define_constant_accessible(void)
{
	TEST_ASSERT_EQUAL_INT(8, SENSOR_MAX_CHANNELS);
	int arr[SENSOR_MAX_CHANNELS];
	arr[0] = 1;
	arr[7] = 99;
	TEST_ASSERT_EQUAL_INT(1, arr[0]);
	TEST_ASSERT_EQUAL_INT(99, arr[7]);
}

/* ---- function-like #define macro preserved ---- */
void test_function_macro_works(void)
{
	TEST_ASSERT_EQUAL_INT(5, SENSOR_CLAMP(3, 5, 10));
	TEST_ASSERT_EQUAL_INT(7, SENSOR_CLAMP(7, 5, 10));
	TEST_ASSERT_EQUAL_INT(10, SENSOR_CLAMP(15, 5, 10));
}

/* ---- static inline function preserved ---- */
void test_static_inline_function_works(void)
{
	TEST_ASSERT_EQUAL_INT(30, sensor_scale(10, 3));
	TEST_ASSERT_EQUAL_INT(0, sensor_scale(0, 100));
	TEST_ASSERT_EQUAL_INT(-6, sensor_scale(-2, 3));
}

/* ---- ObjC class still works alongside preserved content ---- */
void test_class_methods_work(void)
{
	struct SensorCtrl *ctrl = SensorCtrl_alloc();
	TEST_ASSERT_NOT_NULL(ctrl);
	SensorCtrl_setReading_(ctrl, 123);
	TEST_ASSERT_EQUAL_INT(123, SensorCtrl_reading(ctrl));
	OZObject_release((struct OZObject *)ctrl);
}

/* ---- struct and enum interact correctly ---- */
void test_struct_enum_interaction(void)
{
	struct sensor_msg msg;
	msg.state = SENSOR_ERROR;
	msg.value = sensor_scale(5, SENSOR_MAX_CHANNELS);
	TEST_ASSERT_EQUAL_INT(SENSOR_ERROR, msg.state);
	TEST_ASSERT_EQUAL_INT(40, msg.value);
}
