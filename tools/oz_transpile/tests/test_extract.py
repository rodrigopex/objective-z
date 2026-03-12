# SPDX-License-Identifier: Apache-2.0

from pathlib import Path

import pytest

from oz_transpile.extract import (
    extract_template,
    _extract_class_name,
    _extract_protocol_name,
    _extract_selector,
    _is_class_method,
    _loc_key,
    _impl_loc_key,
)
from tree_sitter import Language, Parser
import tree_sitter_objc as tsobjc

_TS_LANG = Language(tsobjc.language())


def _parse(source: str):
    """Parse ObjC source, return (bytes, tree)."""
    raw = source.encode()
    parser = Parser(_TS_LANG)
    tree = parser.parse(raw)
    return raw, tree


# ---------------------------------------------------------------------------
# Helper function tests
# ---------------------------------------------------------------------------


class TestLocKey:
    def test_loc_key_first_line(self):
        src, tree = _parse("#include \"foo.h\"\n")
        node = tree.root_node.children[0]
        assert _loc_key(node) == "_n_1_1"

    def test_loc_key_offset(self):
        src, tree = _parse("\n\nint x;\n")
        node = tree.root_node.children[0]
        assert _loc_key(node) == "_n_3_1"

    def test_impl_loc_key(self):
        src, tree = _parse("@implementation Foo\n@end\n")
        node = tree.root_node.children[0]
        assert _impl_loc_key(node) == "_impl_1_1"


class TestExtractClassName:
    def test_implementation(self):
        src, tree = _parse("@implementation Foo\n@end\n")
        node = tree.root_node.children[0]
        assert _extract_class_name(node) == "Foo"

    def test_interface(self):
        src, tree = _parse("@interface Bar: NSObject\n@end\n")
        node = tree.root_node.children[0]
        assert _extract_class_name(node) == "Bar"

    def test_category(self):
        src, tree = _parse("@implementation Car (Maintenance)\n@end\n")
        node = tree.root_node.children[0]
        name = _extract_class_name(node)
        assert name == "Car"


class TestExtractProtocolName:
    def test_protocol(self):
        src, tree = _parse("@protocol Drawable\n@end\n")
        node = tree.root_node.children[0]
        assert _extract_protocol_name(node) == "Drawable"


class TestExtractSelector:
    def test_unary(self):
        src, tree = _parse(
            "@implementation X\n- (void)greet { }\n@end\n"
        )
        impl = tree.root_node.children[0]
        for child in impl.children:
            if child.type == "implementation_definition":
                md = child.children[0]
                assert _extract_selector(md, src) == "greet"
                return
        pytest.fail("No method_definition found")

    def test_keyword(self):
        src, tree = _parse(
            "@implementation X\n"
            "- (void)setX:(int)x andY:(int)y { }\n"
            "@end\n"
        )
        impl = tree.root_node.children[0]
        for child in impl.children:
            if child.type == "implementation_definition":
                md = child.children[0]
                assert _extract_selector(md, src) == "setX:andY:"
                return
        pytest.fail("No method_definition found")


class TestIsClassMethod:
    def test_instance_method(self):
        src, tree = _parse(
            "@implementation X\n- (void)foo { }\n@end\n"
        )
        impl = tree.root_node.children[0]
        for child in impl.children:
            if child.type == "implementation_definition":
                md = child.children[0]
                assert _is_class_method(md) is False
                return
        pytest.fail("No method_definition found")

    def test_class_method(self):
        src, tree = _parse(
            "@implementation X\n+ (void)foo { }\n@end\n"
        )
        impl = tree.root_node.children[0]
        for child in impl.children:
            if child.type == "implementation_definition":
                md = child.children[0]
                assert _is_class_method(md) is True
                return
        pytest.fail("No method_definition found")


# ---------------------------------------------------------------------------
# extract_template tests
# ---------------------------------------------------------------------------


class TestExtractInterface:
    def test_interface_becomes_comment(self):
        src = b"@interface Foo: NSObject\n- (void)bar;\n@end\n"
        result = extract_template(src)
        assert "/* @interface Foo" in result
        assert "Foo_ozh.h" in result
        assert "@interface" not in result.replace("/* @interface", "")

    def test_interface_unnamed(self):
        """Edge case: if class name extraction fails."""
        result = extract_template(b"@interface Foo: NSObject\n@end\n")
        assert "/* @interface Foo" in result


