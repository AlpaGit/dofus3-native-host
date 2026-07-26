#include "unity_bridge.h"

#include "UnityResolve.hpp"

#include <algorithm>
#include <atomic>
#include <cstring>
#include <mutex>
#include <string>
#include <string_view>
#include <unordered_map>
#include <vector>

namespace {
std::atomic_bool g_ready{false};
std::atomic<void*> g_game_assembly{nullptr};
std::vector<UnityResolve::Class*> g_dynamic_classes;
std::vector<UnityResolve::Method*> g_dynamic_methods;
std::atomic_uint32_t g_next_legacy_gc_handle{1};
std::mutex g_legacy_gc_handles_mutex;
std::unordered_map<std::uint32_t, std::uintptr_t> g_legacy_gc_handles;

std::uintptr_t NewNativeGcHandle(void* object, const bool pinned) {
    if (object == nullptr) {
        return 0;
    }
    return reinterpret_cast<std::uintptr_t>(
        UnityResolve::Invoke<void*>(
            "il2cpp_gchandle_new",
            object,
            pinned));
}

void* NativeGcHandleTarget(const std::uintptr_t handle) {
    return handle == 0
        ? nullptr
        : UnityResolve::Invoke<void*>(
              "il2cpp_gchandle_get_target",
              reinterpret_cast<void*>(handle));
}

void FreeNativeGcHandle(const std::uintptr_t handle) {
    if (handle != 0) {
        UnityResolve::Invoke<void>(
            "il2cpp_gchandle_free",
            reinterpret_cast<void*>(handle));
    }
}

UnityResolve::Class* FindClassByAddress(void* address) {
    if (address == nullptr) {
        return nullptr;
    }
    for (auto* assembly : UnityResolve::assembly) {
        if (assembly == nullptr) {
            continue;
        }
        for (auto* klass : assembly->classes) {
            if (klass != nullptr && klass->address == address) {
                return klass;
            }
        }
    }
    for (auto* klass : g_dynamic_classes) {
        if (klass != nullptr && klass->address == address) {
            return klass;
        }
    }
    return nullptr;
}

UnityResolve::Class* ResolveClassByAddress(void* address) {
    if (address == nullptr) {
        return nullptr;
    }
    if (auto* existing = FindClassByAddress(address); existing != nullptr) {
        return existing;
    }

    auto* klass = new UnityResolve::Class{};
    klass->address = address;
    if (const char* name =
            UnityResolve::Invoke<const char*>("il2cpp_class_get_name", address);
        name != nullptr) {
        klass->name = name;
    }
    if (const char* namespace_name = UnityResolve::Invoke<const char*>(
            "il2cpp_class_get_namespace",
            address);
        namespace_name != nullptr) {
        klass->namespaze = namespace_name;
    }
    if (void* parent =
            UnityResolve::Invoke<void*>("il2cpp_class_get_parent", address);
        parent != nullptr) {
        if (const char* parent_name = UnityResolve::Invoke<const char*>(
                "il2cpp_class_get_name",
                parent);
            parent_name != nullptr) {
            klass->parent = parent_name;
        }
    }
    g_dynamic_classes.push_back(klass);

    void* field_iterator = nullptr;
    while (void* field = UnityResolve::Invoke<void*>(
               "il2cpp_class_get_fields",
               address,
               &field_iterator)) {
        auto* resolved = new UnityResolve::Field{};
        resolved->address = field;
        if (const char* name =
                UnityResolve::Invoke<const char*>("il2cpp_field_get_name", field);
            name != nullptr) {
            resolved->name = name;
        }
        resolved->type = new UnityResolve::Type{};
        resolved->type->address =
            UnityResolve::Invoke<void*>("il2cpp_field_get_type", field);
        if (const char* type_name = UnityResolve::Invoke<const char*>(
                "il2cpp_type_get_name",
                resolved->type->address);
            type_name != nullptr) {
            resolved->type->name = type_name;
        }
        resolved->type->size = -1;
        resolved->klass = klass;
        resolved->offset =
            UnityResolve::Invoke<int>("il2cpp_field_get_offset", field);
        const int flags =
            UnityResolve::Invoke<int>("il2cpp_field_get_flags", field);
        resolved->static_field = (flags & 0x10) != 0;
        klass->fields.push_back(resolved);
    }

    void* method_iterator = nullptr;
    while (void* method = UnityResolve::Invoke<void*>(
               "il2cpp_class_get_methods",
               address,
               &method_iterator)) {
        auto* resolved = new UnityResolve::Method{};
        resolved->address = method;
        if (const char* name = UnityResolve::Invoke<const char*>(
                "il2cpp_method_get_name",
                method);
            name != nullptr) {
            resolved->name = name;
        }
        resolved->klass = klass;
        resolved->return_type = new UnityResolve::Type{};
        resolved->return_type->address = UnityResolve::Invoke<void*>(
            "il2cpp_method_get_return_type",
            method);
        if (const char* return_name = UnityResolve::Invoke<const char*>(
                "il2cpp_type_get_name",
                resolved->return_type->address);
            return_name != nullptr) {
            resolved->return_type->name = return_name;
        }
        resolved->return_type->size = -1;
        int implementation_flags{};
        resolved->flags = UnityResolve::Invoke<int>(
            "il2cpp_method_get_flags",
            method,
            &implementation_flags);
        resolved->static_function = (resolved->flags & 0x10) != 0;
        resolved->function = *static_cast<void**>(method);

        const int parameter_count = UnityResolve::Invoke<int>(
            "il2cpp_method_get_param_count",
            method);
        for (int index = 0; index < parameter_count; ++index) {
            void* parameter_type = UnityResolve::Invoke<void*>(
                "il2cpp_method_get_param",
                method,
                index);
            auto* type = new UnityResolve::Type{};
            type->address = parameter_type;
            if (const char* type_name = UnityResolve::Invoke<const char*>(
                    "il2cpp_type_get_name",
                    parameter_type);
                type_name != nullptr) {
                type->name = type_name;
            }
            type->size = -1;
            const char* parameter_name = UnityResolve::Invoke<const char*>(
                "il2cpp_method_get_param_name",
                method,
                index);
            resolved->args.push_back(new UnityResolve::Method::Arg{
                parameter_name == nullptr ? "" : parameter_name,
                type,
            });
        }
        klass->methods.push_back(resolved);
    }
    return klass;
}

UnityResolve::Class* ParentOf(UnityResolve::Class* klass) {
    if (klass == nullptr) {
        return nullptr;
    }
    return ResolveClassByAddress(
        UnityResolve::Invoke<void*>("il2cpp_class_get_parent", klass->address));
}

template <typename T>
std::size_t CopyHandles(
    const std::vector<T*>& values,
    void** output,
    const std::size_t capacity) {
    if (output != nullptr && capacity != 0) {
        const std::size_t count = std::min(capacity, values.size());
        for (std::size_t index = 0; index < count; ++index) {
            output[index] = values[index];
        }
    }
    return values.size();
}

bool TextMatches(const char* expected, const std::string& actual) {
    return expected == nullptr || expected[0] == '\0' ||
           expected == std::string_view{"*"} || actual == expected;
}

bool FieldMatches(
    const UnityResolve::Field* field,
    const DnhFieldSignatureV3* signature) {
    if (field == nullptr || signature == nullptr ||
        signature->struct_size < sizeof(DnhFieldSignatureV3)) {
        return false;
    }
    if (!TextMatches(signature->name, field->name) ||
        field->type == nullptr ||
        !TextMatches(signature->type_name, field->type->name)) {
        return false;
    }
    if (signature->storage == DNH_MEMBER_STORAGE_INSTANCE &&
        field->static_field) {
        return false;
    }
    if (signature->storage == DNH_MEMBER_STORAGE_STATIC &&
        !field->static_field) {
        return false;
    }
    return signature->minimum_offset > signature->maximum_offset ||
           (field->offset >= signature->minimum_offset &&
            field->offset <= signature->maximum_offset);
}

bool MethodMatches(
    const UnityResolve::Method* method,
    const DnhMethodSignatureV3* signature) {
    if (method == nullptr || signature == nullptr ||
        signature->struct_size < sizeof(DnhMethodSignatureV3) ||
        method->return_type == nullptr ||
        !TextMatches(signature->name, method->name) ||
        !TextMatches(signature->return_type_name, method->return_type->name)) {
        return false;
    }
    if (signature->storage == DNH_MEMBER_STORAGE_INSTANCE &&
        method->static_function) {
        return false;
    }
    if (signature->storage == DNH_MEMBER_STORAGE_STATIC &&
        !method->static_function) {
        return false;
    }
    if (signature->parameter_count >= 0 &&
        method->args.size() !=
            static_cast<std::size_t>(signature->parameter_count)) {
        return false;
    }
    if (signature->parameter_type_count != 0) {
        if (signature->parameter_type_names == nullptr ||
            method->args.size() != signature->parameter_type_count) {
            return false;
        }
        for (std::size_t index = 0;
             index < signature->parameter_type_count;
             ++index) {
            if (method->args[index] == nullptr ||
                method->args[index]->pType == nullptr ||
                !TextMatches(
                    signature->parameter_type_names[index],
                    method->args[index]->pType->name)) {
                return false;
            }
        }
    }
    return true;
}

bool ClassMatches(
    const UnityResolve::Class* klass,
    const DnhClassSignatureV3* signature) {
    if (klass == nullptr || signature == nullptr ||
        signature->struct_size < sizeof(DnhClassSignatureV3) ||
        !TextMatches(signature->name, klass->name) ||
        !TextMatches(signature->namespace_name, klass->namespaze) ||
        !TextMatches(signature->parent_name, klass->parent) ||
        klass->fields.size() < signature->minimum_field_count ||
        klass->methods.size() < signature->minimum_method_count) {
        return false;
    }
    if ((signature->required_field_count != 0 &&
         signature->required_fields == nullptr) ||
        (signature->required_method_count != 0 &&
         signature->required_methods == nullptr)) {
        return false;
    }
    for (std::size_t index = 0; index < signature->required_field_count;
         ++index) {
        const auto& required = signature->required_fields[index];
        if (std::none_of(
                klass->fields.begin(),
                klass->fields.end(),
                [&required](const UnityResolve::Field* field) {
                    return FieldMatches(field, &required);
                })) {
            return false;
        }
    }
    for (std::size_t index = 0; index < signature->required_method_count;
         ++index) {
        const auto& required = signature->required_methods[index];
        if (std::none_of(
                klass->methods.begin(),
                klass->methods.end(),
                [&required](const UnityResolve::Method* method) {
                    return MethodMatches(method, &required);
                })) {
            return false;
        }
    }
    return true;
}
}

