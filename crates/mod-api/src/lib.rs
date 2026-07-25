#![no_std]

use core::ffi::{c_char, c_void};

pub const DNH_ABI_VERSION_1: u32 = 1;
pub const DNH_ABI_VERSION_2: u32 = 2;
pub const DNH_ABI_VERSION_3: u32 = 3;
pub const DNH_ABI_VERSION_4: u32 = 4;
pub const DNH_ABI_VERSION: u32 = DNH_ABI_VERSION_4;
pub const DNH_OK: i32 = 0;
pub const DNH_ERROR: i32 = -1;

pub type DnhHandle = *mut c_void;
pub type DnhGcHandleV4 = usize;

#[repr(u32)]
#[derive(Clone, Copy)]
pub enum DnhLogLevel {
    Trace = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
}

pub type DnhLogFn =
    unsafe extern "system" fn(level: DnhLogLevel, message: *const u8, message_len: usize);
pub type DnhUnityIsReadyFn = unsafe extern "system" fn() -> bool;
pub type DnhUnityThreadAttachFn = unsafe extern "system" fn() -> DnhHandle;
pub type DnhUnityThreadDetachFn = unsafe extern "system" fn(thread: DnhHandle);
pub type DnhUnityGetClassFn = unsafe extern "system" fn(
    assembly: *const c_char,
    namespace_name: *const c_char,
    class_name: *const c_char,
) -> DnhHandle;
pub type DnhUnityGetFieldFn =
    unsafe extern "system" fn(class_handle: DnhHandle, field_name: *const c_char) -> DnhHandle;
pub type DnhUnityGetMethodFn = unsafe extern "system" fn(
    class_handle: DnhHandle,
    method_name: *const c_char,
    parameter_count: i32,
) -> DnhHandle;
pub type DnhUnityFieldOffsetFn = unsafe extern "system" fn(field_handle: DnhHandle) -> i32;
pub type DnhUnityMethodAddressFn = unsafe extern "system" fn(method_handle: DnhHandle) -> DnhHandle;
pub type DnhUnityFindObjectsFn = unsafe extern "system" fn(
    class_handle: DnhHandle,
    output: *mut DnhHandle,
    capacity: usize,
) -> usize;
pub type DnhUnityGetClassesFn = unsafe extern "system" fn(
    assembly: *const c_char,
    output: *mut DnhHandle,
    capacity: usize,
) -> usize;
pub type DnhUnityGetObjectClassFn = unsafe extern "system" fn(object: DnhHandle) -> DnhHandle;
pub type DnhUnityClassTypeObjectFn =
    unsafe extern "system" fn(class_handle: DnhHandle) -> DnhHandle;
pub type DnhUnityCopyClassInfoFn =
    unsafe extern "system" fn(class_handle: DnhHandle, output: *mut DnhClassInfoV2) -> bool;
pub type DnhUnityGetClassMembersFn = unsafe extern "system" fn(
    class_handle: DnhHandle,
    output: *mut DnhHandle,
    capacity: usize,
) -> usize;
pub type DnhUnityCopyFieldInfoFn =
    unsafe extern "system" fn(field_handle: DnhHandle, output: *mut DnhFieldInfoV2) -> bool;
pub type DnhUnityCopyMethodInfoFn =
    unsafe extern "system" fn(method_handle: DnhHandle, output: *mut DnhMethodInfoV2) -> bool;
pub type DnhUnityMethodParameterTypeNameFn =
    unsafe extern "system" fn(method_handle: DnhHandle, index: u32) -> *const c_char;
pub type DnhUnityRuntimeInvokeFn = unsafe extern "system" fn(
    method_handle: DnhHandle,
    object: DnhHandle,
    arguments: *const DnhHandle,
    exception: *mut DnhHandle,
) -> DnhHandle;
pub type DnhUnityUnaryHandleFn = unsafe extern "system" fn(value: DnhHandle) -> DnhHandle;
pub type DnhUnityStringNewUtf8Fn = unsafe extern "system" fn(value: *const c_char) -> DnhHandle;
pub type DnhUnityArrayNewFn =
    unsafe extern "system" fn(element_class: DnhHandle, length: usize) -> DnhHandle;
pub type DnhUnityArrayLengthFn = unsafe extern "system" fn(array: DnhHandle) -> usize;
pub type DnhUnityClassValueSizeFn =
    unsafe extern "system" fn(class_handle: DnhHandle, alignment: *mut u32) -> i32;
pub type DnhUnityClassIsValueTypeFn = unsafe extern "system" fn(class_handle: DnhHandle) -> bool;
pub type DnhUnityFieldValueFn = unsafe extern "system" fn(
    object: DnhHandle,
    field_handle: DnhHandle,
    output: *mut c_void,
) -> bool;
pub type DnhUnityFieldSetValueFn = unsafe extern "system" fn(
    object: DnhHandle,
    field_handle: DnhHandle,
    value: *const c_void,
) -> bool;
pub type DnhUnityFieldGetValueObjectFn =
    unsafe extern "system" fn(object: DnhHandle, field_handle: DnhHandle) -> DnhHandle;
