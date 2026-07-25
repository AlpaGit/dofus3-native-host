mod manager_ui;
mod marketplace;

use dofus_native_mod_api::{
    DNH_ABI_VERSION_1, DNH_ABI_VERSION_2, DNH_ABI_VERSION_3, DNH_ABI_VERSION_4, DNH_ABI_VERSION_5,
    DNH_ABI_VERSION_6, DNH_ERROR, DNH_OK, DnhClassInfoV2, DnhClassSignatureV3, DnhFieldInfoV2,
    DnhFieldSignatureV3, DnhGcHandleV4, DnhHookApiV5, DnhHostApiV1, DnhHostApiV2, DnhHostApiV3,
    DnhHostApiV4, DnhHostApiV5, DnhHostApiV6, DnhLogLevel, DnhMethodInfoV2, DnhMethodSignatureV3,
    DnhModInfoV1, DnhUnityApiV1, DnhUnityApiV2, DnhUnityApiV3, DnhUnityApiV4, DnhUnityApiV6,
    DnmLoadFn, DnmQueryFn, DnmTickFn, DnmUnloadFn, LOAD_EXPORT, QUERY_EXPORT, TICK_EXPORT,
    UNLOAD_EXPORT,
};
use manager_ui::{InstalledModView, UiAction, UiController, UiSnapshot};
use marketplace::{MarketplaceMod, WorkerResult};
use minhook::MinHook;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CStr, OsString, c_char, c_void};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::mem::size_of;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

type HModule = *mut c_void;
type Bool = i32;
type DWord = u32;

const DLL_PROCESS_ATTACH: DWord = 1;
const GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT: DWord = 0x0000_0002;
const GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS: DWord = 0x0000_0004;
const LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR: DWord = 0x0000_0100;
const LOAD_LIBRARY_SEARCH_DEFAULT_DIRS: DWord = 0x0000_1000;
const MAX_PATH_CHARS: usize = 32_768;
const VK_F1: i32 = 0x70;

unsafe extern "system" {
    fn DisableThreadLibraryCalls(module: HModule) -> Bool;
    fn GetModuleFileNameW(module: HModule, filename: *mut u16, size: DWord) -> DWord;
    fn GetModuleHandleExW(flags: DWord, module_name: *const u16, module: *mut HModule) -> Bool;
    fn GetModuleHandleW(module_name: *const u16) -> HModule;
    fn LoadLibraryExW(filename: *const u16, file: HModule, flags: DWord) -> HModule;
    fn GetProcAddress(module: HModule, proc_name: *const c_char) -> *mut c_void;
    fn FreeLibrary(module: HModule) -> Bool;
    fn OutputDebugStringA(message: *const c_char);
}

unsafe extern "C" {
    fn dnh_unity_initialize(game_assembly: HModule) -> bool;
    fn dnh_unity_is_ready() -> bool;
    fn dnh_unity_thread_attach() -> HModule;
    fn dnh_unity_thread_detach(thread: HModule);
    fn dnh_unity_get_class(
        assembly: *const c_char,
        namespace_name: *const c_char,
        class_name: *const c_char,
    ) -> HModule;
    fn dnh_unity_get_field(class_handle: HModule, field_name: *const c_char) -> HModule;
    fn dnh_unity_get_method(
        class_handle: HModule,
        method_name: *const c_char,
        parameter_count: i32,
    ) -> HModule;
    fn dnh_unity_field_offset(field_handle: HModule) -> i32;
    fn dnh_unity_method_address(method_handle: HModule) -> HModule;
    fn dnh_unity_find_objects(
        class_handle: HModule,
        output: *mut HModule,
        capacity: usize,
    ) -> usize;
    fn dnh_unity_get_classes(
        assembly: *const c_char,
        output: *mut HModule,
        capacity: usize,
    ) -> usize;
    fn dnh_unity_get_object_class(object: HModule) -> HModule;
    fn dnh_unity_class_type_object(class_handle: HModule) -> HModule;
    fn dnh_unity_copy_class_info(class_handle: HModule, output: *mut DnhClassInfoV2) -> bool;
    fn dnh_unity_get_class_fields(
        class_handle: HModule,
        output: *mut HModule,
        capacity: usize,
    ) -> usize;
    fn dnh_unity_get_class_methods(
        class_handle: HModule,
        output: *mut HModule,
        capacity: usize,
    ) -> usize;
    fn dnh_unity_copy_field_info(field_handle: HModule, output: *mut DnhFieldInfoV2) -> bool;
    fn dnh_unity_copy_method_info(method_handle: HModule, output: *mut DnhMethodInfoV2) -> bool;
    fn dnh_unity_method_parameter_type_name(method_handle: HModule, index: u32) -> *const c_char;
    fn dnh_unity_runtime_invoke(
        method_handle: HModule,
        object: HModule,
        arguments: *const HModule,
        exception: *mut HModule,
    ) -> HModule;
    fn dnh_unity_object_unbox(object: HModule) -> HModule;
    fn dnh_unity_object_new(class_handle: HModule) -> HModule;
    fn dnh_unity_string_new_utf8(value: *const c_char) -> HModule;
    fn dnh_unity_array_new(element_class: HModule, length: usize) -> HModule;
    fn dnh_unity_array_length(array: HModule) -> usize;
    fn dnh_unity_array_data(array: HModule) -> HModule;
    fn dnh_unity_class_value_size(class_handle: HModule, alignment: *mut u32) -> i32;
    fn dnh_unity_class_is_value_type(class_handle: HModule) -> bool;
    fn dnh_unity_field_get_value(
        object: HModule,
        field_handle: HModule,
        output: *mut c_void,
    ) -> bool;
    fn dnh_unity_field_set_value(
        object: HModule,
        field_handle: HModule,
        value: *const c_void,
    ) -> bool;
    fn dnh_unity_field_get_value_object(object: HModule, field_handle: HModule) -> HModule;
    fn dnh_unity_field_get_static_value(field_handle: HModule, output: *mut c_void) -> bool;
    fn dnh_unity_field_set_static_value(field_handle: HModule, value: *const c_void) -> bool;
    fn dnh_unity_gc_handle_new(object: HModule, pinned: bool) -> u32;
    fn dnh_unity_gc_handle_target(handle: u32) -> HModule;
    fn dnh_unity_gc_handle_free(handle: u32);
    fn dnh_unity_gc_handle_new_v4(object: HModule, pinned: bool) -> DnhGcHandleV4;
    fn dnh_unity_gc_handle_target_v4(handle: DnhGcHandleV4) -> HModule;
    fn dnh_unity_gc_handle_free_v4(handle: DnhGcHandleV4);
    fn dnh_unity_resolve_icall(name: *const c_char) -> HModule;
    fn dnh_unity_inflate_generic_method(
        method_handle: HModule,
        type_arguments: *const HModule,
        type_argument_count: usize,
    ) -> HModule;
    fn dnh_unity_find_fields_by_signature(
        class_handle: HModule,
        signature: *const DnhFieldSignatureV3,
        output: *mut HModule,
        capacity: usize,
    ) -> usize;
    fn dnh_unity_find_methods_by_signature(
        class_handle: HModule,
        signature: *const DnhMethodSignatureV3,
        output: *mut HModule,
        capacity: usize,
    ) -> usize;
    fn dnh_unity_find_classes_by_signature(
        assembly: *const c_char,
        signature: *const DnhClassSignatureV3,
        output: *mut HModule,
        capacity: usize,
    ) -> usize;
}

