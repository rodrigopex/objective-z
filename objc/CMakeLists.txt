if(CONFIG_OBJZ)
    include(${ZEPHYR_CURRENT_MODULE_DIR}/cmake/ObjcClang.cmake)
    objz_find_clang()

    zephyr_library()
    zephyr_include_directories("${ZEPHYR_CURRENT_MODULE_DIR}/include")

    # C files: always GCC
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/malloc.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/load.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/category.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/class.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/dtable.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/hash.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/message.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/protocol.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/runtime.c")

    # ObjC files: Clang if enabled, else GCC
    objz_compile_objc_sources(${ZEPHYR_CURRENT_LIBRARY}
        "${ZEPHYR_CURRENT_MODULE_DIR}/src/init.m"
        "${ZEPHYR_CURRENT_MODULE_DIR}/src/OZString.m"
        "${ZEPHYR_CURRENT_MODULE_DIR}/src/Object.m"
        "${ZEPHYR_CURRENT_MODULE_DIR}/src/Protocol.m"
    )

    # gnustep-2.0 dispatch: objc_msgSend trampoline (ARM Thumb-2)
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/objc_msgSend.S")

    # MRR memory management layer
    if(CONFIG_OBJZ_MRR)
        zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/refcount.c")
        objz_compile_objc_sources(${ZEPHYR_CURRENT_LIBRARY}
            "${ZEPHYR_CURRENT_MODULE_DIR}/src/OZObject.m"
            "${ZEPHYR_CURRENT_MODULE_DIR}/src/OZAutoreleasePool.m"
            "${ZEPHYR_CURRENT_MODULE_DIR}/src/OZMutableString.m"
            "${ZEPHYR_CURRENT_MODULE_DIR}/src/OZLog.m"
        )
    endif()

    # ARC entry points (pure C, no -fobjc-arc)
    if(CONFIG_OBJZ_ARC)
        zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/arc.c")
    endif()

    # Boxed literals and collection literals
    if(CONFIG_OBJZ_LITERALS)
        objz_compile_objc_sources(${ZEPHYR_CURRENT_LIBRARY}
            "${ZEPHYR_CURRENT_MODULE_DIR}/src/OZNumber.m"
            "${ZEPHYR_CURRENT_MODULE_DIR}/src/OZArray.m"
            "${ZEPHYR_CURRENT_MODULE_DIR}/src/OZDictionary.m"
        )
    endif()

    # Static allocation pools
    if(CONFIG_OBJZ_STATIC_POOLS)
        zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/pool.c")
    endif()

    # gnustep-2.0 ObjC metadata sections (__objc_selectors, __objc_classes, etc.)
    zephyr_linker_sources(SECTIONS ${ZEPHYR_CURRENT_MODULE_DIR}/cmake/objc_sections.ld)

    # STATIC_INIT_GNU creates segments that newer binutils (>=2.39) flag as
    # having RWX permissions. Suppress the linker warning.
    zephyr_link_libraries(-Wl,--no-warn-rwx-segments)

    # libgcc objects (unwind-arm.o, _arm_fixunsdfsi.o) trigger linker
    # warnings that --fatal-warnings (used by twister) turns into errors.
    zephyr_link_libraries(-Wl,--no-warn-execstack)
    zephyr_link_libraries(-Wl,--orphan-handling=place)
endif()
