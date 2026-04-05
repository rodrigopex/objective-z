# Regression Tests

Each file in this directory prevents a specific bug from reoccurring.

## Naming Convention

    issue_NNN_short_description.m

where NNN is the GitHub issue number (or a sequential local number
if no issue exists).

## Required Header

Every regression test MUST start with:

    /*
     * Regression test for issue #NNN.
     *
     * Bug: <one-line description of the incorrect behavior>
     * Fix: <one-line description of what was fixed>
     * Commit: <hash of the fix commit>
     */

## Workflow

1. Reproduce the bug in a minimal .m file
2. Verify it fails (transpiler error, wrong output, or crash)
3. Fix the transpiler
4. Name the file `issue_NNN_description.m`, add the header
5. Verify it passes: `just test-behavior`
6. Commit the test alongside the fix

## Quick Start

    ./scripts/new_regression_test.sh 42 "nil struct return"