extern "C" void* dnh_unity_class_parent(void* class_handle) {
    return ParentOf(static_cast<UnityResolve::Class*>(class_handle));
}

extern "C" bool dnh_unity_initialize(void* game_assembly) {
    if (g_ready.load(std::memory_order_acquire)) {
        return true;
    }
    if (game_assembly == nullptr) {
        return false;
    }

    g_game_assembly.store(game_assembly, std::memory_order_release);
    UnityResolve::Init(game_assembly, UnityResolve::Mode::Il2Cpp);
    const bool initialized = !UnityResolve::assembly.empty();
    g_ready.store(initialized, std::memory_order_release);
    return initialized;
}

extern "C" bool dnh_unity_is_ready() {
    return g_ready.load(std::memory_order_acquire);
}

extern "C" void* dnh_unity_thread_attach() {
    if (!dnh_unity_is_ready()) {
        return nullptr;
    }

    const auto module =
        static_cast<HMODULE>(g_game_assembly.load(std::memory_order_acquire));
    if (module == nullptr) {
        return nullptr;
    }
    const auto domain_get = reinterpret_cast<void* (*)()>(
        GetProcAddress(module, "il2cpp_domain_get"));
    const auto thread_attach = reinterpret_cast<void* (*)(void*)>(
        GetProcAddress(module, "il2cpp_thread_attach"));
    if (domain_get == nullptr || thread_attach == nullptr) {
        return nullptr;
    }
    return thread_attach(domain_get());
}