static OWN_MODULE: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static STATE: OnceLock<Mutex<Option<HostState>>> = OnceLock::new();
static LOGGER: OnceLock<Mutex<Option<Logger>>> = OnceLock::new();
static LAST_ERROR: OnceLock<Mutex<String>> = OnceLock::new();
static HOOKS: OnceLock<Mutex<BTreeMap<usize, usize>>> = OnceLock::new();

unsafe extern "system" fn unity_is_ready() -> bool {
    unsafe { dnh_unity_is_ready() }
}

unsafe extern "system" fn unity_thread_attach() -> HModule {
    unsafe { dnh_unity_thread_attach() }
}

unsafe extern "system" fn unity_thread_detach(thread: HModule) {
    unsafe { dnh_unity_thread_detach(thread) }
}

unsafe extern "system" fn unity_get_class(
    assembly: *const c_char,
    namespace_name: *const c_char,
    class_name: *const c_char,
) -> HModule {
    unsafe { dnh_unity_get_class(assembly, namespace_name, class_name) }
}

unsafe extern "system" fn unity_get_field(
    class_handle: HModule,
    field_name: *const c_char,
) -> HModule {
    unsafe { dnh_unity_get_field(class_handle, field_name) }
}

unsafe extern "system" fn unity_get_method(
    class_handle: HModule,
    method_name: *const c_char,
    parameter_count: i32,
) -> HModule {
    unsafe { dnh_unity_get_method(class_handle, method_name, parameter_count) }
}

unsafe extern "system" fn unity_field_offset(field_handle: HModule) -> i32 {
    unsafe { dnh_unity_field_offset(field_handle) }
}

unsafe extern "system" fn unity_method_address(method_handle: HModule) -> HModule {
    unsafe { dnh_unity_method_address(method_handle) }
}

unsafe extern "system" fn unity_find_objects(
    class_handle: HModule,
    output: *mut HModule,
    capacity: usize,
) -> usize {
    unsafe { dnh_unity_find_objects(class_handle, output, capacity) }
}

unsafe extern "system" fn unity_get_classes(
    assembly: *const c_char,
    output: *mut HModule,
    capacity: usize,
) -> usize {
    unsafe { dnh_unity_get_classes(assembly, output, capacity) }
}

unsafe extern "system" fn unity_get_object_class(object: HModule) -> HModule {
    unsafe { dnh_unity_get_object_class(object) }
}

unsafe extern "system" fn unity_class_type_object(class_handle: HModule) -> HModule {
    unsafe { dnh_unity_class_type_object(class_handle) }
}

unsafe extern "system" fn unity_copy_class_info(
    class_handle: HModule,
    output: *mut DnhClassInfoV2,
) -> bool {
    unsafe { dnh_unity_copy_class_info(class_handle, output) }
}

unsafe extern "system" fn unity_get_class_fields(
    class_handle: HModule,
    output: *mut HModule,
    capacity: usize,
) -> usize {
    unsafe { dnh_unity_get_class_fields(class_handle, output, capacity) }
}

unsafe extern "system" fn unity_get_class_methods(
    class_handle: HModule,
    output: *mut HModule,
    capacity: usize,
) -> usize {
    unsafe { dnh_unity_get_class_methods(class_handle, output, capacity) }
}

unsafe extern "system" fn unity_copy_field_info(
    field_handle: HModule,
    output: *mut DnhFieldInfoV2,
) -> bool {
    unsafe { dnh_unity_copy_field_info(field_handle, output) }
}

unsafe extern "system" fn unity_copy_method_info(
    method_handle: HModule,
    output: *mut DnhMethodInfoV2,
) -> bool {
    unsafe { dnh_unity_copy_method_info(method_handle, output) }
}

unsafe extern "system" fn unity_method_parameter_type_name(
    method_handle: HModule,
    index: u32,
) -> *const c_char {
    unsafe { dnh_unity_method_parameter_type_name(method_handle, index) }
}

unsafe extern "system" fn unity_runtime_invoke(
    method_handle: HModule,
    object: HModule,
    arguments: *const HModule,
    exception: *mut HModule,
) -> HModule {
    unsafe { dnh_unity_runtime_invoke(method_handle, object, arguments, exception) }
}

unsafe extern "system" fn unity_object_unbox(object: HModule) -> HModule {
    unsafe { dnh_unity_object_unbox(object) }
}

unsafe extern "system" fn unity_object_new(class_handle: HModule) -> HModule {
    unsafe { dnh_unity_object_new(class_handle) }
}

unsafe extern "system" fn unity_string_new_utf8(value: *const c_char) -> HModule {
    unsafe { dnh_unity_string_new_utf8(value) }
}

unsafe extern "system" fn unity_array_new(element_class: HModule, length: usize) -> HModule {
    unsafe { dnh_unity_array_new(element_class, length) }
}

unsafe extern "system" fn unity_array_length(array: HModule) -> usize {
    unsafe { dnh_unity_array_length(array) }
}

unsafe extern "system" fn unity_array_data(array: HModule) -> HModule {
    unsafe { dnh_unity_array_data(array) }
}

unsafe extern "system" fn unity_class_value_size(
    class_handle: HModule,
    alignment: *mut u32,
) -> i32 {
    unsafe { dnh_unity_class_value_size(class_handle, alignment) }
}

unsafe extern "system" fn unity_class_is_value_type(class_handle: HModule) -> bool {
    unsafe { dnh_unity_class_is_value_type(class_handle) }
}

unsafe extern "system" fn unity_field_get_value(
    object: HModule,
    field_handle: HModule,
    output: *mut c_void,
) -> bool {
    unsafe { dnh_unity_field_get_value(object, field_handle, output) }
}

unsafe extern "system" fn unity_field_set_value(
    object: HModule,
    field_handle: HModule,
    value: *const c_void,
) -> bool {
    unsafe { dnh_unity_field_set_value(object, field_handle, value) }
}

unsafe extern "system" fn unity_field_get_value_object(
    object: HModule,
    field_handle: HModule,
) -> HModule {
    unsafe { dnh_unity_field_get_value_object(object, field_handle) }
}

unsafe extern "system" fn unity_field_get_static_value(
    field_handle: HModule,
    output: *mut c_void,
) -> bool {
    unsafe { dnh_unity_field_get_static_value(field_handle, output) }
}

unsafe extern "system" fn unity_field_set_static_value(
    field_handle: HModule,
    value: *const c_void,
) -> bool {
    unsafe { dnh_unity_field_set_static_value(field_handle, value) }
}

unsafe extern "system" fn unity_gc_handle_new(object: HModule, pinned: bool) -> u32 {
    unsafe { dnh_unity_gc_handle_new(object, pinned) }
}

unsafe extern "system" fn unity_gc_handle_target(handle: u32) -> HModule {
    unsafe { dnh_unity_gc_handle_target(handle) }
}

unsafe extern "system" fn unity_gc_handle_free(handle: u32) {
    unsafe { dnh_unity_gc_handle_free(handle) }
}

unsafe extern "system" fn unity_gc_handle_new_v4(object: HModule, pinned: bool) -> DnhGcHandleV4 {
    unsafe { dnh_unity_gc_handle_new_v4(object, pinned) }
}

unsafe extern "system" fn unity_gc_handle_target_v4(handle: DnhGcHandleV4) -> HModule {
    unsafe { dnh_unity_gc_handle_target_v4(handle) }
}

unsafe extern "system" fn unity_gc_handle_free_v4(handle: DnhGcHandleV4) {
    unsafe { dnh_unity_gc_handle_free_v4(handle) }
}