pub type DnhUnityStaticFieldValueFn =
    unsafe extern "system" fn(field_handle: DnhHandle, output: *mut c_void) -> bool;
pub type DnhUnityStaticFieldSetValueFn =
    unsafe extern "system" fn(field_handle: DnhHandle, value: *const c_void) -> bool;
pub type DnhUnityGcHandleNewFn = unsafe extern "system" fn(object: DnhHandle, pinned: bool) -> u32;
pub type DnhUnityGcHandleTargetFn = unsafe extern "system" fn(handle: u32) -> DnhHandle;
pub type DnhUnityGcHandleFreeFn = unsafe extern "system" fn(handle: u32);
pub type DnhUnityResolveIcallFn = unsafe extern "system" fn(name: *const c_char) -> DnhHandle;

#[repr(C)]
pub struct DnhClassInfoV2 {
    pub struct_size: u32,
    pub name: *const c_char,
    pub namespace_name: *const c_char,
    pub parent_name: *const c_char,
    pub field_count: u32,
    pub method_count: u32,
    pub is_value_type: u8,
    pub reserved: [u8; 3],
}

#[repr(C)]
pub struct DnhFieldInfoV2 {
    pub struct_size: u32,
    pub name: *const c_char,
    pub type_name: *const c_char,
    pub offset: i32,
    pub is_static: u8,
    pub reserved: [u8; 3],
}

#[repr(C)]
pub struct DnhMethodInfoV2 {
    pub struct_size: u32,
    pub name: *const c_char,
    pub return_type_name: *const c_char,
    pub parameter_count: u32,
    pub flags: u32,
    pub is_static: u8,
    pub reserved: [u8; 3],
}

#[repr(C)]
pub struct DnhUnityApiV1 {
    pub abi_version: u32,
    pub struct_size: u32,
    pub is_ready: DnhUnityIsReadyFn,
    pub thread_attach: DnhUnityThreadAttachFn,
    pub thread_detach: DnhUnityThreadDetachFn,
    pub get_class: DnhUnityGetClassFn,
    pub get_field: DnhUnityGetFieldFn,
    pub get_method: DnhUnityGetMethodFn,
    pub field_offset: DnhUnityFieldOffsetFn,
    pub method_address: DnhUnityMethodAddressFn,
    pub find_objects: DnhUnityFindObjectsFn,
}

#[repr(C)]
pub struct DnhUnityApiV2 {
    pub abi_version: u32,
    pub struct_size: u32,
    pub is_ready: DnhUnityIsReadyFn,
    pub thread_attach: DnhUnityThreadAttachFn,
    pub thread_detach: DnhUnityThreadDetachFn,
    pub get_class: DnhUnityGetClassFn,
    pub get_field: DnhUnityGetFieldFn,
    pub get_method: DnhUnityGetMethodFn,
    pub field_offset: DnhUnityFieldOffsetFn,
    pub method_address: DnhUnityMethodAddressFn,
    pub find_objects: DnhUnityFindObjectsFn,
    pub get_classes: DnhUnityGetClassesFn,
    pub get_object_class: DnhUnityGetObjectClassFn,
    pub class_type_object: DnhUnityClassTypeObjectFn,
    pub copy_class_info: DnhUnityCopyClassInfoFn,
    pub get_class_fields: DnhUnityGetClassMembersFn,
    pub get_class_methods: DnhUnityGetClassMembersFn,
    pub copy_field_info: DnhUnityCopyFieldInfoFn,
    pub copy_method_info: DnhUnityCopyMethodInfoFn,
    pub method_parameter_type_name: DnhUnityMethodParameterTypeNameFn,
    pub runtime_invoke: DnhUnityRuntimeInvokeFn,
    pub object_unbox: DnhUnityUnaryHandleFn,
    pub object_new: DnhUnityUnaryHandleFn,
    pub string_new_utf8: DnhUnityStringNewUtf8Fn,
    pub array_new: DnhUnityArrayNewFn,
    pub array_length: DnhUnityArrayLengthFn,
    pub array_data: DnhUnityUnaryHandleFn,
    pub class_value_size: DnhUnityClassValueSizeFn,
    pub class_is_value_type: DnhUnityClassIsValueTypeFn,
    pub field_get_value: DnhUnityFieldValueFn,
    pub field_set_value: DnhUnityFieldSetValueFn,
    pub field_get_value_object: DnhUnityFieldGetValueObjectFn,
    pub field_get_static_value: DnhUnityStaticFieldValueFn,
    pub field_set_static_value: DnhUnityStaticFieldSetValueFn,
    pub gc_handle_new: DnhUnityGcHandleNewFn,
    pub gc_handle_target: DnhUnityGcHandleTargetFn,
    pub gc_handle_free: DnhUnityGcHandleFreeFn,
    pub resolve_icall: DnhUnityResolveIcallFn,
}