extern "C" void dnh_unity_thread_detach(void* thread) {
    if (thread == nullptr || !dnh_unity_is_ready()) {
        return;
    }
    const auto module =
        static_cast<HMODULE>(g_game_assembly.load(std::memory_order_acquire));
    if (module != nullptr) {
        const auto thread_detach = reinterpret_cast<void (*)(void*)>(
            GetProcAddress(module, "il2cpp_thread_detach"));
        if (thread_detach != nullptr) {
            thread_detach(thread);
        }
    }
}

extern "C" void* dnh_unity_get_class(
    const char* assembly,
    const char* namespace_name,
    const char* class_name) {
    if (!dnh_unity_is_ready() || assembly == nullptr || class_name == nullptr) {
        return nullptr;
    }

    auto* resolved_assembly = UnityResolve::Get(assembly);
    if (resolved_assembly == nullptr) {
        return nullptr;
    }
    const char* resolved_namespace =
        namespace_name == nullptr || namespace_name[0] == '\0' ? "*" : namespace_name;
    return resolved_assembly->Get(class_name, resolved_namespace);
}

extern "C" void* dnh_unity_get_field(void* class_handle, const char* field_name) {
    if (class_handle == nullptr || field_name == nullptr) {
        return nullptr;
    }
    for (auto* klass = static_cast<UnityResolve::Class*>(class_handle);
         klass != nullptr;
         klass = ParentOf(klass)) {
        if (auto* field = klass->Get<UnityResolve::Field>(field_name);
            field != nullptr) {
            return field;
        }
    }
    return nullptr;
}

