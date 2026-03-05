/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file main.c
 * @brief Tests for OZGPIOOutput, OZGPIOInput, and block validation.
 *
 * Uses gpio_emul to simulate button presses and verify output state.
 */
#include <zephyr/ztest.h>

/* ── Cleanup (helpers.m) ───────────────────────────────────────── */

extern void test_gpio_cleanup(void);

/* ── OZGPIOOutput helpers (helpers.m) ──────────────────────────── */

extern bool test_gpio_output_create(void);
extern bool test_gpio_output_is_ready(void);
extern bool test_gpio_output_is_active(void);
extern void test_gpio_output_set_active(bool active);
extern void test_gpio_output_toggle(void);
extern int test_gpio_output_description(char *buf, int maxLen);

/* ── GPIO emul helpers (helpers.m) ─────────────────────────────── */

extern int test_gpio_emul_output_get(void);
extern int test_gpio_emul_btn_set(int value);

/* ── OZGPIOInput helpers (helpers.m) ───────────────────────────── */

extern bool test_gpio_input_create_callback(void);
extern bool test_gpio_input_create_block(void);
extern bool test_gpio_input_create_capturing_block(void);
extern bool test_gpio_input_is_active(void);

/* ── ISR counters (helpers.m) ──────────────────────────────────── */

extern volatile int g_isr_count;
extern volatile int g_isr_block_count;

/* ── Block introspection helpers (helpers.m) ───────────────────── */

extern bool test_gpio_block_is_global(void);
extern bool test_gpio_capturing_block_is_not_global(void);

/* ── Suite setup/teardown ──────────────────────────────────────── */

static void gpio_after(void *fixture)
{
	ARG_UNUSED(fixture);
	test_gpio_cleanup();
}

ZTEST_SUITE(gpio_output, NULL, NULL, NULL, gpio_after, NULL);
ZTEST_SUITE(gpio_input, NULL, NULL, NULL, gpio_after, NULL);
ZTEST_SUITE(gpio_block, NULL, NULL, NULL, NULL, NULL);

/* ── OZGPIOOutput tests ────────────────────────────────────────── */

ZTEST(gpio_output, test_output_create)
{
	zassert_true(test_gpio_output_create(), "OZGPIOOutput init should succeed");
}

ZTEST(gpio_output, test_output_is_ready)
{
	zassert_true(test_gpio_output_create());
	zassert_true(test_gpio_output_is_ready(), "GPIO port should be ready");
}

ZTEST(gpio_output, test_output_set_active_on)
{
	zassert_true(test_gpio_output_create());

	test_gpio_output_set_active(true);
	zassert_true(test_gpio_output_is_active(), "LED should be active after setActive:YES");
	zassert_equal(test_gpio_emul_output_get(), 1, "Emulated pin should be high");
}

ZTEST(gpio_output, test_output_set_active_off)
{
	zassert_true(test_gpio_output_create());

	test_gpio_output_set_active(true);
	test_gpio_output_set_active(false);
	zassert_false(test_gpio_output_is_active(), "LED should be inactive after setActive:NO");
	zassert_equal(test_gpio_emul_output_get(), 0, "Emulated pin should be low");
}

ZTEST(gpio_output, test_output_toggle)
{
	zassert_true(test_gpio_output_create());

	test_gpio_output_set_active(false);
	zassert_equal(test_gpio_emul_output_get(), 0);

	test_gpio_output_toggle();
	zassert_true(test_gpio_output_is_active(), "LED should be active after first toggle");
	zassert_equal(test_gpio_emul_output_get(), 1, "Emulated pin should be high");

	test_gpio_output_toggle();
	zassert_false(test_gpio_output_is_active(), "LED should be inactive after second toggle");
	zassert_equal(test_gpio_emul_output_get(), 0, "Emulated pin should be low");
}

ZTEST(gpio_output, test_output_description)
{
	zassert_true(test_gpio_output_create());

	char buf[48];
	int len = test_gpio_output_description(buf, sizeof(buf));
	zassert_true(len > 0, "Description should be non-empty");
	zassert_not_null(strstr(buf, "OZGPIOOutput"), "Description should contain class name");
}

/* ── OZGPIOInput tests with C callback ─────────────────────────── */

ZTEST(gpio_input, test_input_callback_create)
{
	zassert_true(test_gpio_input_create_callback(),
		     "OZGPIOInput with C callback should succeed on gpio_emul");
}

ZTEST(gpio_input, test_input_callback_isr_fires)
{
	zassert_true(test_gpio_input_create_callback());
	zassert_equal(g_isr_count, 0, "ISR should not have fired yet");

	/* sw0 is GPIO_ACTIVE_LOW: press = pin goes low */
	test_gpio_emul_btn_set(0);
	zassert_equal(g_isr_count, 1, "ISR should fire once on button press");
}

ZTEST(gpio_input, test_input_callback_toggles_led)
{
	zassert_true(test_gpio_input_create_callback());

	zassert_equal(test_gpio_emul_output_get(), 0, "LED should start off");

	/* Press → callback toggles LED on */
	test_gpio_emul_btn_set(0);
	zassert_equal(test_gpio_emul_output_get(), 1, "LED should be on after first press");

	/* Release and press again → LED off */
	test_gpio_emul_btn_set(1);
	test_gpio_emul_btn_set(0);
	zassert_equal(test_gpio_emul_output_get(), 0, "LED should be off after second press");
}

ZTEST(gpio_input, test_input_is_active)
{
	zassert_true(test_gpio_input_create_callback());

	/* Pin high = inactive (GPIO_ACTIVE_LOW) */
	test_gpio_emul_btn_set(1);
	zassert_false(test_gpio_input_is_active(), "Button should be inactive when released");

	/* Pin low = active */
	test_gpio_emul_btn_set(0);
	zassert_true(test_gpio_input_is_active(), "Button should be active when pressed");
}

/* ── OZGPIOInput tests with block callback ─────────────────────── */

ZTEST(gpio_input, test_input_block_create)
{
	zassert_true(test_gpio_input_create_block(),
		     "OZGPIOInput with block callback should succeed on gpio_emul");
}

ZTEST(gpio_input, test_input_block_isr_fires)
{
	zassert_true(test_gpio_input_create_block());
	zassert_equal(g_isr_block_count, 0, "Block ISR should not have fired yet");

	test_gpio_emul_btn_set(0);
	zassert_equal(g_isr_block_count, 1, "Block ISR should fire once on button press");
}

ZTEST(gpio_input, test_input_capturing_block_rejected)
{
	zassert_false(test_gpio_input_create_capturing_block(),
		      "OZGPIOInput with capturing block should return nil");
}

/* ── Block validation tests ────────────────────────────────────── */

ZTEST(gpio_block, test_non_capturing_block_is_global)
{
	zassert_true(test_gpio_block_is_global(),
		     "Non-capturing block should have BLOCK_IS_GLOBAL flag");
}

ZTEST(gpio_block, test_capturing_block_is_not_global)
{
	zassert_true(test_gpio_capturing_block_is_not_global(),
		     "Capturing block should NOT have BLOCK_IS_GLOBAL flag");
}