unsafe extern "system" fn unity_resolve_icall(name: *const c_char) -> HModule {
    unsafe { dnh_unity_resolve_icall(name) }
}

unsafe extern "system" fn unity_inflate_generic_method(
    method_handle: HModule,
    type_arguments: *const HModule,
    type_argument_count: usize,
) -> HModule {
    unsafe { dnh_unity_inflate_generic_method(method_handle, type_arguments, type_argument_count) }
}

unsafe extern "system" fn unity_find_fields_by_signature(
    class_handle: HModule,
    signature: *const DnhFieldSignatureV3,
    output: *mut HModule,
    capacity: usize,
) -> usize {
    unsafe { dnh_unity_find_fields_by_signature(class_handle, signature, output, capacity) }
}

unsafe extern "system" fn unity_find_methods_by_signature(
    class_handle: HModule,
    signature: *const DnhMethodSignatureV3,
    output: *mut HModule,
    capacity: usize,
) -> usize {
    unsafe { dnh_unity_find_methods_by_signature(class_handle, signature, output, capacity) }
}

unsafe extern "system" fn unity_find_classes_by_signature(
    assembly: *const c_char,
    signature: *const DnhClassSignatureV3,
    output: *mut HModule,
    capacity: usize,
) -> usize {
    unsafe { dnh_unity_find_classes_by_signature(assembly, signature, output, capacity) }
}

struct SyncUnityApiV1(DnhUnityApiV1);
unsafe impl Sync for SyncUnityApiV1 {}

static UNITY_API_V1: SyncUnityApiV1 = SyncUnityApiV1(DnhUnityApiV1 {
    abi_version: DNH_ABI_VERSION_1,
    struct_size: size_of::<DnhUnityApiV1>() as u32,
    is_ready: unity_is_ready,
    thread_attach: unity_thread_attach,
    thread_detach: unity_thread_detach,
    get_class: unity_get_class,
    get_field: unity_get_field,
    get_method: unity_get_method,
    field_offset: unity_field_offset,
    method_address: unity_method_address,
    find_objects: unity_find_objects,
});

struct SyncUnityApiV2(DnhUnityApiV2);
unsafe impl Sync for SyncUnityApiV2 {}

const fn make_unity_api_v2(abi_version: u32) -> DnhUnityApiV2 {
    DnhUnityApiV2 {
        abi_version,
        struct_size: size_of::<DnhUnityApiV2>() as u32,
        is_ready: unity_is_ready,
        thread_attach: unity_thread_attach,
        thread_detach: unity_thread_detach,
        get_class: unity_get_class,
        get_field: unity_get_field,
        get_method: unity_get_method,
        field_offset: unity_field_offset,
        method_address: unity_method_address,
        find_objects: unity_find_objects,
        get_classes: unity_get_classes,
        get_object_class: unity_get_object_class,
        class_type_object: unity_class_type_object,
        copy_class_info: unity_copy_class_info,
        get_class_fields: unity_get_class_fields,
        get_class_methods: unity_get_class_methods,
        copy_field_info: unity_copy_field_info,
        copy_method_info: unity_copy_method_info,
        method_parameter_type_name: unity_method_parameter_type_name,
        runtime_invoke: unity_runtime_invoke,
        object_unbox: unity_object_unbox,
        object_new: unity_object_new,
        string_new_utf8: unity_string_new_utf8,
        array_new: unity_array_new,
        array_length: unity_array_length,
        array_data: unity_array_data,
        class_value_size: unity_class_value_size,
        class_is_value_type: unity_class_is_value_type,
        field_get_value: unity_field_get_value,
        field_set_value: unity_field_set_value,
        field_get_value_object: unity_field_get_value_object,
        field_get_static_value: unity_field_get_static_value,
        field_set_static_value: unity_field_set_static_value,
        gc_handle_new: unity_gc_handle_new,
        gc_handle_target: unity_gc_handle_target,
        gc_handle_free: unity_gc_handle_free,
        resolve_icall: unity_resolve_icall,
    }
}

static UNITY_API_V2: SyncUnityApiV2 = SyncUnityApiV2(make_unity_api_v2(DNH_ABI_VERSION_2));

struct SyncUnityApiV3(DnhUnityApiV3);
unsafe impl Sync for SyncUnityApiV3 {}

const fn make_unity_api_v3(abi_version: u32) -> DnhUnityApiV3 {
    DnhUnityApiV3 {
        v2: make_unity_api_v2(abi_version),
        find_fields_by_signature: unity_find_fields_by_signature,
        find_methods_by_signature: unity_find_methods_by_signature,
        find_classes_by_signature: unity_find_classes_by_signature,
    }
}

static UNITY_API_V3: SyncUnityApiV3 = SyncUnityApiV3(make_unity_api_v3(DNH_ABI_VERSION_3));

struct SyncUnityApiV4(DnhUnityApiV4);
unsafe impl Sync for SyncUnityApiV4 {}

const fn make_unity_api_v4(abi_version: u32) -> DnhUnityApiV4 {
    DnhUnityApiV4 {
        v3: make_unity_api_v3(abi_version),
        gc_handle_new_v4: unity_gc_handle_new_v4,
        gc_handle_target_v4: unity_gc_handle_target_v4,
        gc_handle_free_v4: unity_gc_handle_free_v4,
    }
}

static UNITY_API_V4: SyncUnityApiV4 = SyncUnityApiV4(make_unity_api_v4(DNH_ABI_VERSION_4));
static UNITY_API_V5: SyncUnityApiV4 = SyncUnityApiV4(make_unity_api_v4(DNH_ABI_VERSION_5));

struct SyncUnityApiV6(DnhUnityApiV6);
unsafe impl Sync for SyncUnityApiV6 {}

static UNITY_API_V6: SyncUnityApiV6 = SyncUnityApiV6(DnhUnityApiV6 {
    v4: make_unity_api_v4(DNH_ABI_VERSION_6),
    inflate_generic_method: unity_inflate_generic_method,
});

fn hooks() -> &'static Mutex<BTreeMap<usize, usize>> {
    HOOKS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

unsafe extern "system" fn hook_create(
    target: HModule,
    detour: HModule,
    original: *mut HModule,
) -> i32 {
    if target.is_null() || detour.is_null() || original.is_null() {
        return DNH_ERROR;
    }
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        MinHook::create_hook(target, detour)
    }));
    let Ok(Ok(trampoline)) = result else {
        write_log(DnhLogLevel::Error, "MinHook create failed");
        return DNH_ERROR;
    };
    unsafe {
        *original = trampoline;
    }
    if let Ok(mut registry) = hooks().lock() {
        registry.insert(target as usize, detour as usize);
    }
    DNH_OK
}

unsafe extern "system" fn hook_enable(target: HModule) -> i32 {
    if target.is_null() {
        return DNH_ERROR;
    }
    match catch_unwind(AssertUnwindSafe(|| unsafe { MinHook::enable_hook(target) })) {
        Ok(Ok(())) => DNH_OK,
        _ => DNH_ERROR,
    }
}

unsafe extern "system" fn hook_disable(target: HModule) -> i32 {
    if target.is_null() {
        return DNH_ERROR;
    }
    match catch_unwind(AssertUnwindSafe(|| unsafe {
        MinHook::disable_hook(target)
    })) {
        Ok(Ok(())) => DNH_OK,
        _ => DNH_ERROR,
    }
}