extern "C" void* dnh_unity_get_method(
    void* class_handle,
    const char* method_name,
    std::int32_t parameter_count) {
    if (class_handle == nullptr || method_name == nullptr) {
        return nullptr;
    }

    for (auto* klass = static_cast<UnityResolve::Class*>(class_handle);
         klass != nullptr;
         klass = ParentOf(klass)) {
        for (auto* method : klass->methods) {
            if (method != nullptr && method->name == method_name &&
                (parameter_count < 0 ||
                 static_cast<std::int32_t>(method->args.size()) ==
                     parameter_count)) {
                return method;
            }
        }
    }
    return nullptr;
}

extern "C" std::int32_t dnh_unity_field_offset(void* field_handle) {
    if (field_handle == nullptr) {
        return -1;
    }
    return static_cast<UnityResolve::Field*>(field_handle)->offset;
}

extern "C" void* dnh_unity_method_address(void* method_handle) {
    if (method_handle == nullptr) {
        return nullptr;
    }
    return static_cast<UnityResolve::Method*>(method_handle)->function;
}

extern "C" std::size_t dnh_unity_find_objects(
    void* class_handle,
    void** output,
    std::size_t capacity) {
    if (class_handle == nullptr) {
        return 0;
    }

    auto* klass = static_cast<UnityResolve::Class*>(class_handle);
    const std::vector<void*> objects = klass->FindObjectsByType<void*>();
    if (output != nullptr && capacity != 0) {
        const std::size_t count = std::min(capacity, objects.size());
        std::copy_n(objects.begin(), count, output);
    }
    return objects.size();
}

extern "C" std::size_t dnh_unity_get_classes(
    const char* assembly,
    void** output,
    const std::size_t capacity) {
    if (!dnh_unity_is_ready()) {
        return 0;
    }

    if (assembly != nullptr && assembly[0] != '\0') {
        auto* resolved = UnityResolve::Get(assembly);
        return resolved == nullptr
            ? 0
            : CopyHandles(resolved->classes, output, capacity);
    }

    std::size_t total = 0;
    for (auto* resolved : UnityResolve::assembly) {
        if (resolved == nullptr) {
            continue;
        }
        for (auto* klass : resolved->classes) {
            if (output != nullptr && total < capacity) {
                output[total] = klass;
            }
            ++total;
        }
    }
    return total;
}

extern "C" void* dnh_unity_get_object_class(void* object) {
    if (object == nullptr || !dnh_unity_is_ready()) {
        return nullptr;
    }
    const auto address =
        UnityResolve::Invoke<void*>("il2cpp_object_get_class", object);
    return ResolveClassByAddress(address);
}

extern "C" void* dnh_unity_class_type_object(void* class_handle) {
    if (class_handle == nullptr || !dnh_unity_is_ready()) {
        return nullptr;
    }
    return static_cast<UnityResolve::Class*>(class_handle)->GetType();
}

extern "C" bool dnh_unity_copy_class_info(
    void* class_handle,
    DnhClassInfoV2* output) {
    if (class_handle == nullptr || output == nullptr ||
        output->struct_size < sizeof(DnhClassInfoV2)) {
        return false;
    }

    const auto* klass = static_cast<UnityResolve::Class*>(class_handle);
    output->struct_size = sizeof(DnhClassInfoV2);
    output->name = klass->name.c_str();
    output->namespace_name = klass->namespaze.c_str();
    output->parent_name = klass->parent.c_str();
    output->field_count = static_cast<std::uint32_t>(klass->fields.size());
    output->method_count = static_cast<std::uint32_t>(klass->methods.size());
    output->is_value_type = UnityResolve::Invoke<bool>(
        "il2cpp_class_is_valuetype",
        klass->address);
    std::memset(output->reserved, 0, sizeof(output->reserved));
    return true;
}

extern "C" std::size_t dnh_unity_get_class_fields(
    void* class_handle,
    void** output,
    const std::size_t capacity) {
    if (class_handle == nullptr) {
        return 0;
    }
    return CopyHandles(
        static_cast<UnityResolve::Class*>(class_handle)->fields,
        output,
        capacity);
}

