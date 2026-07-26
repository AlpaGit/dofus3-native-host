#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _WIN32
#define DNH_CALL __stdcall
#define DNH_EXPORT __declspec(dllexport)
#else
#define DNH_CALL
#define DNH_EXPORT
#endif

#define DNH_ABI_VERSION_1 1u
#define DNH_ABI_VERSION_2 2u
#define DNH_ABI_VERSION_3 3u
#define DNH_ABI_VERSION_4 4u
#define DNH_ABI_VERSION_5 5u
#define DNH_ABI_VERSION_6 6u
#define DNH_ABI_VERSION_7 7u
#define DNH_ABI_VERSION_8 8u
#define DNH_ABI_VERSION DNH_ABI_VERSION_8
#define DNH_OK 0
#define DNH_ERROR (-1)

typedef void* DnhHandle;
typedef uintptr_t DnhGcHandleV4;

typedef enum DnhLogLevel {
    DNH_LOG_TRACE = 0,
    DNH_LOG_INFO = 1,
    DNH_LOG_WARN = 2,
    DNH_LOG_ERROR = 3,
} DnhLogLevel;

typedef struct DnhUnityApiV1 {
    uint32_t abi_version;
    uint32_t struct_size;
    bool(DNH_CALL* is_ready)(void);
    DnhHandle(DNH_CALL* thread_attach)(void);
    void(DNH_CALL* thread_detach)(DnhHandle thread);
    DnhHandle(DNH_CALL* get_class)(
        const char* assembly,
        const char* namespace_name,
        const char* class_name);
    DnhHandle(DNH_CALL* get_field)(DnhHandle class_handle, const char* field_name);
    DnhHandle(DNH_CALL* get_method)(
        DnhHandle class_handle,
        const char* method_name,
        int32_t parameter_count);
    int32_t(DNH_CALL* field_offset)(DnhHandle field_handle);
    DnhHandle(DNH_CALL* method_address)(DnhHandle method_handle);
    size_t(DNH_CALL* find_objects)(
        DnhHandle class_handle,
        DnhHandle* output,
        size_t capacity);
} DnhUnityApiV1;

typedef struct DnhClassInfoV2 {
    uint32_t struct_size;
    const char* name;
    const char* namespace_name;
    const char* parent_name;
    uint32_t field_count;
    uint32_t method_count;
    uint8_t is_value_type;
    uint8_t reserved[3];
} DnhClassInfoV2;

typedef struct DnhFieldInfoV2 {
    uint32_t struct_size;
    const char* name;
    const char* type_name;
    int32_t offset;
    uint8_t is_static;
    uint8_t reserved[3];
} DnhFieldInfoV2;

typedef struct DnhMethodInfoV2 {
    uint32_t struct_size;
    const char* name;
    const char* return_type_name;
    uint32_t parameter_count;
    uint32_t flags;
    uint8_t is_static;
    uint8_t reserved[3];
} DnhMethodInfoV2;