unsafe extern "system" fn hook_remove(target: HModule) -> i32 {
    if target.is_null() {
        return DNH_ERROR;
    }
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        let _ = MinHook::disable_hook(target);
        MinHook::remove_hook(target)
    }));
    if let Ok(mut registry) = hooks().lock() {
        registry.remove(&(target as usize));
    }
    match result {
        Ok(Ok(())) => DNH_OK,
        _ => DNH_ERROR,
    }
}

fn remove_hooks_for_module(module: HModule) {
    let targets = hooks()
        .lock()
        .ok()
        .map(|registry| {
            registry
                .iter()
                .filter_map(|(target, detour)| {
                    let mut detour_module = null_mut();
                    let found = unsafe {
                        GetModuleHandleExW(
                            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                                | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                            *detour as *const u16,
                            &mut detour_module,
                        )
                    };
                    (found != 0 && detour_module == module).then_some(*target)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for target in targets {
        unsafe {
            hook_remove(target as HModule);
        }
    }
}

struct SyncHookApiV5(DnhHookApiV5);
unsafe impl Sync for SyncHookApiV5 {}

static HOOK_API_V5: SyncHookApiV5 = SyncHookApiV5(DnhHookApiV5 {
    struct_size: size_of::<DnhHookApiV5>() as u32,
    create: hook_create,
    enable: hook_enable,
    disable: hook_disable,
    remove: hook_remove,
});

struct SyncHostApiV1(DnhHostApiV1);
unsafe impl Sync for SyncHostApiV1 {}

static HOST_API_V1: SyncHostApiV1 = SyncHostApiV1(DnhHostApiV1 {
    abi_version: DNH_ABI_VERSION_1,
    struct_size: size_of::<DnhHostApiV1>() as u32,
    log: host_log_callback,
    unity: &UNITY_API_V1.0,
});

struct SyncHostApiV2(DnhHostApiV2);
unsafe impl Sync for SyncHostApiV2 {}

static HOST_API_V2: SyncHostApiV2 = SyncHostApiV2(DnhHostApiV2 {
    abi_version: DNH_ABI_VERSION_2,
    struct_size: size_of::<DnhHostApiV2>() as u32,
    log: host_log_callback,
    unity: &UNITY_API_V2.0,
});

struct SyncHostApiV3(DnhHostApiV3);
unsafe impl Sync for SyncHostApiV3 {}

static HOST_API_V3: SyncHostApiV3 = SyncHostApiV3(DnhHostApiV3 {
    abi_version: DNH_ABI_VERSION_3,
    struct_size: size_of::<DnhHostApiV3>() as u32,
    log: host_log_callback,
    unity: &UNITY_API_V3.0,
});

struct SyncHostApiV4(DnhHostApiV4);
unsafe impl Sync for SyncHostApiV4 {}

static HOST_API_V4: SyncHostApiV4 = SyncHostApiV4(DnhHostApiV4 {
    abi_version: DNH_ABI_VERSION_4,
    struct_size: size_of::<DnhHostApiV4>() as u32,
    log: host_log_callback,
    unity: &UNITY_API_V4.0,
});

struct SyncHostApiV5(DnhHostApiV5);
unsafe impl Sync for SyncHostApiV5 {}

static HOST_API_V5: SyncHostApiV5 = SyncHostApiV5(DnhHostApiV5 {
    v4: DnhHostApiV4 {
        abi_version: DNH_ABI_VERSION_5,
        struct_size: size_of::<DnhHostApiV5>() as u32,
        log: host_log_callback,
        unity: &UNITY_API_V5.0,
    },
    hooks: &HOOK_API_V5.0,
});

struct SyncHostApiV6(DnhHostApiV6);
unsafe impl Sync for SyncHostApiV6 {}

static HOST_API_V6: SyncHostApiV6 = SyncHostApiV6(DnhHostApiV6 {
    abi_version: DNH_ABI_VERSION_6,
    struct_size: size_of::<DnhHostApiV6>() as u32,
    log: host_log_callback,
    unity: &UNITY_API_V6.0,
    hooks: &HOOK_API_V5.0,
});

struct Logger {
    file: File,
}

struct LoadedMod {
    module: HModule,
    path: PathBuf,
    abi_version: u32,
    id: String,
    name: String,
    version: String,
    author: String,
    tick: DnmTickFn,
    unload: DnmUnloadFn,
}

unsafe impl Send for LoadedMod {}

struct HostState {
    mod_directory: PathBuf,
    control_directory: PathBuf,
    control_tick: u64,
    f1_down: bool,
    disabled: BTreeSet<String>,
    mods: Vec<LoadedMod>,
    ui: Option<UiController>,
    worker_sender: Sender<WorkerResult>,
    worker_receiver: Receiver<WorkerResult>,
    workers: Vec<JoinHandle<()>>,
}

unsafe impl Send for HostState {}

fn state() -> &'static Mutex<Option<HostState>> {
    STATE.get_or_init(|| Mutex::new(None))
}

fn logger() -> &'static Mutex<Option<Logger>> {
    LOGGER.get_or_init(|| Mutex::new(None))
}

fn last_error() -> &'static Mutex<String> {
    LAST_ERROR.get_or_init(|| Mutex::new(String::new()))
}

fn wide_null(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

fn module_path(module: HModule) -> Option<PathBuf> {
    let mut buffer = vec![0_u16; MAX_PATH_CHARS];
    let length =
        unsafe { GetModuleFileNameW(module, buffer.as_mut_ptr(), buffer.len() as DWord) } as usize;
    if length == 0 || length >= buffer.len() {
        return None;
    }
    buffer.truncate(length);
    Some(PathBuf::from(OsString::from_wide(&buffer)))
}

fn own_module() -> HModule {
    let cached = OWN_MODULE.load(Ordering::Acquire);
    if !cached.is_null() {
        return cached;
    }

    let mut module = null_mut();
    let address = DNH_Initialize as *const () as *const u16;
    let found = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            address,
            &mut module,
        )
    };
    if found != 0 {
        OWN_MODULE.store(module, Ordering::Release);
        module
    } else {
        null_mut()
    }
}

fn path_from_wide_ptr(value: *const u16) -> Option<PathBuf> {
    if value.is_null() {
        return None;
    }
    let mut length = 0_usize;
    unsafe {
        while *value.add(length) != 0 {
            length += 1;
            if length >= MAX_PATH_CHARS {
                return None;
            }
        }
        Some(PathBuf::from(OsString::from_wide(
            std::slice::from_raw_parts(value, length),
        )))
    }
}

fn set_error(message: impl Into<String>) {
    let message = message.into();
    if let Ok(mut value) = last_error().lock() {
        *value = message.clone();
    }
    write_log(DnhLogLevel::Error, &message);
}

fn level_name(level: DnhLogLevel) -> &'static str {
    match level {
        DnhLogLevel::Trace => "TRACE",
        DnhLogLevel::Info => "INFO",
        DnhLogLevel::Warn => "WARN",
        DnhLogLevel::Error => "ERROR",
    }
}

fn write_log(level: DnhLogLevel, message: &str) {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let line = format!("[{seconds}] [{}] {message}\r\n", level_name(level));

    let mut debug_line = line.as_bytes().to_vec();
    for byte in &mut debug_line {
        if *byte == 0 {
            *byte = b'?';
        }
    }
    debug_line.push(0);
    unsafe { OutputDebugStringA(debug_line.as_ptr().cast()) };

    if let Ok(mut guard) = logger().lock()
        && let Some(logger) = guard.as_mut()
    {
        let _ = logger.file.write_all(line.as_bytes());
        let _ = logger.file.flush();
    }
}

