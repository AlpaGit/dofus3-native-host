cmake_minimum_required(VERSION 3.24)
project(DofusNativeBootstrap LANGUAGES CXX)

if(NOT WIN32 OR NOT CMAKE_SIZEOF_VOID_P EQUAL 8)
    message(FATAL_ERROR "DofusNativeBootstrap supports Windows x64 only")
endif()

add_library(DofusNativeBootstrap SHARED
    src/version_proxy.cpp
    src/version_proxy.def
)
target_compile_features(DofusNativeBootstrap PRIVATE cxx_std_20)
target_compile_definitions(DofusNativeBootstrap PRIVATE
    NOMINMAX
    WIN32_LEAN_AND_MEAN
    UNICODE
    _UNICODE
)
target_compile_options(DofusNativeBootstrap PRIVATE
    /W4
    /WX
    /permissive-
    /guard:cf
)
target_link_options(DofusNativeBootstrap PRIVATE /guard:cf)
set_target_properties(DofusNativeBootstrap PROPERTIES
    PREFIX ""
    OUTPUT_NAME "DofusNativeBootstrap"
)
