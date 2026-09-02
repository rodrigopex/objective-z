#!/bin/bash
# SPDX-License-Identifier: Apache-2.0
#
# Install a *host* clang 19 and make it the one every mechanism finds.
#
# There are deliberately two clangs in this workflow, and the distinction
# is the target, not the version:
#
#   this script            clang 19 built for x86_64 Linux, for jobs that
#                          dump ASTs of code compiled and run on the
#                          runner (the behavior corpus, the adapted
#                          tests, the Python pipeline's own suites)
#
#   install-zephyr-sdk.sh  the Zephyr SDK's clang 19, a cross toolchain,
#                          for jobs that build for a board
#
# The first attempt at #269 used the SDK's clang everywhere, on the
# reasoning that one clang beats three. That was wrong and CI said so
# within two minutes: the SDK's clang picks up `/usr/include/stdint.h`
# and then cannot find `bits/libc-header-start.h`, because on Debian
# multiarch that header is in `/usr/include/x86_64-linux-gnu/` and a
# cross-built clang has no host gcc installation to learn that from.
# 276 of 539 Python tests failed on it.
#
# So: same version on both sides, which is what actually mattered --
# `cmake/ObjcClang.cmake` names 19 as the tested one -- and each side
# targeting what it is for.

set -euo pipefail

CLANG_VERSION="${CLANG_VERSION:-19}"
CODENAME="$(lsb_release -cs)"

if ! command -v "clang-${CLANG_VERSION}" > /dev/null; then
        wget -qO- https://apt.llvm.org/llvm-snapshot.gpg.key \
                | sudo tee /etc/apt/trusted.gpg.d/llvm.asc > /dev/null
        echo "deb http://apt.llvm.org/${CODENAME}/ llvm-toolchain-${CODENAME}-${CLANG_VERSION} main" \
                | sudo tee "/etc/apt/sources.list.d/llvm-${CLANG_VERSION}.list" > /dev/null
        sudo apt-get update
        sudo apt-get install -y "clang-${CLANG_VERSION}"
fi

# Three consumers, one answer. `--compiler=clang` in the behavior matrix
# and the Python harness's own bare-`clang` fallback both resolve through
# PATH; OZ_CLANG (set per job) names it outright, which is what the
# harnesses prefer. Explicit beats a search order -- relying on one is
# how this workflow ended up dumping ASTs with a clang nobody picked.
sudo ln -sf "/usr/bin/clang-${CLANG_VERSION}" /usr/local/bin/clang

echo "host clang for this job: $(clang --version | head -1)"
echo "                         $(command -v clang)"
