if(CONFIG_OBJZ)
    # oz2c.cmake defines objz_transpile_sources(), the entry point every
    # sample calls. This used to point at oz_transpile.cmake, which dispatched
    # between the Rust and Python backends; the Python one is retired (see the
    # `python-backend-final` tag) so there is nothing left to dispatch on.
    include(${ZEPHYR_OBJZ_MODULE_DIR}/cmake/oz2c.cmake)
else()
    # Stubs that explain themselves, for the case where an application calls
    # the entry point with CONFIG_OBJZ off (#611).
    #
    # Without these the call is simply an unknown command, and CMake reports
    # `Unknown CMake command "objz_transpile_sources"` pointing at the
    # application's own line. That names the consequence and not the cause:
    # the reason the function does not exist is that the `include()` above did
    # not run, which is hundreds of log lines away and shares no vocabulary
    # with the error. A reader new to this concludes the sample or the module
    # installation is broken.
    #
    # It has to be a stub rather than a bare message(FATAL_ERROR) in this
    # `else()`. A Zephyr module's CMakeLists.txt is processed for *every*
    # application in the workspace, so erroring here would break every build
    # that does not use Objective-Z at all. Only a call is evidence that
    # someone wanted the transpiler, so only a call may fail.
    #
    # Both names are stubbed. `objz_transpile_sources()` is what the 18
    # samples and the benchmarks call, and `objz_transpile_sources_static()`
    # is documented as public in cmake/oz2c.cmake even though nothing
    # in-tree calls it, so an out-of-tree consumer may.
    macro(_objz_unavailable fn)
        message(FATAL_ERROR
            "${fn}() requires CONFIG_OBJZ, which is not enabled for board "
            "'${BOARD}'.\n"
            "Add CONFIG_OBJZ=y to the application's prj.conf, or to a "
            "board-specific .conf. If it is already set there, Kconfig could "
            "not honour it: look earlier in this log for a warning naming "
            "OBJZ and the dependency it could not satisfy.")
    endmacro()

    function(objz_transpile_sources target)
        _objz_unavailable(objz_transpile_sources)
    endfunction()

    function(objz_transpile_sources_static target)
        _objz_unavailable(objz_transpile_sources_static)
    endfunction()
endif()