class TestExtractProtocol:
    def test_protocol_becomes_comment(self):
        src = b"@protocol Drawable\n- (void)draw;\n@end\n"
        result = extract_template(src)
        assert "/* @protocol Drawable" in result
        assert "oz_dispatch.h" in result


class TestExtractImplementation:
    def test_basic_implementation(self):
        src = b"@implementation Foo\n- (void)bar { }\n@end\n"
        result = extract_template(src)
        assert "/* @implementation Foo */" in result
        assert "/* -[Foo bar] */" in result
        assert "/* @end Foo */" in result
        assert "{{ _impl_" in result
        assert "{{ _n_" in result

    def test_class_method_sign(self):
        src = b"@implementation Foo\n+ (void)bar { }\n@end\n"
        result = extract_template(src)
        assert "/* +[Foo bar] */" in result

    def test_instance_method_sign(self):
        src = b"@implementation Foo\n- (void)bar { }\n@end\n"
        result = extract_template(src)
        assert "/* -[Foo bar] */" in result

    def test_keyword_selector(self):
        src = (
            b"@implementation Foo\n"
            b"- (void)setX:(int)x andY:(int)y { }\n"
            b"@end\n"
        )
        result = extract_template(src)
        assert "/* -[Foo setX:andY:] */" in result

    def test_multiple_methods(self):
        src = (
            b"@implementation Foo\n"
            b"- (void)bar { }\n"
            b"- (void)baz { }\n"
            b"@end\n"
        )
        result = extract_template(src)
        assert "/* -[Foo bar] */" in result
        assert "/* -[Foo baz] */" in result
        # Two method placeholders
        assert result.count("{{ _n_") == 2

    def test_preamble_placeholder_always_present(self):
        src = b"@implementation Foo\n- (void)bar { }\n@end\n"
        result = extract_template(src)
        assert "{{ _impl_" in result

    def test_empty_implementation(self):
        src = b"@implementation Foo\n@end\n"
        result = extract_template(src)
        assert "/* @implementation Foo */" in result
        assert "{{ _impl_" in result
        assert "/* @end Foo */" in result
        # No method placeholders
        assert "{{ _n_" not in result


class TestExtractCategory:
    def test_category_no_parens_leaked(self):
        src = b"@implementation Car (Maintenance)\n- (int)mileage { return 0; }\n@end\n"
        result = extract_template(src)
        assert "/* @implementation Car */" in result
        assert "/* -[Car mileage] */" in result
        # Parentheses from category syntax must not appear
        lines = [l.strip() for l in result.splitlines() if l.strip()]
        for line in lines:
            if line.startswith("(") or line == ")":
                pytest.fail(f"Leaked parenthesis: {line!r}")


class TestExtractSynthesize:
    def test_synthesize_skipped(self):
        src = (
            b"@implementation Foo\n"
            b"@synthesize color = _color;\n"
            b"- (void)bar { }\n"
            b"@end\n"
        )
        result = extract_template(src)
        assert "@synthesize" not in result
        assert "/* -[Foo bar] */" in result


class TestExtractInstanceVariables:
    def test_ivars_skipped(self):
        src = (
            b"@implementation Foo {\n"
            b"\tint _x;\n"
            b"}\n"
            b"- (void)bar { }\n"
            b"@end\n"
        )
        result = extract_template(src)
        assert "int _x" not in result
        assert "/* -[Foo bar] */" in result


class TestExtractComments:
    def test_comments_between_methods_preserved(self):
        src = (
            b"@implementation Foo\n"
            b"- (void)bar { }\n"
            b"/* middle comment */\n"
            b"- (void)baz { }\n"
            b"@end\n"
        )
        result = extract_template(src)
        assert "/* middle comment */" in result

    def test_top_level_comment_preserved(self):
        src = b"/* top comment */\nint x;\n"
        result = extract_template(src)
        assert "/* top comment */" in result

    def test_comment_before_first_method_after_preamble(self):
        """Comments before the first method should appear AFTER the preamble
        placeholder, so generated preamble code doesn't split user comments."""
        src = (
            b"@implementation Foo\n"
            b"/* comment before first method */\n"
            b"- (void)bar { }\n"
            b"@end\n"
        )
        result = extract_template(src)
        preamble_pos = result.find("{{ _impl_")
        comment_pos = result.find("/* comment before first method */")
        assert preamble_pos >= 0
        assert comment_pos >= 0
        assert preamble_pos < comment_pos, \
            "preamble placeholder must come before comments"


