if(CONFIG_OBJZ)
    include(${ZEPHYR_CURRENT_MODULE_DIR}/cmake/ObjcClang.cmake)

    if(CONFIG_OBJZ_USE_CLANG)
        objz_find_clang()
    endif()

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

    # STATIC_INIT_GNU creates segments that newer binutils (>=2.39) flag as
    # having RWX permissions. Suppress the linker warning.
    zephyr_link_libraries(-Wl,--no-warn-rwx-segments)

    if(CONFIG_OBJZ_USE_CLANG)
        # libgcc objects (unwind-arm.o, _arm_fixunsdfsi.o) trigger linker
        # warnings that --fatal-warnings (used by twister) turns into errors.
        zephyr_link_libraries(-Wl,--no-warn-execstack)
        zephyr_link_libraries(-Wl,--orphan-handling=place)
    endif()
endif()
