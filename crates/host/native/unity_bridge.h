#pragma once

#include <cstddef>
#include <cstdint>

#include "../../../include/dofus_native_mod_api.h"

extern "C" {

bool dnh_unity_initialize(void* game_assembly);
bool dnh_unity_is_ready();
void* dnh_unity_thread_attach();
void dnh_unity_thread_detach(void* thread);
void* dnh_unity_get_class(
    const char* assembly,
    const char* namespace_name,
    const char* class_name);
void* dnh_unity_get_field(void* class_handle, const char* field_name);
void* dnh_unity_get_method(
    void* class_handle,
    const char* method_name,
    std::int32_t parameter_count);
std::int32_t dnh_unity_field_offset(void* field_handle);
void* dnh_unity_method_address(void* method_handle);
std::size_t dnh_unity_find_objects(
    void* class_handle,
    void** output,
    std::size_t capacity);
std::size_t dnh_unity_get_classes(
    const char* assembly,
    void** output,
    std::size_t capacity);
void* dnh_unity_get_object_class(void* object);
void* dnh_unity_class_type_object(void* class_handle);
bool dnh_unity_copy_class_info(void* class_handle, DnhClassInfoV2* output);
std::size_t dnh_unity_get_class_fields(
    void* class_handle,
    void** output,
    std::size_t capacity);
std::size_t dnh_unity_get_class_methods(
    void* class_handle,
    void** output,
    std::size_t capacity);
bool dnh_unity_copy_field_info(void* field_handle, DnhFieldInfoV2* output);
bool dnh_unity_copy_method_info(void* method_handle, DnhMethodInfoV2* output);
const char* dnh_unity_method_parameter_type_name(
    void* method_handle,
    std::uint32_t index);
void* dnh_unity_runtime_invoke(
    void* method_handle,
    void* object,
    void* const* arguments,
    void** exception);
void* dnh_unity_object_unbox(void* object);
void* dnh_unity_object_new(void* class_handle);
void* dnh_unity_string_new_utf8(const char* value);
void* dnh_unity_array_new(void* element_class, std::size_t length);
std::size_t dnh_unity_array_length(void* array);
void* dnh_unity_array_data(void* array);
std::int32_t dnh_unity_class_value_size(
    void* class_handle,
    std::uint32_t* alignment);
bool dnh_unity_class_is_value_type(void* class_handle);
bool dnh_unity_field_get_value(
    void* object,
    void* field_handle,
    void* output);
bool dnh_unity_field_set_value(
    void* object,
    void* field_handle,
    const void* value);
void* dnh_unity_field_get_value_object(void* object, void* field_handle);
bool dnh_unity_field_get_static_value(void* field_handle, void* output);
bool dnh_unity_field_set_static_value(void* field_handle, const void* value);
std::uint32_t dnh_unity_gc_handle_new(void* object, bool pinned);
void* dnh_unity_gc_handle_target(std::uint32_t handle);
void dnh_unity_gc_handle_free(std::uint32_t handle);
std::uintptr_t dnh_unity_gc_handle_new_v4(void* object, bool pinned);
void* dnh_unity_gc_handle_target_v4(std::uintptr_t handle);
void dnh_unity_gc_handle_free_v4(std::uintptr_t handle);
void* dnh_unity_resolve_icall(const char* name);
void* dnh_unity_inflate_generic_method(
    void* method_handle,
    void* const* type_arguments,
    std::size_t type_argument_count);
bool dnh_unity_class_is_assignable_from(
    void* base_class,
    void* candidate_class);
std::size_t dnh_unity_copy_string_utf8(
    void* string_object,
    std::uint8_t* output,
    std::size_t capacity);
void* dnh_unity_runtime_invoke_virtual(
    void* method_handle,
    void* object,
    void* const* arguments,
    void** exception);
std::size_t dnh_unity_find_fields_by_signature(
    void* class_handle,
    const DnhFieldSignatureV3* signature,
    void** output,
    std::size_t capacity);
std::size_t dnh_unity_find_methods_by_signature(
    void* class_handle,
    const DnhMethodSignatureV3* signature,
    void** output,
    std::size_t capacity);
std::size_t dnh_unity_find_classes_by_signature(
    const char* assembly,
    const DnhClassSignatureV3* signature,
    void** output,
    std::size_t capacity);

}
