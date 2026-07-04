cmake_minimum_required(VERSION 3.20)
project(plug LANGUAGES C CXX)

if(WIN32)
    enable_language(RC)
endif()

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

set(ROOT_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../..")
set(APP_DIR "${ROOT_DIR}/plug.app")
set(COMMON_DIR "${APP_DIR}/cmn")

if(NOT DEFINED RUST_LIB_PATH)
    message(FATAL_ERROR "RUST_LIB_PATH must be defined!")
endif()

add_library(rust_core STATIC IMPORTED)
set_target_properties(rust_core PROPERTIES IMPORTED_LOCATION "${RUST_LIB_PATH}")

set(SOURCES
    "${APP_DIR}/src/main.cpp"
    "${APP_DIR}/srv/sys/init/admin.c"
    "${APP_DIR}/srv/sys/init/prov.c"
    "${APP_DIR}/src/windows/api/intf/main_w.cpp"
    "${APP_DIR}/src/windows/api/intf/cmp/u_prmt.cpp"
    "${APP_DIR}/src/windows/api/intf/cmp/u_lbl.cpp"
    "${APP_DIR}/src/windows/api/intf/cmp/caret/caret_.cpp"
    "${APP_DIR}/src/windows/api/intf/cmp/tab/tab_.cpp"
    "${APP_DIR}/src/windows/api/intf/cmp/scrollbar/scrollb_.cpp"
    "${APP_DIR}/src/windows/api/intf/cmp/clipboard/clipb_.cpp"
    "${APP_DIR}/src/windows/api/intf/cmp/effects/m_hover.cpp"
    "${APP_DIR}/src/windows/api/intf/cmp/highlight/selection_.cpp"
)

# Thêm RC file đã generate
if(WIN32 AND EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/plug_manifest.rc")
    list(APPEND SOURCES "${CMAKE_CURRENT_SOURCE_DIR}/plug_manifest.rc")
endif()

if(HIDE_CONSOLE)
    add_executable(plug WIN32 ${SOURCES})
else()
    add_executable(plug ${SOURCES})
endif()

target_include_directories(plug PRIVATE
    "${APP_DIR}/inc"
    "${COMMON_DIR}/inc"
    "${COMMON_DIR}/inc/sys/api/cmp"
)

if(MSVC)
    target_compile_options(plug PRIVATE /O2 /Gw /Gy /MT /wd4068)
    target_link_options(plug PRIVATE /OPT:REF /OPT:ICF /LTCG)
    
    target_link_libraries(plug PRIVATE
        rust_core
        kernel32 user32 gdi32 advapi32 crypt32 shell32 ole32 oleaut32 uuid
        psapi shlwapi ws2_32 userenv ntdll iphlpapi bcrypt secur32 dbghelp
        comctl32 urlmon wininet msimg32
        bcrypt userenv ws2_32
        ntdll
    )
else()
    target_compile_options(plug PRIVATE -Os -ffunction-sections -fdata-sections -flto)
    target_link_options(plug PRIVATE -static -static-libgcc -static-libstdc++ -Wl,--gc-sections -flto)
    
    if(CMAKE_CXX_COMPILER_ID STREQUAL "Clang")
        target_link_options(plug PRIVATE -fuse-ld=lld)
    endif()

    target_link_libraries(plug PRIVATE
        rust_core
        winpthread
        kernel32 user32 gdi32 winspool comdlg32 advapi32 crypt32 shell32 ole32 oleaut32 uuid
        psapi shlwapi ws2_32 userenv ntdll iphlpapi bcrypt secur32 dbghelp
        comctl32 urlmon wininet msimg32
        ntdll
    )
endif()