typedef struct DnhUnityApiV2 {
    uint32_t abi_version;
    uint32_t struct_size;
    bool(DNH_CALL* is_ready)(void);
    DnhHandle(DNH_CALL* thread_attach)(void);
    void(DNH_CALL* thread_detach)(DnhHandle thread);
    DnhHandle(DNH_CALL* get_class)(
        const char* assembly,
        const char* namespace_name,
        const char* class_name);
    DnhHandle(DNH_CALL* get_field)(DnhHandle class_handle, const char* field_name);
    DnhHandle(DNH_CALL* get_method)(
        DnhHandle class_handle,
        const char* method_name,
        int32_t parameter_count);
    int32_t(DNH_CALL* field_offset)(DnhHandle field_handle);
    DnhHandle(DNH_CALL* method_address)(DnhHandle method_handle);
    size_t(DNH_CALL* find_objects)(
        DnhHandle class_handle,
        DnhHandle* output,
        size_t capacity);

    size_t(DNH_CALL* get_classes)(
        const char* assembly,
        DnhHandle* output,
        size_t capacity);
    DnhHandle(DNH_CALL* get_object_class)(DnhHandle object);
    DnhHandle(DNH_CALL* class_type_object)(DnhHandle class_handle);
    bool(DNH_CALL* copy_class_info)(
        DnhHandle class_handle,
        DnhClassInfoV2* output);
    size_t(DNH_CALL* get_class_fields)(
        DnhHandle class_handle,
        DnhHandle* output,
        size_t capacity);
    size_t(DNH_CALL* get_class_methods)(
        DnhHandle class_handle,
        DnhHandle* output,
        size_t capacity);
    bool(DNH_CALL* copy_field_info)(
        DnhHandle field_handle,
        DnhFieldInfoV2* output);
    bool(DNH_CALL* copy_method_info)(
        DnhHandle method_handle,
        DnhMethodInfoV2* output);
    const char*(DNH_CALL* method_parameter_type_name)(
        DnhHandle method_handle,
        uint32_t index);
    DnhHandle(DNH_CALL* runtime_invoke)(
        DnhHandle method_handle,
        DnhHandle object,
        const DnhHandle* arguments,
        DnhHandle* exception);
    DnhHandle(DNH_CALL* object_unbox)(DnhHandle object);
    DnhHandle(DNH_CALL* object_new)(DnhHandle class_handle);
    DnhHandle(DNH_CALL* string_new_utf8)(const char* value);
    DnhHandle(DNH_CALL* array_new)(DnhHandle element_class, size_t length);
    size_t(DNH_CALL* array_length)(DnhHandle array);
    DnhHandle(DNH_CALL* array_data)(DnhHandle array);
    int32_t(DNH_CALL* class_value_size)(
        DnhHandle class_handle,
        uint32_t* alignment);
    bool(DNH_CALL* class_is_value_type)(DnhHandle class_handle);
    bool(DNH_CALL* field_get_value)(
        DnhHandle object,
        DnhHandle field_handle,
        void* output);
    bool(DNH_CALL* field_set_value)(
        DnhHandle object,
        DnhHandle field_handle,
        const void* value);
    DnhHandle(DNH_CALL* field_get_value_object)(
        DnhHandle object,
        DnhHandle field_handle);
    bool(DNH_CALL* field_get_static_value)(
        DnhHandle field_handle,
        void* output);
    bool(DNH_CALL* field_set_static_value)(
        DnhHandle field_handle,
        const void* value);
    uint32_t(DNH_CALL* gc_handle_new)(DnhHandle object, bool pinned);
    DnhHandle(DNH_CALL* gc_handle_target)(uint32_t handle);
    void(DNH_CALL* gc_handle_free)(uint32_t handle);
    DnhHandle(DNH_CALL* resolve_icall)(const char* name);
} DnhUnityApiV2;

typedef enum DnhMemberStorageV3 {
    DNH_MEMBER_STORAGE_ANY = 0,
    DNH_MEMBER_STORAGE_INSTANCE = 1,
    DNH_MEMBER_STORAGE_STATIC = 2,
} DnhMemberStorageV3;

typedef struct DnhFieldSignatureV3 {
    uint32_t struct_size;
    const char* name;
    const char* type_name;
    int32_t minimum_offset;
    int32_t maximum_offset;
    uint8_t storage;
    uint8_t reserved[3];
} DnhFieldSignatureV3;

typedef struct DnhMethodSignatureV3 {
    uint32_t struct_size;
    const char* name;
    const char* return_type_name;
    int32_t parameter_count;
    const char* const* parameter_type_names;
    uint32_t parameter_type_count;
    uint8_t storage;
    uint8_t reserved[3];
} DnhMethodSignatureV3;

typedef struct DnhClassSignatureV3 {
    uint32_t struct_size;
    const char* name;
    const char* namespace_name;
    const char* parent_name;
    const DnhFieldSignatureV3* required_fields;
    uint32_t required_field_count;
    const DnhMethodSignatureV3* required_methods;
    uint32_t required_method_count;
    uint32_t minimum_field_count;
    uint32_t minimum_method_count;
} DnhClassSignatureV3;

typedef struct DnhUnityApiV3 {
    DnhUnityApiV2 v2;
    size_t(DNH_CALL* find_fields_by_signature)(
        DnhHandle class_handle,
        const DnhFieldSignatureV3* signature,
        DnhHandle* output,
        size_t capacity);
    size_t(DNH_CALL* find_methods_by_signature)(
        DnhHandle class_handle,
        const DnhMethodSignatureV3* signature,
        DnhHandle* output,
        size_t capacity);
    size_t(DNH_CALL* find_classes_by_signature)(
        const char* assembly,
        const DnhClassSignatureV3* signature,
        DnhHandle* output,
        size_t capacity);
} DnhUnityApiV3;

typedef struct DnhUnityApiV4 {
    DnhUnityApiV3 v3;
    DnhGcHandleV4(DNH_CALL* gc_handle_new_v4)(
        DnhHandle object,
        bool pinned);
    DnhHandle(DNH_CALL* gc_handle_target_v4)(DnhGcHandleV4 handle);
    void(DNH_CALL* gc_handle_free_v4)(DnhGcHandleV4 handle);
} DnhUnityApiV4;

