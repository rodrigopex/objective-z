if(CONFIG_OBJZ)
    include(${ZEPHYR_CURRENT_MODULE_DIR}/cmake/ObjcClang.cmake)
    objz_find_clang()

    zephyr_library()
    zephyr_include_directories("${ZEPHYR_CURRENT_MODULE_DIR}/include")

    # C files: always GCC
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/malloc.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/module.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/category.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/class.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/dtable.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/hash.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/message.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/protocol.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/statics.c")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/runtime.c")

    # ObjC files: Clang if enabled, else GCC
    objz_compile_objc_sources(${ZEPHYR_CURRENT_LIBRARY}
        "${ZEPHYR_CURRENT_MODULE_DIR}/src/init.m"
        "${ZEPHYR_CURRENT_MODULE_DIR}/src/NXConstantString.m"
        "${ZEPHYR_CURRENT_MODULE_DIR}/src/Object.m"
        "${ZEPHYR_CURRENT_MODULE_DIR}/src/Protocol.m"
    )

    # gnustep-1.7 dispatch support (Clang uses objc_msgSend instead of
    # objc_msg_lookup + indirect call, and objc_slot_lookup_super for [super ...])
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/objc_msgSend.S")
    zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/slot.c")

    # MRR memory management layer
    if(CONFIG_OBJZ_MRR)
        zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/refcount.c")
        objz_compile_objc_sources(${ZEPHYR_CURRENT_LIBRARY}
            "${ZEPHYR_CURRENT_MODULE_DIR}/src/OZObject.m"
            "${ZEPHYR_CURRENT_MODULE_DIR}/src/OZAutoreleasePool.m"
        )
    endif()

    # ARC entry points (pure C, no -fobjc-arc)
    if(CONFIG_OBJZ_ARC)
        zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/arc.c")
    endif()

    # Static allocation pools
    if(CONFIG_OBJZ_STATIC_POOLS)
        zephyr_library_sources("${ZEPHYR_CURRENT_MODULE_DIR}/src/pool.c")
    endif()

    # STATIC_INIT_GNU creates segments that newer binutils (>=2.39) flag as
    # having RWX permissions. Suppress the linker warning.
    zephyr_link_libraries(-Wl,--no-warn-rwx-segments)

    # libgcc objects (unwind-arm.o, _arm_fixunsdfsi.o) trigger linker
    # warnings that --fatal-warnings (used by twister) turns into errors.
    zephyr_link_libraries(-Wl,--no-warn-execstack)
    zephyr_link_libraries(-Wl,--orphan-handling=place)
endif()