extern "C" std::size_t dnh_unity_get_class_methods(
    void* class_handle,
    void** output,
    const std::size_t capacity) {
    if (class_handle == nullptr) {
        return 0;
    }
    return CopyHandles(
        static_cast<UnityResolve::Class*>(class_handle)->methods,
        output,
        capacity);
}

extern "C" bool dnh_unity_copy_field_info(
    void* field_handle,
    DnhFieldInfoV2* output) {
    if (field_handle == nullptr || output == nullptr ||
        output->struct_size < sizeof(DnhFieldInfoV2)) {
        return false;
    }

    const auto* field = static_cast<UnityResolve::Field*>(field_handle);
    output->struct_size = sizeof(DnhFieldInfoV2);
    output->name = field->name.c_str();
    output->type_name =
        field->type == nullptr ? nullptr : field->type->name.c_str();
    output->offset = field->offset;
    output->is_static = field->static_field;
    std::memset(output->reserved, 0, sizeof(output->reserved));
    return true;
}

extern "C" bool dnh_unity_copy_method_info(
    void* method_handle,
    DnhMethodInfoV2* output) {
    if (method_handle == nullptr || output == nullptr ||
        output->struct_size < sizeof(DnhMethodInfoV2)) {
        return false;
    }

    const auto* method = static_cast<UnityResolve::Method*>(method_handle);
    output->struct_size = sizeof(DnhMethodInfoV2);
    output->name = method->name.c_str();
    output->return_type_name =
        method->return_type == nullptr
            ? nullptr
            : method->return_type->name.c_str();
    output->parameter_count =
        static_cast<std::uint32_t>(method->args.size());
    output->flags = static_cast<std::uint32_t>(method->flags);
    output->is_static = method->static_function;
    std::memset(output->reserved, 0, sizeof(output->reserved));
    return true;
}

extern "C" const char* dnh_unity_method_parameter_type_name(
    void* method_handle,
    const std::uint32_t index) {
    if (method_handle == nullptr) {
        return nullptr;
    }
    const auto* method = static_cast<UnityResolve::Method*>(method_handle);
    if (index >= method->args.size() || method->args[index] == nullptr ||
        method->args[index]->pType == nullptr) {
        return nullptr;
    }
    return method->args[index]->pType->name.c_str();
}

extern "C" void* dnh_unity_runtime_invoke(
    void* method_handle,
    void* object,
    void* const* arguments,
    void** exception) {
    if (method_handle == nullptr || !dnh_unity_is_ready()) {
        return nullptr;
    }
    auto* method = static_cast<UnityResolve::Method*>(method_handle);
    void* local_exception = nullptr;
    void** exception_output =
        exception == nullptr ? &local_exception : exception;
    return UnityResolve::Invoke<void*>(
        "il2cpp_runtime_invoke",
        method->address,
        object,
        const_cast<void**>(arguments),
        exception_output);
}

extern "C" void* dnh_unity_object_unbox(void* object) {
    return object == nullptr
        ? nullptr
        : UnityResolve::Invoke<void*>("il2cpp_object_unbox", object);
}

extern "C" void* dnh_unity_object_new(void* class_handle) {
    if (class_handle == nullptr) {
        return nullptr;
    }
    return UnityResolve::Invoke<void*>(
        "il2cpp_object_new",
        static_cast<UnityResolve::Class*>(class_handle)->address);
}

extern "C" void* dnh_unity_string_new_utf8(const char* value) {
    return value == nullptr
        ? nullptr
        : UnityResolve::Invoke<void*>("il2cpp_string_new", value);
}

extern "C" void* dnh_unity_array_new(
    void* element_class,
    const std::size_t length) {
    if (element_class == nullptr) {
        return nullptr;
    }
    return UnityResolve::Invoke<void*>(
        "il2cpp_array_new",
        static_cast<UnityResolve::Class*>(element_class)->address,
        length);
}

extern "C" std::size_t dnh_unity_array_length(void* array) {
    return array == nullptr
        ? 0
        : UnityResolve::Invoke<std::size_t>("il2cpp_array_length", array);
}

extern "C" void* dnh_unity_array_data(void* array) {
    if (array == nullptr) {
        return nullptr;
    }
    struct ArrayHeader {
        void* klass;
        void* monitor;
        void* bounds;
        std::size_t max_length;
    };
    return static_cast<void*>(
        reinterpret_cast<std::byte*>(array) + sizeof(ArrayHeader));
}

