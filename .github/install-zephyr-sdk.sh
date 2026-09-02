#!/bin/bash
# SPDX-License-Identifier: Apache-2.0
#
# Install the Zephyr SDK with its LLVM component, and make that clang the
# one every mechanism in this repo finds.
#
# Shared by every CI job that needs a clang, which is most of them: the
# Python pipeline and oz_static's `--ast` both read a Clang JSON AST, and
# that AST decides ivar ownership and method definedness in the generated
# C. There is one script rather than eight inline blocks because eight
# inline blocks is how the workflow ended up with three different clangs
# at once (#269).
#
# The `-l` is the point. Both `west sdk install` and this setup.sh install
# the GNU toolchains only; LLVM is a separate opt-in download. Without it
# `$ZEPHYR_SDK_INSTALL_DIR/llvm/bin/clang` does not exist, so
# `objz_find_clang()` falls past its priority-2 entry and takes whatever
# `clang` is on PATH -- which on an Ubuntu runner is the preinstalled
# 18.1, not the tested 19.
#
# Expects ZEPHYR_SDK_VERSION and ZEPHYR_SDK_INSTALL_DIR from the
# workflow's env. Idempotent: skips the download on a cache hit, but
# always rewrites the symlink, which the cache does not carry.

set -euo pipefail

: "${ZEPHYR_SDK_VERSION:?must be set by the workflow}"
: "${ZEPHYR_SDK_INSTALL_DIR:?must be set by the workflow}"

HOST_ARCH="linux-x86_64"
BUNDLE="zephyr-sdk-${ZEPHYR_SDK_VERSION}_${HOST_ARCH}_minimal.tar.xz"
BASE="https://github.com/zephyrproject-rtos/sdk-ng/releases/download/v${ZEPHYR_SDK_VERSION}"

if [ ! -d "${ZEPHYR_SDK_INSTALL_DIR}" ]; then
        echo "Installing Zephyr SDK ${ZEPHYR_SDK_VERSION} (arm, riscv64, LLVM)"
        wget -q "${BASE}/${BUNDLE}"
        tar xf "${BUNDLE}" -C "$(dirname "${ZEPHYR_SDK_INSTALL_DIR}")"
        rm -f "${BUNDLE}"
        # -t toolchain  -l LLVM  -h host tools  -c register CMake package
        "${ZEPHYR_SDK_INSTALL_DIR}/setup.sh" \
                -t arm-zephyr-eabi \
                -t riscv64-zephyr-elf \
                -l -h -c
else
        echo "Zephyr SDK ${ZEPHYR_SDK_VERSION} restored from cache"
fi

CLANG="${ZEPHYR_SDK_INSTALL_DIR}/llvm/bin/clang"
if [ ! -x "${CLANG}" ]; then
        echo "error: ${CLANG} is missing -- setup.sh ran without -l," >&2
        echo "       so every clang lookup in this repo would silently" >&2
        echo "       fall back to whatever is on PATH. See #269." >&2
        exit 1
fi

# Two consumers, one answer. `objz_find_clang()` finds this through
# ZEPHYR_SDK_INSTALL_DIR; the Python harnesses read OZ_CLANG, which the
# workflow sets. The symlink covers the third case: a bare `clang` on
# PATH, which is both `--compiler=clang` in the behavior matrix and the
# Python harness's own version search.
sudo ln -sf "${CLANG}" /usr/local/bin/clang

echo "clang for this job: $(clang --version | head -1)"
echo "                    $(command -v clang)"