class TestExtractPreprocessor:
    def test_top_level_include_placeholder(self):
        src = b'#include "foo.h"\n'
        result = extract_template(src)
        assert "{{ _n_1_1 }}" in result
        assert '#include "foo.h"' not in result

    def test_import_placeholder(self):
        src = b"#import <Foundation/Foundation.h>\n"
        result = extract_template(src)
        assert "{{ _n_" in result

    def test_preproc_inside_impl_preserved(self):
        src = (
            b"@implementation Foo\n"
            b"#ifdef DEBUG\n"
            b"- (void)bar { }\n"
            b"#endif\n"
            b"@end\n"
        )
        result = extract_template(src)
        assert "#ifdef DEBUG" in result
        assert "#endif" in result


class TestExtractFunctionDefinition:
    def test_function_becomes_placeholder(self):
        src = b"void my_func(void) { }\n"
        result = extract_template(src)
        assert "{{ _n_1_1 }}" in result
        assert "void my_func" not in result

    def test_function_after_impl(self):
        src = (
            b"@implementation Foo\n- (void)bar { }\n@end\n"
            b"void helper(void) { }\n"
        )
        result = extract_template(src)
        assert "/* -[Foo bar] */" in result
        # The function gets a placeholder too
        assert result.count("{{ _n_") == 2


class TestExtractDeclaration:
    def test_declaration_becomes_placeholder(self):
        src = b"int global_var = 42;\n"
        result = extract_template(src)
        assert "{{ _n_" in result
        assert "int global_var" not in result

    def test_macro_calls_pass_through(self):
        """Macro calls without definitions are expression_statement, not declaration.
        They pass through verbatim — context builder handles them via Clang AST."""
        src = b"ZBUS_MSG_SUBSCRIBER_DEFINE(msub);\n"
        result = extract_template(src)
        assert "ZBUS_MSG_SUBSCRIBER_DEFINE(msub);" in result


class TestExtractVerbatim:
    def test_plain_c_preserved(self):
        """Top-level C that isn't a recognized type passes through verbatim."""
        src = b";\n"
        result = extract_template(src)
        assert ";" in result

    def test_copyright_comment_preserved(self):
        src = (
            b"/* Copyright 2024 */\n"
            b"@implementation Foo\n- (void)bar { }\n@end\n"
        )
        result = extract_template(src)
        assert "/* Copyright 2024 */" in result


class TestExtractMultipleBlocks:
    def test_interface_then_implementation(self):
        src = (
            b"@interface Foo: NSObject\n- (void)bar;\n@end\n"
            b"@implementation Foo\n- (void)bar { }\n@end\n"
        )
        result = extract_template(src)
        assert "/* @interface Foo" in result
        assert "/* @implementation Foo */" in result
        assert "/* -[Foo bar] */" in result

    def test_two_implementations(self):
        src = (
            b"@implementation A\n- (void)x { }\n@end\n"
            b"@implementation B\n- (void)y { }\n@end\n"
        )
        result = extract_template(src)
        assert "/* @implementation A */" in result
        assert "/* @implementation B */" in result
        assert "/* -[A x] */" in result
        assert "/* -[B y] */" in result
        assert result.count("{{ _impl_") == 2

    def test_implementation_plus_free_function(self):
        src = (
            b"@implementation Foo\n- (void)bar { }\n@end\n"
            b"void helper(void) { return; }\n"
        )
        result = extract_template(src)
        assert "/* -[Foo bar] */" in result
        # One _n_ for the method, one _n_ for the function
        assert result.count("{{ _n_") == 2