unsafe extern "system" fn host_log_callback(
    level: DnhLogLevel,
    message: *const u8,
    message_len: usize,
) {
    if message.is_null() || message_len == 0 {
        return;
    }
    let bytes = unsafe { std::slice::from_raw_parts(message, message_len) };
    write_log(level, &String::from_utf8_lossy(bytes));
}

fn c_string(value: *const c_char, label: &str) -> Result<String, String> {
    if value.is_null() {
        return Err(format!("descriptor field '{label}' is null"));
    }
    let text = unsafe { CStr::from_ptr(value) }
        .to_str()
        .map_err(|_| format!("descriptor field '{label}' is not UTF-8"))?;
    if text.is_empty() {
        return Err(format!("descriptor field '{label}' is empty"));
    }
    Ok(text.to_owned())
}

unsafe fn export<T: Copy>(module: HModule, name: &[u8]) -> Option<T> {
    let address = unsafe { GetProcAddress(module, name.as_ptr().cast()) };
    if address.is_null() {
        return None;
    }
    Some(unsafe { std::mem::transmute_copy::<*mut c_void, T>(&address) })
}

struct ModMetadata {
    abi_version: u32,
    id: String,
    name: String,
    version: String,
    author: String,
}

fn query_mod(module: HModule) -> Result<ModMetadata, String> {
    unsafe {
        let query = export::<DnmQueryFn>(module, QUERY_EXPORT)
            .ok_or_else(|| "missing DNM_Query".to_owned())?;
        let mut negotiated_abi = 0;
        let mut info_ptr = std::ptr::null();
        for abi_version in [
            DNH_ABI_VERSION_6,
            DNH_ABI_VERSION_5,
            DNH_ABI_VERSION_4,
            DNH_ABI_VERSION_3,
            DNH_ABI_VERSION_2,
            DNH_ABI_VERSION_1,
        ] {
            let candidate = catch_unwind(AssertUnwindSafe(|| query(abi_version)))
                .map_err(|_| format!("DNM_Query panicked for ABI v{abi_version}"))?;
            if !candidate.is_null() {
                negotiated_abi = abi_version;
                info_ptr = candidate;
                break;
            }
        }

        let info = info_ptr
            .as_ref()
            .ok_or_else(|| "DNM_Query rejected ABI v6, v5, v4, v3, v2 and v1".to_owned())?;
        if info.abi_version != negotiated_abi || info.struct_size < size_of::<DnhModInfoV1>() as u32
        {
            return Err("invalid DnhModInfoV1 descriptor".to_owned());
        }

        Ok(ModMetadata {
            abi_version: negotiated_abi,
            id: c_string(info.id, "id")?,
            name: c_string(info.name, "name")?,
            version: c_string(info.version, "version")?,
            author: c_string(info.author, "author")?,
        })
    }
}

fn load_one(path: &Path) -> Result<LoadedMod, String> {
    let wide_path = wide_null(path.as_os_str());
    let module = unsafe {
        LoadLibraryExW(
            wide_path.as_ptr(),
            null_mut(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        )
    };
    if module.is_null() {
        return Err(format!("LoadLibraryExW failed for {}", path.display()));
    }

    let result = (|| -> Result<LoadedMod, String> {
        unsafe {
            let load = export::<DnmLoadFn>(module, LOAD_EXPORT)
                .ok_or_else(|| "missing DNM_Load".to_owned())?;
            let tick = export::<DnmTickFn>(module, TICK_EXPORT)
                .ok_or_else(|| "missing DNM_Tick".to_owned())?;
            let unload = export::<DnmUnloadFn>(module, UNLOAD_EXPORT)
                .ok_or_else(|| "missing DNM_Unload".to_owned())?;

            let metadata = query_mod(module)?;

            let host_api = match metadata.abi_version {
                DNH_ABI_VERSION_6 => (&HOST_API_V6.0 as *const DnhHostApiV6).cast::<DnhHostApiV1>(),
                DNH_ABI_VERSION_5 => (&HOST_API_V5.0 as *const DnhHostApiV5).cast::<DnhHostApiV1>(),
                DNH_ABI_VERSION_4 => (&HOST_API_V4.0 as *const DnhHostApiV4).cast::<DnhHostApiV1>(),
                DNH_ABI_VERSION_3 => (&HOST_API_V3.0 as *const DnhHostApiV3).cast::<DnhHostApiV1>(),
                DNH_ABI_VERSION_2 => (&HOST_API_V2.0 as *const DnhHostApiV2).cast::<DnhHostApiV1>(),
                _ => &HOST_API_V1.0,
            };
            let load_result = catch_unwind(AssertUnwindSafe(|| load(host_api)))
                .map_err(|_| "DNM_Load panicked".to_owned())?;
            if load_result != DNH_OK {
                return Err(format!("DNM_Load returned {load_result}"));
            }

            write_log(
                DnhLogLevel::Info,
                &format!(
                    "{} negotiated native ABI v{}",
                    metadata.name, metadata.abi_version
                ),
            );

            Ok(LoadedMod {
                module,
                path: path.to_path_buf(),
                abi_version: metadata.abi_version,
                id: metadata.id,
                name: metadata.name,
                version: metadata.version,
                author: metadata.author,
                tick,
                unload,
            })
        }
    })();

    if result.is_err() {
        remove_hooks_for_module(module);
        unsafe {
            FreeLibrary(module);
        }
    }
    result
}

fn discover_mods(directory: &Path, own_path: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let is_dll = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"));
            if !is_dll || own_path.is_some_and(|own| own == path) {
                return None;
            }
            Some(path)
        })
        .collect::<Vec<_>>();
    paths.sort_by_key(|path| path.file_name().map(|name| name.to_ascii_lowercase()));
    paths
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModSettings {
    disabled: BTreeSet<String>,
}

fn mod_file_key(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_ascii_lowercase)
}

fn settings_path(directory: &Path) -> PathBuf {
    directory.join("native-mods.json")
}

fn load_settings(directory: &Path) -> BTreeSet<String> {
    let path = settings_path(directory);
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return BTreeSet::new();
    };
    match serde_json::from_str::<ModSettings>(&contents) {
        Ok(mut settings) => {
            settings.disabled = settings
                .disabled
                .into_iter()
                .map(|name| name.to_ascii_lowercase())
                .collect();
            settings.disabled
        }
        Err(error) => {
            write_log(
                DnhLogLevel::Warn,
                &format!("ignored invalid {}: {error}", path.display()),
            );
            BTreeSet::new()
        }
    }
}

fn save_settings(state: &HostState) -> Result<(), String> {
    let settings = ModSettings {
        disabled: state.disabled.clone(),
    };
    let json = serde_json::to_vec_pretty(&settings)
        .map_err(|error| format!("sérialisation des réglages impossible : {error}"))?;
    let path = settings_path(&state.mod_directory);
    std::fs::write(&path, json)
        .map_err(|error| format!("écriture de {} impossible : {error}", path.display()))
}

