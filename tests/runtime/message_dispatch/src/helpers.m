/**
 * @file helpers.m
 * @brief ObjC helper classes for message dispatch tests.
 */
#import <objc/objc.h>

/* ── Counters for +initialize tracking (read from C test code) ──── */
int g_animal_init_count = 0;
int g_dog_init_count = 0;

/* ── TestAnimal: root subclass of Object ────────────────────────── */

@interface TestAnimal : Object
+ (int)classValue;
- (int)speak;
- (int)legCount;
@end

@implementation TestAnimal

+ (void)initialize
{
	g_animal_init_count++;
}

+ (int)classValue
{
	return 42;
}

- (int)speak
{
	return 1;
}

- (int)legCount
{
	return 4;
}

@end

/* ── TestDog: subclass of TestAnimal ────────────────────────────── */

@interface TestDog : TestAnimal
- (int)speak;
- (int)fetch;
@end

@implementation TestDog

+ (void)initialize
{
	g_dog_init_count++;
}

- (int)speak
{
	return 2;
}

- (int)fetch
{
	return 99;
}

@end

/* ── C-callable helpers ─────────────────────────────────────────── */

id test_create_animal(void)
{
	return [[TestAnimal alloc] init];
}

id test_create_dog(void)
{
	return [[TestDog alloc] init];
}

void test_dealloc(id obj)
{
	[obj dealloc];
}

int test_call_speak(id obj)
{
	return [obj speak];
}

int test_call_legCount(id obj)
{
	return [obj legCount];
}

int test_call_classValue_on_class(Class cls)
{
	return [(id)cls classValue];
}

int test_call_fetch(id obj)
{
	return [obj fetch];
}

BOOL test_call_respondsToSelector_speak(id obj)
{
	return [obj respondsToSelector:@selector(speak)];
}

BOOL test_call_respondsToSelector_fetch(id obj)
{
	return [obj respondsToSelector:@selector(fetch)];
}
