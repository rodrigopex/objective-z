/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file helpers.m
 * @brief ObjC helper classes with protocols for protocol tests.
 */
#import <Foundation/Object.h>

/* ── Protocols ───────────────────────────────────────────────────── */

@protocol TestDrawable
- (int)draw;
@end

@protocol TestResizable
- (int)resize:(int)factor;
@end

/* ── TestWidget: conforms to TestDrawable ─────────────────────────── */

@interface TestWidget : Object <TestDrawable> {
	int _id;
}
- (id)initWithId:(int)wid;
- (int)widgetId;
@end

@implementation TestWidget

- (id)initWithId:(int)wid
{
	self = [super init];
	if (self) {
		_id = wid;
	}
	return self;
}

- (int)widgetId
{
	return _id;
}

- (int)draw
{
	return _id * 10;
}

@end

/* ── TestButton: subclass of TestWidget, also conforms to TestResizable */

@interface TestButton : TestWidget <TestResizable>
@end

@implementation TestButton

- (int)resize:(int)factor
{
	return [self widgetId] * factor;
}

@end

/* ── TestLabel: conforms to neither protocol ──────────────────────── */

@interface TestLabel : Object {
	int _text;
}
- (id)initWithText:(int)text;
- (int)text;
@end

@implementation TestLabel

- (id)initWithText:(int)text
{
	self = [super init];
	if (self) {
		_text = text;
	}
	return self;
}

- (int)text
{
	return _text;
}

@end

/* ── C-callable wrappers ─────────────────────────────────────────── */

id test_proto_create_widget(int wid)
{
	return [[TestWidget alloc] initWithId:wid];
}

id test_proto_create_button(int wid)
{
	return [[TestButton alloc] initWithId:wid];
}

id test_proto_create_label(int text)
{
	return [[TestLabel alloc] initWithText:text];
}

int test_proto_draw(id obj)
{
	return [(id<TestDrawable>)obj draw];
}

int test_proto_resize(id obj, int factor)
{
	return [(id<TestResizable>)obj resize:factor];
}

int test_proto_widget_id(id obj)
{
	return [(TestWidget *)obj widgetId];
}

BOOL test_proto_widget_conforms_drawable(id obj)
{
	return [obj conformsTo:@protocol(TestDrawable)];
}

BOOL test_proto_widget_conforms_resizable(id obj)
{
	return [obj conformsTo:@protocol(TestResizable)];
}

BOOL test_proto_class_conforms_drawable(const char *name)
{
	Class cls = objc_lookupClass(name);
	if (cls == Nil) {
		return NO;
	}
	return [cls conformsTo:@protocol(TestDrawable)];
}

BOOL test_proto_class_conforms_resizable(const char *name)
{
	Class cls = objc_lookupClass(name);
	if (cls == Nil) {
		return NO;
	}
	return [cls conformsTo:@protocol(TestResizable)];
}

void test_proto_dealloc(id obj)
{
	[obj dealloc];
}