fn inspect_one(path: &Path) -> Result<ModMetadata, String> {
    let wide_path = wide_null(path.as_os_str());
    let module = unsafe {
        LoadLibraryExW(
            wide_path.as_ptr(),
            null_mut(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        )
    };
    if module.is_null() {
        return Err(format!("LoadLibraryExW failed for {}", path.display()));
    }
    let result = query_mod(module);
    unsafe {
        FreeLibrary(module);
    }
    result
}

fn ui_snapshot(state: &HostState) -> UiSnapshot {
    let mut installed = Vec::new();
    for path in discover_mods(&state.mod_directory, None) {
        if let Some(loaded) = state.mods.iter().find(|loaded| {
            std::fs::canonicalize(&loaded.path)
                .ok()
                .zip(std::fs::canonicalize(&path).ok())
                .is_some_and(|(left, right)| left == right)
        }) {
            installed.push(InstalledModView {
                id: loaded.id.clone(),
                name: loaded.name.clone(),
                version: loaded.version.clone(),
                abi_version: loaded.abi_version,
                path: path.display().to_string(),
                loaded: true,
                disabled: false,
            });
            continue;
        }
        match inspect_one(&path) {
            Ok(metadata) => installed.push(InstalledModView {
                id: metadata.id,
                name: metadata.name,
                version: metadata.version,
                abi_version: metadata.abi_version,
                path: path.display().to_string(),
                loaded: false,
                disabled: mod_file_key(&path).is_some_and(|key| state.disabled.contains(&key)),
            }),
            Err(error) => write_log(
                DnhLogLevel::Warn,
                &format!("cannot inspect {} for mod manager: {error}", path.display()),
            ),
        }
    }
    installed.sort_by_key(|entry| entry.name.to_ascii_lowercase());
    UiSnapshot { installed }
}

fn json_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(output, "\\u{:04x}", character as u32);
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

fn control_status(state: &HostState) -> String {
    let mods = state
        .mods
        .iter()
        .map(|loaded_mod| {
            format!(
                "{{\"id\":{},\"name\":{},\"version\":{},\"author\":{},\"abiVersion\":{},\"path\":{}}}",
                json_string(&loaded_mod.id),
                json_string(&loaded_mod.name),
                json_string(&loaded_mod.version),
                json_string(&loaded_mod.author),
                loaded_mod.abi_version,
                json_string(&loaded_mod.path.display().to_string()),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"running\":true,\"pid\":{},\"abiVersion\":{},\"controlDirectory\":{},\"mods\":[{}]}}",
        std::process::id(),
        DNH_ABI_VERSION_6,
        json_string(&state.control_directory.display().to_string()),
        mods,
    )
}

fn unload_mod_at(state: &mut HostState, index: usize) {
    let loaded_mod = state.mods.remove(index);
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe { (loaded_mod.unload)() }));
    remove_hooks_for_module(loaded_mod.module);
    unsafe {
        FreeLibrary(loaded_mod.module);
    }
    write_log(
        DnhLogLevel::Info,
        &format!("control unloaded {}", loaded_mod.id),
    );
}

fn controlled_load(state: &mut HostState, requested_path: &str) -> Result<String, String> {
    let path = PathBuf::from(requested_path);
    if !path.is_absolute()
        || !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
    {
        return Err("LOAD requires an absolute .dll path".to_owned());
    }
    let canonical = std::fs::canonicalize(&path)
        .map_err(|error| format!("cannot resolve {}: {error}", path.display()))?;
    let loaded_mod = load_one(&canonical)?;
    if state
        .mods
        .iter()
        .any(|existing| existing.id == loaded_mod.id)
    {
        unsafe {
            (loaded_mod.unload)();
            remove_hooks_for_module(loaded_mod.module);
            FreeLibrary(loaded_mod.module);
        }
        return Err(format!("mod id '{}' is already loaded", loaded_mod.id));
    }
    let result = format!(
        "{{\"id\":{},\"name\":{},\"version\":{},\"abiVersion\":{}}}",
        json_string(&loaded_mod.id),
        json_string(&loaded_mod.name),
        json_string(&loaded_mod.version),
        loaded_mod.abi_version,
    );
    write_log(
        DnhLogLevel::Info,
        &format!(
            "control loaded {} {} ({})",
            loaded_mod.name, loaded_mod.version, loaded_mod.id
        ),
    );
    state.mods.push(loaded_mod);
    Ok(result)
}

fn managed_mod_path(state: &HostState, requested_path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(requested_path);
    let canonical = std::fs::canonicalize(&path)
        .map_err(|error| format!("résolution de {} impossible : {error}", path.display()))?;
    let directory = std::fs::canonicalize(&state.mod_directory).map_err(|error| {
        format!(
            "résolution de {} impossible : {error}",
            state.mod_directory.display()
        )
    })?;
    if canonical.parent() != Some(directory.as_path())
        || !canonical
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
    {
        return Err("le mod sélectionné n'appartient pas au dossier NativeMods".to_owned());
    }
    Ok(canonical)
}

fn enable_mod(state: &mut HostState, view: &InstalledModView) -> Result<String, String> {
    if state.mods.iter().any(|loaded| loaded.id == view.id) {
        return Ok(format!("{} est déjà actif.", view.name));
    }
    let path = managed_mod_path(state, &view.path)?;
    let key = mod_file_key(&path).ok_or_else(|| "nom de DLL invalide".to_owned())?;
    let was_disabled = state.disabled.remove(&key);
    match load_one(&path) {
        Ok(loaded) => {
            if state.mods.iter().any(|existing| existing.id == loaded.id) {
                unsafe {
                    (loaded.unload)();
                    remove_hooks_for_module(loaded.module);
                    FreeLibrary(loaded.module);
                }
                if was_disabled {
                    state.disabled.insert(key);
                }
                return Err(format!("le mod '{}' est déjà chargé", view.id));
            }
            let name = loaded.name.clone();
            state.mods.push(loaded);
            save_settings(state)?;
            Ok(format!("{name} est maintenant actif."))
        }
        Err(error) => {
            if was_disabled {
                state.disabled.insert(key);
            }
            Err(error)
        }
    }
}

fn disable_mod(state: &mut HostState, view: &InstalledModView) -> Result<String, String> {
    let path = managed_mod_path(state, &view.path)?;
    if let Some(index) = state.mods.iter().position(|loaded| loaded.id == view.id) {
        unload_mod_at(state, index);
    }
    let key = mod_file_key(&path).ok_or_else(|| "nom de DLL invalide".to_owned())?;
    state.disabled.insert(key);
    save_settings(state)?;
    Ok(format!(
        "{} est désactivé, y compris pour les prochains lancements.",
        view.name
    ))
}

fn reload_mod(state: &mut HostState, view: &InstalledModView) -> Result<String, String> {
    let path = managed_mod_path(state, &view.path)?;
    if let Some(index) = state.mods.iter().position(|loaded| loaded.id == view.id) {
        unload_mod_at(state, index);
    }
    let loaded = load_one(&path)?;
    let name = loaded.name.clone();
    state.mods.push(loaded);
    if let Some(key) = mod_file_key(&path) {
        state.disabled.remove(&key);
    }
    save_settings(state)?;
    Ok(format!("{name} a été rechargé."))
}

fn apply_prepared_install(
    state: &mut HostState,
    entry: &MarketplaceMod,
    temporary_path: &Path,
) -> Result<String, String> {
    marketplace::validate_entry(entry)?;
    let target = state.mod_directory.join(&entry.file_name);
    let backup = state
        .mod_directory
        .join(format!(".{}.backup", entry.file_name));
    if backup.exists() {
        let _ = std::fs::remove_file(temporary_path);
        return Err(format!(
            "une sauvegarde existe déjà : {}. Supprimez-la après vérification.",
            backup.display()
        ));
    }

    let loaded_index = state
        .mods
        .iter()
        .position(|loaded| loaded.id == entry.id || loaded.path == target);
    let was_loaded = loaded_index.is_some();
    if let Some(index) = loaded_index {
        unload_mod_at(state, index);
    }

    let had_target = target.exists();
    if had_target && let Err(error) = std::fs::rename(&target, &backup) {
        let _ = std::fs::remove_file(temporary_path);
        return Err(format!("sauvegarde de l'ancienne DLL impossible : {error}"));
    }
    if let Err(error) = std::fs::rename(temporary_path, &target) {
        if had_target {
            let _ = std::fs::rename(&backup, &target);
        }
        return Err(format!(
            "installation de la nouvelle DLL impossible : {error}"
        ));
    }

    let load_result = load_one(&target).and_then(|loaded| {
        if loaded.id != entry.id {
            unsafe {
                (loaded.unload)();
                remove_hooks_for_module(loaded.module);
                FreeLibrary(loaded.module);
            }
            return Err(format!(
                "la DLL téléchargée déclare '{}' au lieu de '{}'",
                loaded.id, entry.id
            ));
        }
        Ok(loaded)
    });
    match load_result {
        Ok(loaded) => {
            let installed_name = loaded.name.clone();
            let installed_version = loaded.version.clone();
            state.mods.push(loaded);
            if let Some(key) = mod_file_key(&target) {
                state.disabled.remove(&key);
            }
            save_settings(state)?;
            if had_target {
                let _ = std::fs::remove_file(&backup);
            }
            Ok(format!(
                "{installed_name} {installed_version} est installé et actif."
            ))
        }
        Err(error) => {
            let _ = std::fs::remove_file(&target);
            if had_target {
                let _ = std::fs::rename(&backup, &target);
                if was_loaded {
                    match load_one(&target) {
                        Ok(previous) => state.mods.push(previous),
                        Err(rollback_error) => write_log(
                            DnhLogLevel::Error,
                            &format!(
                                "rollback load failed for {}: {rollback_error}",
                                target.display()
                            ),
                        ),
                    }
                }
            }
            Err(format!(
                "installation annulée et ancienne DLL restaurée : {error}"
            ))
        }
    }
}

fn process_manager(state: &mut HostState) {
    let mut worker_index = 0;
    while worker_index < state.workers.len() {
        if state.workers[worker_index].is_finished() {
            let worker = state.workers.remove(worker_index);
            let _ = worker.join();
        } else {
            worker_index += 1;
        }
    }

    let f1_down = unsafe { GetAsyncKeyState(VK_F1) } < 0;
    if f1_down && !state.f1_down {
        let snapshot = ui_snapshot(state);
        if let Some(ui) = state.ui.as_ref() {
            ui.toggle(snapshot);
        }
    }
    state.f1_down = f1_down;

    for _ in 0..8 {
        let action = state.ui.as_ref().and_then(|ui| ui.try_action().ok());
        let Some(action) = action else {
            break;
        };
        let result = match action {
            UiAction::Enable(view) => enable_mod(state, &view),
            UiAction::Disable(view) => disable_mod(state, &view),
            UiAction::Reload(view) => reload_mod(state, &view),
            UiAction::RefreshMarketplace => {
                if let Some(ui) = state.ui.as_ref() {
                    ui.set_status("Actualisation de la marketplace GitHub…");
                }
                state
                    .workers
                    .push(marketplace::refresh_async(state.worker_sender.clone()));
                continue;
            }
            UiAction::Install(entry) => {
                if let Some(ui) = state.ui.as_ref() {
                    ui.set_status(format!(
                        "Téléchargement et vérification SHA-256 de {}…",
                        entry.name
                    ));
                }
                state.workers.push(marketplace::download_mod_async(
                    entry,
                    state.mod_directory.clone(),
                    state.worker_sender.clone(),
                ));
                continue;
            }
        };
        if let Some(ui) = state.ui.as_ref() {
            match result {
                Ok(message) => ui.set_status(message),
                Err(error) => ui.set_status(format!("Erreur : {error}")),
            }
            ui.update_snapshot(ui_snapshot(state));
        }
    }

    for _ in 0..8 {
        let Ok(result) = state.worker_receiver.try_recv() else {
            break;
        };
        match result {
            WorkerResult::Catalog(result) => {
                if let Some(ui) = state.ui.as_ref() {
                    ui.update_catalog(result);
                }
            }
            WorkerResult::Downloaded {
                entry,
                temporary_path,
            } => {
                let result = apply_prepared_install(state, &entry, &temporary_path);
                if let Some(ui) = state.ui.as_ref() {
                    match result {
                        Ok(message) => ui.set_status(message),
                        Err(error) => ui.set_status(format!("Erreur : {error}")),
                    }
                    ui.update_snapshot(ui_snapshot(state));
                }
            }
            WorkerResult::DownloadFailed { id, error } => {
                if let Some(ui) = state.ui.as_ref() {
                    ui.set_status(format!("Échec du téléchargement de {id} : {error}"));
                }
            }
        }
    }
}

fn execute_control_command(state: &mut HostState, request: &str) -> Result<String, String> {
    let parts = request
        .trim_end_matches(['\r', '\n'])
        .split('\t')
        .collect::<Vec<_>>();
    match parts.as_slice() {
        ["STATUS"] | ["LIST"] => Ok(control_status(state)),
        ["LOAD", path] => controlled_load(state, path),
        ["UNLOAD", id] => {
            let index = state
                .mods
                .iter()
                .position(|loaded_mod| loaded_mod.id == *id)
                .ok_or_else(|| format!("mod id '{id}' is not loaded"))?;
            unload_mod_at(state, index);
            Ok(format!("{{\"unloaded\":{}}}", json_string(id)))
        }
        ["RELOAD", id, path] => {
            let index = state
                .mods
                .iter()
                .position(|loaded_mod| loaded_mod.id == *id)
                .ok_or_else(|| format!("mod id '{id}' is not loaded"))?;
            unload_mod_at(state, index);
            controlled_load(state, path)
        }
        _ => Err("expected STATUS, LIST, LOAD, UNLOAD or RELOAD".to_owned()),
    }
}

fn write_control_response(path: &Path, response: &str) -> std::io::Result<()> {
    let temporary = path.with_extension("response.tmp");
    std::fs::write(&temporary, response.as_bytes())?;
    std::fs::rename(temporary, path)
}

fn process_control_requests(state: &mut HostState) {
    state.control_tick = state.control_tick.wrapping_add(1);
    if !state.control_tick.is_multiple_of(15) {
        return;
    }
    let inbox = state.control_directory.join("inbox");
    let outbox = state.control_directory.join("outbox");
    let mut requests = std::fs::read_dir(&inbox)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|value| value.to_str()) == Some("request")).then_some(path)
        })
        .collect::<Vec<_>>();
    requests.sort();
    requests.truncate(8);

    for path in requests {
        let Some(request_id) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        if request_id.is_empty()
            || !request_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            continue;
        }
        let request = match std::fs::read_to_string(&path) {
            Ok(request) if request.len() <= 32_768 => request,
            Ok(_) => {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            Err(_) => continue,
        };
        let response = match execute_control_command(state, &request) {
            Ok(result) => format!("OK\t{result}"),
            Err(error) => format!("ERROR\t{}", error.replace(['\r', '\n', '\t'], " ")),
        };
        let response_path = outbox.join(format!("{request_id}.response"));
        if let Err(error) = write_control_response(&response_path, &response) {
            write_log(
                DnhLogLevel::Error,
                &format!("cannot write native control response: {error}"),
            );
            continue;
        }
        let _ = std::fs::remove_file(&path);
    }

    let descriptor = state.mod_directory.join("native-control.json");
    let _ = std::fs::write(descriptor, control_status(state));
}