extern "C" std::int32_t dnh_unity_class_value_size(
    void* class_handle,
    std::uint32_t* alignment) {
    if (class_handle == nullptr) {
        return -1;
    }
    return UnityResolve::Invoke<std::int32_t>(
        "il2cpp_class_value_size",
        static_cast<UnityResolve::Class*>(class_handle)->address,
        alignment);
}

extern "C" bool dnh_unity_class_is_value_type(void* class_handle) {
    return class_handle != nullptr &&
           UnityResolve::Invoke<bool>(
               "il2cpp_class_is_valuetype",
               static_cast<UnityResolve::Class*>(class_handle)->address);
}

extern "C" bool dnh_unity_field_get_value(
    void* object,
    void* field_handle,
    void* output) {
    if (object == nullptr || field_handle == nullptr || output == nullptr) {
        return false;
    }
    UnityResolve::Invoke<void>(
        "il2cpp_field_get_value",
        object,
        static_cast<UnityResolve::Field*>(field_handle)->address,
        output);
    return true;
}

extern "C" bool dnh_unity_field_set_value(
    void* object,
    void* field_handle,
    const void* value) {
    if (object == nullptr || field_handle == nullptr || value == nullptr) {
        return false;
    }
    UnityResolve::Invoke<void>(
        "il2cpp_field_set_value",
        object,
        static_cast<UnityResolve::Field*>(field_handle)->address,
        const_cast<void*>(value));
    return true;
}

extern "C" void* dnh_unity_field_get_value_object(
    void* object,
    void* field_handle) {
    if (field_handle == nullptr) {
        return nullptr;
    }
    return UnityResolve::Invoke<void*>(
        "il2cpp_field_get_value_object",
        static_cast<UnityResolve::Field*>(field_handle)->address,
        object);
}

extern "C" bool dnh_unity_field_get_static_value(
    void* field_handle,
    void* output) {
    if (field_handle == nullptr || output == nullptr) {
        return false;
    }
    UnityResolve::Invoke<void>(
        "il2cpp_field_static_get_value",
        static_cast<UnityResolve::Field*>(field_handle)->address,
        output);
    return true;
}

extern "C" bool dnh_unity_field_set_static_value(
    void* field_handle,
    const void* value) {
    if (field_handle == nullptr || value == nullptr) {
        return false;
    }
    UnityResolve::Invoke<void>(
        "il2cpp_field_static_set_value",
        static_cast<UnityResolve::Field*>(field_handle)->address,
        const_cast<void*>(value));
    return true;
}

extern "C" std::uint32_t dnh_unity_gc_handle_new(
    void* object,
    const bool pinned) {
    const std::uintptr_t native_handle = NewNativeGcHandle(object, pinned);
    if (native_handle == 0) {
        return 0;
    }
    for (;;) {
        const std::uint32_t token =
            g_next_legacy_gc_handle.fetch_add(1, std::memory_order_relaxed);
        if (token == 0) {
            continue;
        }
        const std::lock_guard lock(g_legacy_gc_handles_mutex);
        if (g_legacy_gc_handles.emplace(token, native_handle).second) {
            return token;
        }
    }
}

extern "C" void* dnh_unity_gc_handle_target(const std::uint32_t handle) {
    std::uintptr_t native_handle{};
    {
        const std::lock_guard lock(g_legacy_gc_handles_mutex);
        if (const auto found = g_legacy_gc_handles.find(handle);
            found != g_legacy_gc_handles.end()) {
            native_handle = found->second;
        }
    }
    return NativeGcHandleTarget(native_handle);
}

extern "C" void dnh_unity_gc_handle_free(const std::uint32_t handle) {
    std::uintptr_t native_handle{};
    {
        const std::lock_guard lock(g_legacy_gc_handles_mutex);
        if (const auto found = g_legacy_gc_handles.find(handle);
            found != g_legacy_gc_handles.end()) {
            native_handle = found->second;
            g_legacy_gc_handles.erase(found);
        }
    }
    FreeNativeGcHandle(native_handle);
}

extern "C" std::uintptr_t dnh_unity_gc_handle_new_v4(
    void* object,
    const bool pinned) {
    return NewNativeGcHandle(object, pinned);
}

extern "C" void* dnh_unity_gc_handle_target_v4(
    const std::uintptr_t handle) {
    return NativeGcHandleTarget(handle);
}

extern "C" void dnh_unity_gc_handle_free_v4(
    const std::uintptr_t handle) {
    FreeNativeGcHandle(handle);
}