pub const DNH_MEMBER_STORAGE_ANY: u8 = 0;
pub const DNH_MEMBER_STORAGE_INSTANCE: u8 = 1;
pub const DNH_MEMBER_STORAGE_STATIC: u8 = 2;

#[repr(C)]
pub struct DnhFieldSignatureV3 {
    pub struct_size: u32,
    pub name: *const c_char,
    pub type_name: *const c_char,
    pub minimum_offset: i32,
    pub maximum_offset: i32,
    pub storage: u8,
    pub reserved: [u8; 3],
}

#[repr(C)]
pub struct DnhMethodSignatureV3 {
    pub struct_size: u32,
    pub name: *const c_char,
    pub return_type_name: *const c_char,
    pub parameter_count: i32,
    pub parameter_type_names: *const *const c_char,
    pub parameter_type_count: u32,
    pub storage: u8,
    pub reserved: [u8; 3],
}

#[repr(C)]
pub struct DnhClassSignatureV3 {
    pub struct_size: u32,
    pub name: *const c_char,
    pub namespace_name: *const c_char,
    pub parent_name: *const c_char,
    pub required_fields: *const DnhFieldSignatureV3,
    pub required_field_count: u32,
    pub required_methods: *const DnhMethodSignatureV3,
    pub required_method_count: u32,
    pub minimum_field_count: u32,
    pub minimum_method_count: u32,
}

pub type DnhUnityFindFieldsBySignatureFn = unsafe extern "system" fn(
    class_handle: DnhHandle,
    signature: *const DnhFieldSignatureV3,
    output: *mut DnhHandle,
    capacity: usize,
) -> usize;
pub type DnhUnityFindMethodsBySignatureFn = unsafe extern "system" fn(
    class_handle: DnhHandle,
    signature: *const DnhMethodSignatureV3,
    output: *mut DnhHandle,
    capacity: usize,
) -> usize;
pub type DnhUnityFindClassesBySignatureFn = unsafe extern "system" fn(
    assembly: *const c_char,
    signature: *const DnhClassSignatureV3,
    output: *mut DnhHandle,
    capacity: usize,
) -> usize;

#[repr(C)]
pub struct DnhUnityApiV3 {
    pub v2: DnhUnityApiV2,
    pub find_fields_by_signature: DnhUnityFindFieldsBySignatureFn,
    pub find_methods_by_signature: DnhUnityFindMethodsBySignatureFn,
    pub find_classes_by_signature: DnhUnityFindClassesBySignatureFn,
}

pub type DnhUnityGcHandleNewV4Fn =
    unsafe extern "system" fn(object: DnhHandle, pinned: bool) -> DnhGcHandleV4;
pub type DnhUnityGcHandleTargetV4Fn = unsafe extern "system" fn(handle: DnhGcHandleV4) -> DnhHandle;
pub type DnhUnityGcHandleFreeV4Fn = unsafe extern "system" fn(handle: DnhGcHandleV4);

#[repr(C)]
pub struct DnhUnityApiV4 {
    pub v3: DnhUnityApiV3,
    pub gc_handle_new_v4: DnhUnityGcHandleNewV4Fn,
    pub gc_handle_target_v4: DnhUnityGcHandleTargetV4Fn,
    pub gc_handle_free_v4: DnhUnityGcHandleFreeV4Fn,
}

#[repr(C)]
pub struct DnhHostApiV1 {
    pub abi_version: u32,
    pub struct_size: u32,
    pub log: DnhLogFn,
    pub unity: *const DnhUnityApiV1,
}

#[repr(C)]
pub struct DnhHostApiV2 {
    pub abi_version: u32,
    pub struct_size: u32,
    pub log: DnhLogFn,
    pub unity: *const DnhUnityApiV2,
}

#[repr(C)]
pub struct DnhHostApiV3 {
    pub abi_version: u32,
    pub struct_size: u32,
    pub log: DnhLogFn,
    pub unity: *const DnhUnityApiV3,
}

#[repr(C)]
pub struct DnhHostApiV4 {
    pub abi_version: u32,
    pub struct_size: u32,
    pub log: DnhLogFn,
    pub unity: *const DnhUnityApiV4,
}

#[repr(C)]
pub struct DnhModInfoV1 {
    pub abi_version: u32,
    pub struct_size: u32,
    pub id: *const c_char,
    pub name: *const c_char,
    pub version: *const c_char,
    pub author: *const c_char,
}

pub type DnmQueryFn = unsafe extern "system" fn(host_abi_version: u32) -> *const DnhModInfoV1;
pub type DnmLoadFn = unsafe extern "system" fn(host_api: *const DnhHostApiV1) -> i32;
pub type DnmTickFn = unsafe extern "system" fn();
pub type DnmUnloadFn = unsafe extern "system" fn();

pub const QUERY_EXPORT: &[u8] = b"DNM_Query\0";
pub const LOAD_EXPORT: &[u8] = b"DNM_Load\0";
pub const TICK_EXPORT: &[u8] = b"DNM_Tick\0";
pub const UNLOAD_EXPORT: &[u8] = b"DNM_Unload\0";
