/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file helpers.m
 * @brief ObjC helper functions for the GPIO test suite.
 *
 * Compiled with ARC via objz_target_sources().
 * Provides C-callable wrappers around OZGPIOOutput and OZGPIOInput.
 */
#import <Foundation/Foundation.h>
#import <objc/blocks.h>
#import <objc/objc.h>

#include <objc/arc.h>
#include <objc/runtime.h>
#include <zephyr/drivers/gpio/gpio_emul.h>

static const struct gpio_dt_spec kLedSpec = GPIO_DT_SPEC_GET(DT_ALIAS(led0), gpios);
static const struct gpio_dt_spec kBtnSpec = GPIO_DT_SPEC_GET(DT_ALIAS(sw0), gpios);

/* ── Managed objects — ARC releases old on reassign ────────────── */

static OZGPIOOutput *g_led;
static OZGPIOInput *g_btn;

/* ── ISR counters ──────────────────────────────────────────────── */

volatile int g_isr_count;
volatile int g_isr_block_count;

/* ── Cleanup — release managed objects ─────────────────────────── */

void test_gpio_cleanup(void)
{
	g_btn = nil;  /* ARC releases → dealloc removes callback */
	g_led = nil;
	g_isr_count = 0;
	g_isr_block_count = 0;
}

/* ── OZGPIOOutput helpers ──────────────────────────────────────── */

BOOL test_gpio_output_create(void)
{
	g_led = [[OZGPIOOutput alloc] initWithDTSpec:&kLedSpec flags:0];
	return g_led != nil;
}

BOOL test_gpio_output_is_ready(void)
{
	return [g_led isReady];
}

BOOL test_gpio_output_is_active(void)
{
	return [g_led isActive];
}

void test_gpio_output_set_active(BOOL active)
{
	[g_led setActive:active];
}

void test_gpio_output_toggle(void)
{
	[g_led toggle];
}

int test_gpio_output_description(char *buf, int maxLen)
{
	return [g_led cDescription:buf maxLength:maxLen];
}

/* ── GPIO emul helpers (C-callable) ────────────────────────────── */

int test_gpio_emul_output_get(void)
{
	return gpio_emul_output_get(kLedSpec.port, kLedSpec.pin);
}

int test_gpio_emul_btn_set(int value)
{
	return gpio_emul_input_set(kBtnSpec.port, kBtnSpec.pin, value);
}

/* ── OZGPIOInput — C callback variant ──────────────────────────── */

static void btn_c_callback(const struct device *port, struct gpio_callback *cb,
			   gpio_port_pins_t pins)
{
	g_isr_count++;
	[g_led toggle];
}

BOOL test_gpio_input_create_callback(void)
{
	/* Ensure pin is inactive before configuring interrupt */
	gpio_emul_input_set(kBtnSpec.port, kBtnSpec.pin, 1);

	g_led = [[OZGPIOOutput alloc] initWithDTSpec:&kLedSpec flags:0];
	g_btn = [[OZGPIOInput alloc]
		initWithDTSpec:&kBtnSpec
			 flags:GPIO_INT_EDGE_TO_ACTIVE
		      callback:btn_c_callback];
	g_isr_count = 0;
	return g_btn != nil;
}

/* ── OZGPIOInput — block callback variant ──────────────────────── */

BOOL test_gpio_input_create_block(void)
{
	/* Ensure pin is inactive before configuring interrupt */
	gpio_emul_input_set(kBtnSpec.port, kBtnSpec.pin, 1);

	g_led = [[OZGPIOOutput alloc] initWithDTSpec:&kLedSpec flags:0];
	g_btn = [[OZGPIOInput alloc]
		initWithDTSpec:&kBtnSpec
			 flags:GPIO_INT_EDGE_TO_ACTIVE
		 blockCallback:^(const struct device *port, struct gpio_callback *cb,
				 gpio_port_pins_t pins) {
			g_isr_block_count++;
			[g_led toggle];
		 }];
	g_isr_block_count = 0;
	return g_btn != nil;
}

/* ── OZGPIOInput — capturing block (should be rejected) ────────── */

BOOL test_gpio_input_create_capturing_block(void)
{
	int local_var = 42;
	g_btn = [[OZGPIOInput alloc]
		initWithDTSpec:&kBtnSpec
			 flags:GPIO_INT_EDGE_TO_ACTIVE
		 blockCallback:^(const struct device *port, struct gpio_callback *cb,
				 gpio_port_pins_t pins) {
			printk("captured: %d\n", local_var);
		 }];
	return g_btn != nil;
}

BOOL test_gpio_input_is_active(void)
{
	return [g_btn isActive];
}

/* ── Block introspection helpers ───────────────────────────────── */

BOOL test_gpio_block_is_global(void)
{
	OZGPIOISRBlock blk = ^(const struct device *port, struct gpio_callback *cb,
			       gpio_port_pins_t pins) {
		printk("global block\n");
	};
	struct Block_layout *layout = (__bridge struct Block_layout *)blk;
	return (layout->flags & BLOCK_IS_GLOBAL) != 0;
}

BOOL test_gpio_capturing_block_is_not_global(void)
{
	int local_var = 7;
	OZGPIOISRBlock blk = ^(const struct device *port, struct gpio_callback *cb,
			       gpio_port_pins_t pins) {
		printk("captured: %d\n", local_var);
	};
	struct Block_layout *layout = (__bridge struct Block_layout *)blk;
	return (layout->flags & BLOCK_IS_GLOBAL) == 0;
}