extern "C" void* dnh_unity_resolve_icall(const char* name) {
    return name == nullptr
        ? nullptr
        : UnityResolve::Invoke<void*>("il2cpp_resolve_icall", name);
}

extern "C" void* dnh_unity_inflate_generic_method(
    void* method_handle,
    void* const* type_arguments,
    const std::size_t type_argument_count) {
    if (method_handle == nullptr || type_arguments == nullptr ||
        type_argument_count == 0 || !dnh_unity_is_ready()) {
        return nullptr;
    }

    auto* method = static_cast<UnityResolve::Method*>(method_handle);
    void* reflection_method = UnityResolve::Invoke<void*>(
        "il2cpp_method_get_object",
        method->address,
        method->klass == nullptr ? nullptr : method->klass->address);
    if (reflection_method == nullptr) {
        return nullptr;
    }

    void* type_class = dnh_unity_get_class("mscorlib.dll", "System", "Type");
    void* method_info_class =
        dnh_unity_get_class("mscorlib.dll", "System.Reflection", "MethodInfo");
    void* make_generic_method =
        dnh_unity_get_method(method_info_class, "MakeGenericMethod", 1);
    void* type_array =
        dnh_unity_array_new(type_class, type_argument_count);
    auto** type_array_data =
        static_cast<void**>(dnh_unity_array_data(type_array));
    if (type_class == nullptr || method_info_class == nullptr ||
        make_generic_method == nullptr || type_array == nullptr ||
        type_array_data == nullptr) {
        return nullptr;
    }

    for (std::size_t index = 0; index < type_argument_count; ++index) {
        if (type_arguments[index] == nullptr) {
            return nullptr;
        }
        type_array_data[index] =
            dnh_unity_class_type_object(type_arguments[index]);
        if (type_array_data[index] == nullptr) {
            return nullptr;
        }
    }

    void* arguments[]{type_array};
    void* exception = nullptr;
    void* concrete_make_generic_method = UnityResolve::Invoke<void*>(
        "il2cpp_object_get_virtual_method",
        reflection_method,
        static_cast<UnityResolve::Method*>(make_generic_method)->address);
    if (concrete_make_generic_method == nullptr) {
        return nullptr;
    }
    void* inflated_reflection = UnityResolve::Invoke<void*>(
        "il2cpp_runtime_invoke",
        concrete_make_generic_method,
        reflection_method,
        static_cast<void*>(arguments),
        &exception);
    if (inflated_reflection == nullptr || exception != nullptr) {
        return nullptr;
    }

    void* inflated = UnityResolve::Invoke<void*>(
        "il2cpp_method_get_from_reflection",
        inflated_reflection);
    if (inflated == nullptr) {
        return nullptr;
    }

    auto* resolved = new UnityResolve::Method{};
    resolved->address = inflated;
    resolved->klass = method->klass;
    resolved->name = method->name;
    resolved->return_type = method->return_type;
    resolved->flags = method->flags;
    resolved->static_function = method->static_function;
    resolved->function = *static_cast<void**>(inflated);
    g_dynamic_methods.push_back(resolved);
    return resolved;
}

extern "C" bool dnh_unity_class_is_assignable_from(
    void* base_class,
    void* candidate_class) {
    if (base_class == nullptr || candidate_class == nullptr ||
        !dnh_unity_is_ready()) {
        return false;
    }
    return UnityResolve::Invoke<bool>(
        "il2cpp_class_is_assignable_from",
        static_cast<UnityResolve::Class*>(base_class)->address,
        static_cast<UnityResolve::Class*>(candidate_class)->address);
}