typedef DnhHandle(DNH_CALL* DnhUnityInflateGenericMethodFn)(
    DnhHandle method_handle,
    const DnhHandle* type_arguments,
    size_t type_argument_count);

typedef struct DnhUnityApiV6 {
    DnhUnityApiV4 v4;
    DnhUnityInflateGenericMethodFn inflate_generic_method;
} DnhUnityApiV6;

typedef struct DnhUnityApiV7 {
    DnhUnityApiV6 v6;
    bool(DNH_CALL* class_is_assignable_from)(
        DnhHandle base_class,
        DnhHandle candidate_class);
    size_t(DNH_CALL* copy_string_utf8)(
        DnhHandle string_object,
        uint8_t* output,
        size_t capacity);
    DnhHandle(DNH_CALL* runtime_invoke_virtual)(
        DnhHandle method_handle,
        DnhHandle object,
        const DnhHandle* arguments,
        DnhHandle* exception);
} DnhUnityApiV7;

typedef struct DnhUnityApiV8 {
    DnhUnityApiV7 v7;
    DnhHandle(DNH_CALL* class_parent)(DnhHandle class_handle);
} DnhUnityApiV8;

typedef struct DnhHostApiV1 {
    uint32_t abi_version;
    uint32_t struct_size;
    void(DNH_CALL* log)(
        DnhLogLevel level,
        const uint8_t* message,
        size_t message_len);
    const DnhUnityApiV1* unity;
} DnhHostApiV1;

typedef struct DnhHostApiV2 {
    uint32_t abi_version;
    uint32_t struct_size;
    void(DNH_CALL* log)(
        DnhLogLevel level,
        const uint8_t* message,
        size_t message_len);
    const DnhUnityApiV2* unity;
} DnhHostApiV2;

typedef struct DnhHostApiV3 {
    uint32_t abi_version;
    uint32_t struct_size;
    void(DNH_CALL* log)(
        DnhLogLevel level,
        const uint8_t* message,
        size_t message_len);
    const DnhUnityApiV3* unity;
} DnhHostApiV3;

typedef struct DnhHostApiV4 {
    uint32_t abi_version;
    uint32_t struct_size;
    void(DNH_CALL* log)(
        DnhLogLevel level,
        const uint8_t* message,
        size_t message_len);
    const DnhUnityApiV4* unity;
} DnhHostApiV4;

typedef struct DnhHookApiV5 {
    uint32_t struct_size;
    int32_t(DNH_CALL* create)(
        DnhHandle target,
        DnhHandle detour,
        DnhHandle* original);
    int32_t(DNH_CALL* enable)(DnhHandle target);
    int32_t(DNH_CALL* disable)(DnhHandle target);
    int32_t(DNH_CALL* remove)(DnhHandle target);
} DnhHookApiV5;

typedef struct DnhHostApiV5 {
    DnhHostApiV4 v4;
    const DnhHookApiV5* hooks;
} DnhHostApiV5;

typedef struct DnhHostApiV6 {
    uint32_t abi_version;
    uint32_t struct_size;
    void(DNH_CALL* log)(
        DnhLogLevel level,
        const uint8_t* message,
        size_t message_len);
    const DnhUnityApiV6* unity;
    const DnhHookApiV5* hooks;
} DnhHostApiV6;

typedef struct DnhHostApiV7 {
    uint32_t abi_version;
    uint32_t struct_size;
    void(DNH_CALL* log)(
        DnhLogLevel level,
        const uint8_t* message,
        size_t message_len);
    const DnhUnityApiV7* unity;
    const DnhHookApiV5* hooks;
} DnhHostApiV7;

typedef struct DnhHostApiV8 {
    uint32_t abi_version;
    uint32_t struct_size;
    void(DNH_CALL* log)(
        DnhLogLevel level,
        const uint8_t* message,
        size_t message_len);
    const DnhUnityApiV8* unity;
    const DnhHookApiV5* hooks;
} DnhHostApiV8;

typedef struct DnhModInfoV1 {
    uint32_t abi_version;
    uint32_t struct_size;
    const char* id;
    const char* name;
    const char* version;
    const char* author;
} DnhModInfoV1;

DNH_EXPORT const DnhModInfoV1* DNH_CALL DNM_Query(uint32_t host_abi_version);
DNH_EXPORT int32_t DNH_CALL DNM_Load(const DnhHostApiV1* host_api);
DNH_EXPORT void DNH_CALL DNM_Tick(void);
DNH_EXPORT void DNH_CALL DNM_Unload(void);

#ifdef __cplusplus
}
#endif
