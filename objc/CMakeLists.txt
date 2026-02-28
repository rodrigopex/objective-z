if(CONFIG_OBJZ)
    include(${ZEPHYR_CURRENT_MODULE_DIR}/cmake/ObjcClang.cmake)
    objz_find_clang()

    zephyr_library()
    zephyr_include_directories("${ZEPHYR_CURRENT_MODULE_DIR}/include")

    # ── Version ──────────────────────────────────────────────────────
    file(READ "${ZEPHYR_CURRENT_MODULE_DIR}/VERSION" _objz_ver)
    set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS
        "${ZEPHYR_CURRENT_MODULE_DIR}/VERSION")

    string(REGEX MATCH "VERSION_MAJOR = ([0-9]*)" _ ${_objz_ver})
    set(OBJZ_VERSION_MAJOR ${CMAKE_MATCH_1})

    string(REGEX MATCH "VERSION_MINOR = ([0-9]*)" _ ${_objz_ver})
    set(OBJZ_VERSION_MINOR ${CMAKE_MATCH_1})

    string(REGEX MATCH "PATCHLEVEL = ([0-9]*)" _ ${_objz_ver})
    set(OBJZ_PATCHLEVEL ${CMAKE_MATCH_1})

    set(OBJZ_VERSION_STRING "${OBJZ_VERSION_MAJOR}.${OBJZ_VERSION_MINOR}.${OBJZ_PATCHLEVEL}")
    message(STATUS "Objective-Z: v${OBJZ_VERSION_STRING}")

    set(_objz_generated_dir "${CMAKE_CURRENT_BINARY_DIR}/include/generated")
    configure_file(
        "${ZEPHYR_CURRENT_MODULE_DIR}/cmake/version.h.in"
        "${_objz_generated_dir}/objc/version.h"
        @ONLY
    )
    zephyr_include_directories("${_objz_generated_dir}")

    # C files: always GCC
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/malloc.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/load.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/category.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/class.c")
    if(CONFIG_OBJZ_DISPATCH_CACHE)
        zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/dtable.c")
    endif()
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/hash.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/message.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/protocol.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/runtime.c")

    # ObjC files: Clang if enabled, else GCC
    objz_compile_objc_sources(${ZEPHYR_CURRENT_LIBRARY}
        "${ZEPHYR_CURRENT_MODULE_DIR}/src/init.m"
        "${ZEPHYR_CURRENT_MODULE_DIR}/src/Foundation/OZString.m"
        "${ZEPHYR_CURRENT_MODULE_DIR}/src/Foundation/Object.m"
        "${ZEPHYR_CURRENT_MODULE_DIR}/src/Foundation/Protocol.m"
    )

    # gnustep-2.0 dispatch: objc_msgSend trampoline (ARM Thumb-2)
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/objc_msgSend.S")

    # Reference counting and autorelease pool
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/refcount.c")
    objz_compile_objc_sources(${ZEPHYR_CURRENT_LIBRARY}
        "${ZEPHYR_CURRENT_MODULE_DIR}/src/Foundation/OZAutoreleasePool.m"
        "${ZEPHYR_CURRENT_MODULE_DIR}/src/Foundation/OZMutableString.m"
        "${ZEPHYR_CURRENT_MODULE_DIR}/src/Foundation/OZLog.m"
    )

    # ARC entry points (pure C, no -fobjc-arc)
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/arc.c")

    # Boxed literals and collection literals
    if(CONFIG_OBJZ_LITERALS)
        objz_compile_objc_sources(${ZEPHYR_CURRENT_LIBRARY}
            "${ZEPHYR_CURRENT_MODULE_DIR}/src/Foundation/OZNumber.m"
            "${ZEPHYR_CURRENT_MODULE_DIR}/src/Foundation/OZArray.m"
            "${ZEPHYR_CURRENT_MODULE_DIR}/src/Foundation/OZDictionary.m"
        )
    endif()

    # Blocks (closures) runtime
    if(CONFIG_OBJZ_BLOCKS)
        zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/blocks.c")
    endif()

    # Static allocation pools
    if(CONFIG_OBJZ_STATIC_POOLS)
        zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/pool.c")
    endif()

    # gnustep-2.0 ObjC metadata sections (__objc_selectors, __objc_classes, etc.)
    zephyr_linker_sources(DATA_SECTIONS ${ZEPHYR_CURRENT_MODULE_DIR}/cmake/objc_sections.ld)

    # STATIC_INIT_GNU creates segments that newer binutils (>=2.39) flag as
    # having RWX permissions. Suppress the linker warning.
    zephyr_link_libraries(-Wl,--no-warn-rwx-segments)

    # libgcc objects (unwind-arm.o, _arm_fixunsdfsi.o) trigger linker
    # warnings that --fatal-warnings (used by twister) turns into errors.
    zephyr_link_libraries(-Wl,--no-warn-execstack)
    zephyr_link_libraries(-Wl,--orphan-handling=place)
endif()