extern "C" std::size_t dnh_unity_copy_string_utf8(
    void* string_object,
    std::uint8_t* output,
    const std::size_t capacity) {
    if (string_object == nullptr || !dnh_unity_is_ready()) {
        return 0;
    }
    const auto length = UnityResolve::Invoke<std::int32_t>(
        "il2cpp_string_length",
        string_object);
    const auto* characters = UnityResolve::Invoke<const char16_t*>(
        "il2cpp_string_chars",
        string_object);
    if (length <= 0 || characters == nullptr) {
        return 0;
    }

    std::string utf8;
    utf8.reserve(static_cast<std::size_t>(length));
    for (std::int32_t index = 0; index < length; ++index) {
        std::uint32_t code_point = characters[index];
        if (code_point >= 0xD800u && code_point <= 0xDBFFu &&
            index + 1 < length) {
            const std::uint32_t low = characters[index + 1];
            if (low >= 0xDC00u && low <= 0xDFFFu) {
                code_point =
                    0x10000u + ((code_point - 0xD800u) << 10u) +
                    (low - 0xDC00u);
                ++index;
            }
        }
        if (code_point >= 0xD800u && code_point <= 0xDFFFu) {
            code_point = 0xFFFDu;
        }
        if (code_point <= 0x7Fu) {
            utf8.push_back(static_cast<char>(code_point));
        } else if (code_point <= 0x7FFu) {
            utf8.push_back(static_cast<char>(0xC0u | (code_point >> 6u)));
            utf8.push_back(static_cast<char>(0x80u | (code_point & 0x3Fu)));
        } else if (code_point <= 0xFFFFu) {
            utf8.push_back(static_cast<char>(0xE0u | (code_point >> 12u)));
            utf8.push_back(
                static_cast<char>(0x80u | ((code_point >> 6u) & 0x3Fu)));
            utf8.push_back(static_cast<char>(0x80u | (code_point & 0x3Fu)));
        } else {
            utf8.push_back(static_cast<char>(0xF0u | (code_point >> 18u)));
            utf8.push_back(
                static_cast<char>(0x80u | ((code_point >> 12u) & 0x3Fu)));
            utf8.push_back(
                static_cast<char>(0x80u | ((code_point >> 6u) & 0x3Fu)));
            utf8.push_back(static_cast<char>(0x80u | (code_point & 0x3Fu)));
        }
    }
    if (output != nullptr && capacity != 0) {
        const std::size_t count = std::min(capacity, utf8.size());
        std::memcpy(output, utf8.data(), count);
    }
    return utf8.size();
}

extern "C" void* dnh_unity_runtime_invoke_virtual(
    void* method_handle,
    void* object,
    void* const* arguments,
    void** exception) {
    if (method_handle == nullptr || object == nullptr ||
        !dnh_unity_is_ready()) {
        return nullptr;
    }
    const auto* method = static_cast<UnityResolve::Method*>(method_handle);
    void* virtual_method = UnityResolve::Invoke<void*>(
        "il2cpp_object_get_virtual_method",
        object,
        method->address);
    if (virtual_method == nullptr) {
        return nullptr;
    }
    void* local_exception = nullptr;
    void** exception_output =
        exception == nullptr ? &local_exception : exception;
    return UnityResolve::Invoke<void*>(
        "il2cpp_runtime_invoke",
        virtual_method,
        object,
        const_cast<void**>(arguments),
        exception_output);
}

extern "C" std::size_t dnh_unity_find_fields_by_signature(
    void* class_handle,
    const DnhFieldSignatureV3* signature,
    void** output,
    const std::size_t capacity) {
    if (class_handle == nullptr || signature == nullptr) {
        return 0;
    }
    const auto* klass = static_cast<UnityResolve::Class*>(class_handle);
    std::size_t total{};
    for (auto* field : klass->fields) {
        if (!FieldMatches(field, signature)) {
            continue;
        }
        if (output != nullptr && total < capacity) {
            output[total] = field;
        }
        ++total;
    }
    return total;
}

extern "C" std::size_t dnh_unity_find_methods_by_signature(
    void* class_handle,
    const DnhMethodSignatureV3* signature,
    void** output,
    const std::size_t capacity) {
    if (class_handle == nullptr || signature == nullptr) {
        return 0;
    }
    const auto* klass = static_cast<UnityResolve::Class*>(class_handle);
    std::size_t total{};
    for (auto* method : klass->methods) {
        if (!MethodMatches(method, signature)) {
            continue;
        }
        if (output != nullptr && total < capacity) {
            output[total] = method;
        }
        ++total;
    }
    return total;
}

extern "C" std::size_t dnh_unity_find_classes_by_signature(
    const char* assembly,
    const DnhClassSignatureV3* signature,
    void** output,
    const std::size_t capacity) {
    if (signature == nullptr) {
        return 0;
    }
    std::size_t total{};
    for (auto* candidate_assembly : UnityResolve::assembly) {
        if (candidate_assembly == nullptr ||
            (assembly != nullptr && assembly[0] != '\0' &&
             candidate_assembly->name != assembly)) {
            continue;
        }
        for (auto* klass : candidate_assembly->classes) {
            if (!ClassMatches(klass, signature)) {
                continue;
            }
            if (output != nullptr && total < capacity) {
                output[total] = klass;
            }
            ++total;
        }
    }
    return total;
}
