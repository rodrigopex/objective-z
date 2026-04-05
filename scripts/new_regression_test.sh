#!/bin/bash
# Usage: ./scripts/new_regression_test.sh 42 "nil struct return"
# Creates: tests/behavior/cases/regression/issue_042_nil_struct_return.m
#          tests/behavior/cases/regression/issue_042_nil_struct_return_test.c

set -euo pipefail

if [ $# -lt 2 ]; then
    echo "Usage: $0 <issue_number> <short description>"
    echo "Example: $0 42 \"nil struct return\""
    exit 1
fi

NUM=$(printf "%03d" "$1")
DESC=$(echo "$2" | tr ' ' '_')
DIR="tests/behavior/cases/regression"
M_FILE="${DIR}/issue_${NUM}_${DESC}.m"
C_FILE="${DIR}/issue_${NUM}_${DESC}_test.c"

cat > "$M_FILE" << EOF
/*
 * Regression test for issue #$1.
 *
 * Bug: TODO -- describe the incorrect behavior
 * Fix: TODO -- describe what was fixed
 * Commit: TODO -- hash of the fix commit
 */
#import "OZTestBase.h"

@interface RegressionTest${NUM} : OZObject
@end

@implementation RegressionTest${NUM}
@end
EOF

cat > "$C_FILE" << EOF
/* Regression test for issue #$1 */
#include "unity.h"
#include "RegressionTest${NUM}_ozh.h"

void test_issue_${NUM}(void)
{
	/* TODO: Add test body */
	TEST_PASS();
}
EOF

echo "Created: $M_FILE"
echo "Created: $C_FILE"