class TestExtractRealWorld:
    """Tests based on actual sample files from the project."""

    def test_hello_world_style(self):
        src = (
            b"#import <Foundation/Foundation.h>\n"
            b"@interface MyObj: OZObject\n- (void)greet;\n+ (void)greet;\n@end\n"
            b"@implementation MyObj\n"
            b"- (void)greet { }\n"
            b"+ (void)greet { }\n"
            b"@end\n"
            b"int main(void) { return 0; }\n"
        )
        result = extract_template(src)
        assert "/* @interface MyObj" in result
        assert "/* -[MyObj greet] */" in result
        assert "/* +[MyObj greet] */" in result
        assert result.count("{{ _n_") == 4  # import, 2 methods, main

    def test_category_style(self):
        src = (
            b'#import "Car+Maintenance.h"\n'
            b"@implementation Car (Maintenance)\n"
            b"- (int)milage { return 100; }\n"
            b"- (int)oilCapacity { return 30; }\n"
            b"@end\n"
        )
        result = extract_template(src)
        assert "/* @implementation Car */" in result
        assert "/* -[Car milage] */" in result
        assert "/* -[Car oilCapacity] */" in result

    def test_zbus_style(self):
        src = (
            b'#include "Producer.h"\n'
            b'#include "channels.h"\n'
            b"#import <Foundation/OZLog.h>\n"
            b"#include <zephyr/kernel.h>\n"
            b"ZBUS_MSG_SUBSCRIBER_DEFINE(msub);\n"
            b"ZBUS_CHAN_ADD_OBS(chan, msub, 3);\n"
            b"@implementation Producer {\n\tint _count;\n}\n"
            b"@synthesize count = _count;\n"
            b"- (id)init { return self; }\n"
            b"- (void)sendData { }\n"
            b"@end\n"
            b"void thread_entry(void *a, void *b, void *c) { }\n"
            b"K_THREAD_DEFINE(tid, 4096, thread_entry, NULL, NULL, NULL, 3, 0, 0);\n"
        )
        result = extract_template(src)
        # Includes become placeholders
        assert '#include "Producer.h"' not in result
        # Macro calls pass through verbatim (tree-sitter sees expression_statement)
        assert "ZBUS_MSG_SUBSCRIBER_DEFINE" in result
        # Instance variables skipped
        assert "int _count" not in result
        # @synthesize skipped
        assert "@synthesize" not in result
        # Methods present
        assert "/* -[Producer init] */" in result
        assert "/* -[Producer sendData] */" in result
        # Free function becomes placeholder
        assert "void thread_entry" not in result


class TestExtractFixtures:
    """Tests using .m fixture files."""

    _FIXTURES = Path(__file__).parent / "fixtures"

    def test_basic_fixture(self):
        src = (self._FIXTURES / "extract_basic.m").read_bytes()
        result = extract_template(src)
        # Copyright comment preserved
        assert "/* Copyright 2024 Rodrigo Peixoto */" in result
        # Interface becomes comment
        assert "/* @interface OZBlinky" in result
        assert "OZBlinky_ozh.h" in result
        # Implementation annotated
        assert "/* @implementation OZBlinky */" in result
        assert "/* -[OZBlinky initWithPin:] */" in result
        assert "/* -[OZBlinky toggle] */" in result
        assert "/* @end OZBlinky */" in result
        # Comments between methods preserved
        assert "/* Initialize the blinky LED */" in result
        assert "/* Toggle the LED state */" in result
        # Comment inside method body NOT preserved (entire method is a placeholder)
        # Preamble placeholder present
        assert "{{ _impl_" in result
        # Includes become placeholders
        assert "#import" not in result

    def test_category_fixture(self):
        src = (self._FIXTURES / "extract_category.m").read_bytes()
        result = extract_template(src)
        assert "/* @implementation Car */" in result
        assert "/* -[Car mileage] */" in result
        assert "/* -[Car oilCapacity] */" in result
        assert "/* Oil capacity in liters */" in result
        # No parentheses leaked
        assert "Maintenance" not in result

    def test_multiclass_fixture(self):
        src = (self._FIXTURES / "extract_multiclass.m").read_bytes()
        result = extract_template(src)
        # Includes become placeholders
        assert '#include "Producer.h"' not in result
        assert '#include "channels.h"' not in result
        # Interface becomes comment
        assert "/* @interface AccDataProducer" in result
        # Implementation
        assert "/* @implementation AccDataProducer */" in result
        assert "/* -[AccDataProducer init] */" in result
        assert "/* -[AccDataProducer sendData] */" in result
        # @synthesize and ivars skipped
        assert "@synthesize" not in result
        assert "int _count" not in result
        # Free function becomes placeholder
        assert "void thread_entry_producer" not in result
        # Macro calls pass through
        assert "ZBUS_MSG_SUBSCRIBER_DEFINE" in result
        assert "K_THREAD_DEFINE" in result