#[unsafe(no_mangle)]
/// Initializes UnityResolve and loads all valid mods from the selected folder.
///
/// # Safety
///
/// `game_assembly`, when non-null, must be the loaded `GameAssembly.dll`
/// module handle. `mod_directory`, when non-null, must be a readable,
/// null-terminated UTF-16 string.
pub unsafe extern "system" fn DNH_Initialize(
    game_assembly: HModule,
    mod_directory: *const u16,
) -> i32 {
    let mut state_guard = match state().lock() {
        Ok(guard) => guard,
        Err(_) => return -2,
    };
    if state_guard.is_some() {
        return 1;
    }

    let own_module = own_module();
    let own_path = module_path(own_module);
    let directory = path_from_wide_ptr(mod_directory).unwrap_or_else(|| {
        own_path
            .as_deref()
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new("."))
            .join("NativeMods")
    });
    if let Err(error) = std::fs::create_dir_all(&directory) {
        set_error(format!(
            "cannot create mod directory {}: {error}",
            directory.display()
        ));
        return -3;
    }

    let log_path = directory.join("native-host.log");
    match OpenOptions::new().create(true).append(true).open(&log_path) {
        Ok(file) => {
            if let Ok(mut guard) = logger().lock() {
                *guard = Some(Logger { file });
            }
        }
        Err(error) => {
            set_error(format!("cannot open {}: {error}", log_path.display()));
            return -4;
        }
    }

    write_log(
        DnhLogLevel::Info,
        &format!("Dofus Native Host ABI v{DNH_ABI_VERSION_6} starting"),
    );
    write_log(
        DnhLogLevel::Info,
        &format!("mod directory: {}", directory.display()),
    );

    let resolved_game_assembly = if game_assembly.is_null() {
        let name = wide_null("GameAssembly.dll");
        unsafe { GetModuleHandleW(name.as_ptr()) }
    } else {
        game_assembly
    };
    if resolved_game_assembly.is_null() {
        set_error("GameAssembly.dll is not loaded");
        return -5;
    }
    if !unsafe { dnh_unity_initialize(resolved_game_assembly) } {
        set_error("UnityResolve initialization failed; call after IL2CPP startup");
        return -6;
    }

    let control_directory = directory
        .join("control")
        .join(std::process::id().to_string());
    if let Err(error) = std::fs::create_dir_all(control_directory.join("inbox"))
        .and_then(|_| std::fs::create_dir_all(control_directory.join("outbox")))
    {
        set_error(format!(
            "cannot create native control directory {}: {error}",
            control_directory.display()
        ));
        return -7;
    }

    let disabled = load_settings(&directory);
    let mut loaded = Vec::new();
    for path in discover_mods(&directory, own_path.as_deref()) {
        if mod_file_key(&path).is_some_and(|key| disabled.contains(&key)) {
            write_log(
                DnhLogLevel::Info,
                &format!("disabled by native-mods.json: {}", path.display()),
            );
            continue;
        }
        match load_one(&path) {
            Ok(module) => {
                if loaded
                    .iter()
                    .any(|existing: &LoadedMod| existing.id == module.id)
                {
                    write_log(
                        DnhLogLevel::Warn,
                        &format!(
                            "duplicate mod id '{}' in {}; skipped",
                            module.id,
                            path.display()
                        ),
                    );
                    unsafe {
                        (module.unload)();
                        remove_hooks_for_module(module.module);
                        FreeLibrary(module.module);
                    }
                    continue;
                }
                write_log(
                    DnhLogLevel::Info,
                    &format!("loaded {} {} ({})", module.name, module.version, module.id),
                );
                loaded.push(module);
            }
            Err(error) => write_log(
                DnhLogLevel::Warn,
                &format!("skipped {}: {error}", path.display()),
            ),
        }
    }

    write_log(
        DnhLogLevel::Info,
        &format!("startup complete: {} mod(s) loaded", loaded.len()),
    );
    let ui = match UiController::start() {
        Ok(ui) => {
            write_log(
                DnhLogLevel::Info,
                "native mod manager ready; press F1 to open",
            );
            Some(ui)
        }
        Err(error) => {
            write_log(
                DnhLogLevel::Error,
                &format!("native mod manager unavailable: {error}"),
            );
            None
        }
    };
    let (worker_sender, worker_receiver) = channel();
    *state_guard = Some(HostState {
        mod_directory: directory,
        control_directory,
        control_tick: 0,
        f1_down: false,
        disabled,
        mods: loaded,
        ui,
        worker_sender,
        worker_receiver,
        workers: Vec::new(),
    });
    if let Some(state) = state_guard.as_ref() {
        let descriptor = state.mod_directory.join("native-control.json");
        let _ = std::fs::write(descriptor, control_status(state));
    }
    DNH_OK
}

