if(CONFIG_OBJZ)
    # oz_static.cmake defines objz_transpile_sources(), the entry point every
    # sample calls. This used to point at oz_transpile.cmake, which dispatched
    # between the Rust and Python backends; the Python one is retired (see the
    # `python-backend-final` tag) so there is nothing left to dispatch on.
    include(${ZEPHYR_OBJZ_MODULE_DIR}/cmake/oz_static.cmake)
endif()