#[unsafe(no_mangle)]
pub extern "system" fn DNH_Tick() {
    let Ok(mut guard) = state().lock() else {
        return;
    };
    let Some(state) = guard.as_mut() else {
        return;
    };
    process_manager(state);
    process_control_requests(state);
    for loaded_mod in &state.mods {
        if catch_unwind(AssertUnwindSafe(|| unsafe { (loaded_mod.tick)() })).is_err() {
            write_log(
                DnhLogLevel::Error,
                &format!("mod '{}' panicked in DNM_Tick", loaded_mod.id),
            );
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn DNH_Shutdown() {
    let Ok(mut guard) = state().lock() else {
        return;
    };
    let Some(mut current) = guard.take() else {
        return;
    };

    if let Some(ui) = current.ui.take() {
        ui.shutdown();
    }
    for worker in current.workers.drain(..) {
        let _ = worker.join();
    }
    for loaded_mod in current.mods.drain(..).rev() {
        let _ = catch_unwind(AssertUnwindSafe(|| unsafe { (loaded_mod.unload)() }));
        remove_hooks_for_module(loaded_mod.module);
        unsafe {
            FreeLibrary(loaded_mod.module);
        }
        write_log(DnhLogLevel::Info, &format!("unloaded {}", loaded_mod.id));
    }
    write_log(
        DnhLogLevel::Info,
        &format!(
            "Dofus Native Host stopped ({})",
            current.mod_directory.display()
        ),
    );
    let _ = std::fs::remove_file(current.mod_directory.join("native-control.json"));
    if let Ok(mut guard) = logger().lock() {
        *guard = None;
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn DNH_GetLoadedModCount() -> usize {
    state()
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(|state| state.mods.len()))
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
/// Copies the latest initialization error as UTF-8.
///
/// # Safety
///
/// When non-null, `output` must be writable for `capacity` bytes.
pub unsafe extern "system" fn DNH_CopyLastError(output: *mut u8, capacity: usize) -> usize {
    let Ok(error) = last_error().lock() else {
        return 0;
    };
    let bytes = error.as_bytes();
    if !output.is_null() && capacity != 0 {
        let count = bytes.len().min(capacity.saturating_sub(1));
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, count);
            *output.add(count) = 0;
        }
    }
    bytes.len()
}

#[unsafe(no_mangle)]
/// Windows loader entry point. It deliberately performs no runtime startup.
///
/// # Safety
///
/// This function is called by the Windows loader with loader-owned handles.
pub unsafe extern "system" fn DllMain(module: HModule, reason: DWord, _reserved: HModule) -> Bool {
    if reason == DLL_PROCESS_ATTACH {
        OWN_MODULE.store(module, Ordering::Release);
        unsafe {
            DisableThreadLibraryCalls(module);
        }
    }
    1
}
