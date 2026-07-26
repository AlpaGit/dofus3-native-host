use dofus_native_mod_api::{
    DNH_ABI_VERSION_4, DNH_ABI_VERSION_5, DNH_ABI_VERSION_6, DNH_ABI_VERSION_7, DNH_ABI_VERSION_8,
    DNH_MEMBER_STORAGE_INSTANCE, DNH_OK, DnhClassInfoV2, DnhFieldInfoV2, DnhFieldSignatureV3,
    DnhGcHandleV4, DnhHandle, DnhHookApiV5, DnhHostApiV1, DnhHostApiV4, DnhHostApiV5, DnhLogLevel,
    DnhMethodInfoV2, DnhMethodSignatureV3, DnhUnityApiV2, DnhUnityApiV3, DnhUnityApiV4,
    DnhUnityApiV6, DnhUnityApiV7, DnhUnityApiV8,
};
use std::ffi::{CStr, c_void};
use std::mem::{MaybeUninit, size_of};
use std::ptr::{NonNull, null, null_mut};

#[derive(Clone, Copy)]
pub struct Runtime {
    host: NonNull<DnhHostApiV4>,
    unity: NonNull<DnhUnityApiV4>,
}

// The host tables live for the loaded mod's lifetime. Mods still have to call
// Unity methods from the Unity thread, as required by the ABI.
unsafe impl Send for Runtime {}
unsafe impl Sync for Runtime {}

// `DnhHandle` values are opaque IL2CPP handles obtained from this same runtime.
// Passing them back to the host is the safe SDK operation; their validity
// cannot be proven from Rust's raw-pointer type alone.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
impl Runtime {
    /// # Safety
    ///
    /// `host_api` must be a process-lifetime ABI v4 through v8 table supplied by the
    /// native host.
    pub unsafe fn bind(host_api: *const DnhHostApiV1) -> Option<Self> {
        let host = NonNull::new(host_api.cast_mut().cast::<DnhHostApiV4>())?;
        let host_ref = unsafe { host.as_ref() };
        if !matches!(
            host_ref.abi_version,
            DNH_ABI_VERSION_4
                | DNH_ABI_VERSION_5
                | DNH_ABI_VERSION_6
                | DNH_ABI_VERSION_7
                | DNH_ABI_VERSION_8
        ) || host_ref.struct_size < size_of::<DnhHostApiV4>() as u32
        {
            return None;
        }
        let unity = NonNull::new(host_ref.unity.cast_mut())?;
        let unity_ref = unsafe { unity.as_ref() };
        if unity_ref.v3.v2.abi_version != host_ref.abi_version
            || unity_ref.v3.v2.struct_size < size_of::<DnhUnityApiV2>() as u32
        {
            return None;
        }
        Some(Self { host, unity })
    }

    pub fn v2(self) -> &'static DnhUnityApiV2 {
        unsafe { &self.unity.as_ref().v3.v2 }
    }

    pub fn v3(self) -> &'static DnhUnityApiV3 {
        unsafe { &self.unity.as_ref().v3 }
    }

    pub fn v4(self) -> &'static DnhUnityApiV4 {
        unsafe { self.unity.as_ref() }
    }

    pub fn v6(self) -> Option<&'static DnhUnityApiV6> {
        let host = unsafe { self.host.as_ref() };
        matches!(
            host.abi_version,
            DNH_ABI_VERSION_6 | DNH_ABI_VERSION_7 | DNH_ABI_VERSION_8
        )
        .then(|| unsafe { &*self.unity.as_ptr().cast::<DnhUnityApiV6>() })
    }

    pub fn v7(self) -> Option<&'static DnhUnityApiV7> {
        let host = unsafe { self.host.as_ref() };
        matches!(host.abi_version, DNH_ABI_VERSION_7 | DNH_ABI_VERSION_8)
            .then(|| unsafe { &*self.unity.as_ptr().cast::<DnhUnityApiV7>() })
    }

    pub fn v8(self) -> Option<&'static DnhUnityApiV8> {
        let host = unsafe { self.host.as_ref() };
        (host.abi_version == DNH_ABI_VERSION_8)
            .then(|| unsafe { &*self.unity.as_ptr().cast::<DnhUnityApiV8>() })
    }

    pub fn v5_hooks(self) -> Option<&'static DnhHookApiV5> {
        let host = unsafe { self.host.as_ref() };
        if !matches!(
            host.abi_version,
            DNH_ABI_VERSION_5 | DNH_ABI_VERSION_6 | DNH_ABI_VERSION_7 | DNH_ABI_VERSION_8
        ) || host.struct_size < size_of::<DnhHostApiV5>() as u32
        {
            return None;
        }
        let host_v5 = unsafe { &*self.host.as_ptr().cast::<DnhHostApiV5>() };
        unsafe { host_v5.hooks.as_ref() }
            .filter(|hooks| hooks.struct_size >= size_of::<DnhHookApiV5>() as u32)
    }

    pub fn create_hook(self, target: DnhHandle, detour: DnhHandle) -> Option<DnhHandle> {
        let hooks = self.v5_hooks()?;
        let mut original = null_mut();
        (unsafe { (hooks.create)(target, detour, &mut original) } == DNH_OK).then_some(original)
    }

    pub fn enable_hook(self, target: DnhHandle) -> bool {
        self.v5_hooks()
            .is_some_and(|hooks| unsafe { (hooks.enable)(target) } == DNH_OK)
    }

    pub fn disable_hook(self, target: DnhHandle) -> bool {
        self.v5_hooks()
            .is_some_and(|hooks| unsafe { (hooks.disable)(target) } == DNH_OK)
    }

    pub fn remove_hook(self, target: DnhHandle) -> bool {
        self.v5_hooks()
            .is_some_and(|hooks| unsafe { (hooks.remove)(target) } == DNH_OK)
    }

    pub fn inflate_generic_method(
        self,
        method: DnhHandle,
        type_arguments: &[DnhHandle],
    ) -> Option<DnhHandle> {
        if method.is_null() || type_arguments.is_empty() {
            return None;
        }
        let api = self.v6()?;
        let inflated = unsafe {
            (api.inflate_generic_method)(method, type_arguments.as_ptr(), type_arguments.len())
        };
        (!inflated.is_null()).then_some(inflated)
    }

    pub fn log(self, level: DnhLogLevel, message: &str) {
        let host = unsafe { self.host.as_ref() };
        unsafe { (host.log)(level, message.as_ptr(), message.len()) };
    }

    pub fn class(self, assembly: &CStr, namespace_name: &CStr, class_name: &CStr) -> DnhHandle {
        unsafe {
            (self.v2().get_class)(
                assembly.as_ptr(),
                namespace_name.as_ptr(),
                class_name.as_ptr(),
            )
        }
    }

    pub fn fields_by_type(self, class: DnhHandle, type_name: &CStr) -> Vec<DnhHandle> {
        let signature = DnhFieldSignatureV3 {
            struct_size: size_of::<DnhFieldSignatureV3>() as u32,
            name: null(),
            type_name: type_name.as_ptr(),
            minimum_offset: 1,
            maximum_offset: 0,
            storage: DNH_MEMBER_STORAGE_INSTANCE,
            reserved: [0; 3],
        };
        let count =
            unsafe { (self.v3().find_fields_by_signature)(class, &signature, null_mut(), 0) };
        let mut fields = vec![null_mut(); count];
        if count != 0 {
            unsafe {
                (self.v3().find_fields_by_signature)(
                    class,
                    &signature,
                    fields.as_mut_ptr(),
                    fields.len(),
                );
            }
        }
        fields
    }

    pub fn methods_by_signature(
        self,
        class: DnhHandle,
        name: Option<&CStr>,
        return_type: Option<&CStr>,
        parameter_types: &[&CStr],
        storage: u8,
    ) -> Vec<DnhHandle> {
        if class.is_null() {
            return Vec::new();
        }
        let parameter_type_names = parameter_types
            .iter()
            .map(|value| value.as_ptr())
            .collect::<Vec<_>>();
        let signature = DnhMethodSignatureV3 {
            struct_size: size_of::<DnhMethodSignatureV3>() as u32,
            name: name.map_or(null(), CStr::as_ptr),
            return_type_name: return_type.map_or(null(), CStr::as_ptr),
            parameter_count: parameter_types.len() as i32,
            parameter_type_names: if parameter_type_names.is_empty() {
                null()
            } else {
                parameter_type_names.as_ptr()
            },
            parameter_type_count: parameter_type_names.len() as u32,
            storage,
            reserved: [0; 3],
        };
        let count =
            unsafe { (self.v3().find_methods_by_signature)(class, &signature, null_mut(), 0) };
        let mut methods = vec![null_mut(); count];
        if count != 0 {
            unsafe {
                (self.v3().find_methods_by_signature)(
                    class,
                    &signature,
                    methods.as_mut_ptr(),
                    methods.len(),
                );
            }
        }
        methods
    }

    pub fn unique_method_by_signature(
        self,
        class: DnhHandle,
        name: Option<&CStr>,
        return_type: Option<&CStr>,
        parameter_types: &[&CStr],
        storage: u8,
        label: &str,
    ) -> Option<DnhHandle> {
        let methods = self.methods_by_signature(class, name, return_type, parameter_types, storage);
        if methods.len() == 1 {
            methods.first().copied()
        } else {
            self.log(
                DnhLogLevel::Error,
                &format!(
                    "{label}: expected one method signature match, found {}.",
                    methods.len()
                ),
            );
            None
        }
    }

    pub fn method(
        self,
        class: DnhHandle,
        name: &CStr,
        parameter_types: &[&CStr],
        storage: u8,
    ) -> Option<DnhHandle> {
        let parameter_type_names = parameter_types
            .iter()
            .map(|value| value.as_ptr())
            .collect::<Vec<_>>();
        let signature = DnhMethodSignatureV3 {
            struct_size: size_of::<DnhMethodSignatureV3>() as u32,
            name: name.as_ptr(),
            return_type_name: null(),
            parameter_count: parameter_types.len() as i32,
            parameter_type_names: if parameter_type_names.is_empty() {
                null()
            } else {
                parameter_type_names.as_ptr()
            },
            parameter_type_count: parameter_type_names.len() as u32,
            storage,
            reserved: [0; 3],
        };
        let mut method = null_mut();
        let count =
            unsafe { (self.v3().find_methods_by_signature)(class, &signature, &mut method, 1) };
        if count == 1 {
            Some(method)
        } else {
            self.log(
                DnhLogLevel::Error,
                &format!(
                    "Method signature {} resolved {count} candidate(s).",
                    name.to_string_lossy()
                ),
            );
            None
        }
    }

    pub fn instance_method(
        self,
        class: DnhHandle,
        name: &CStr,
        parameter_types: &[&CStr],
    ) -> Option<DnhHandle> {
        self.method(class, name, parameter_types, DNH_MEMBER_STORAGE_INSTANCE)
    }

    pub fn invoke(
        self,
        method: DnhHandle,
        object: DnhHandle,
        arguments: &[DnhHandle],
    ) -> Option<DnhHandle> {
        if method.is_null() {
            return None;
        }
        let mut exception = null_mut();
        let result = unsafe {
            (self.v2().runtime_invoke)(
                method,
                object,
                if arguments.is_empty() {
                    null()
                } else {
                    arguments.as_ptr()
                },
                &mut exception,
            )
        };
        if !exception.is_null() {
            self.log(
                DnhLogLevel::Error,
                &format!("IL2CPP invocation raised exception {exception:p}"),
            );
            None
        } else {
            Some(result)
        }
    }

    pub fn invoke_virtual(
        self,
        method: DnhHandle,
        object: DnhHandle,
        arguments: &[DnhHandle],
    ) -> Option<DnhHandle> {
        if method.is_null() || object.is_null() {
            return None;
        }
        let api = self.v7()?;
        let mut exception = null_mut();
        let result = unsafe {
            (api.runtime_invoke_virtual)(
                method,
                object,
                if arguments.is_empty() {
                    null()
                } else {
                    arguments.as_ptr()
                },
                &mut exception,
            )
        };
        if !exception.is_null() {
            self.log(
                DnhLogLevel::Error,
                &format!("IL2CPP virtual invocation raised exception {exception:p}"),
            );
            None
        } else {
            Some(result)
        }
    }

    pub fn unbox<T: Copy>(self, boxed: DnhHandle) -> Option<T> {
        let value = self.unboxed_handle(boxed)?;
        Some(unsafe { value.cast::<T>().read_unaligned() })
    }

    pub fn unboxed_handle(self, boxed: DnhHandle) -> Option<DnhHandle> {
        if boxed.is_null() {
            return None;
        }
        let value = unsafe { (self.v2().object_unbox)(boxed) };
        (!value.is_null()).then_some(value)
    }

    pub fn string(self, value: &str) -> DnhHandle {
        let Ok(value) = std::ffi::CString::new(value) else {
            return null_mut();
        };
        unsafe { (self.v2().string_new_utf8)(value.as_ptr()) }
    }

    pub fn managed_string(self, value: DnhHandle) -> Option<String> {
        if value.is_null() {
            return None;
        }
        if let Some(api) = self.v7() {
            let length = unsafe { (api.copy_string_utf8)(value, null_mut(), 0) };
            if length > 16 * 1024 * 1024 {
                return None;
            }
            let mut bytes = vec![0_u8; length];
            if length != 0 {
                let copied =
                    unsafe { (api.copy_string_utf8)(value, bytes.as_mut_ptr(), bytes.len()) };
                if copied != length {
                    return None;
                }
            }
            return String::from_utf8(bytes).ok();
        }
        // IL2CPP strings are an object header, a signed UTF-16 length, then the
        // inline UTF-16 payload. The host is x64-only, so the header is 16 bytes.
        let length = unsafe { value.cast::<u8>().add(16).cast::<i32>().read_unaligned() };
        if !(0..=1_048_576).contains(&length) {
            return None;
        }
        let characters = unsafe {
            std::slice::from_raw_parts(value.cast::<u8>().add(20).cast::<u16>(), length as usize)
        };
        Some(String::from_utf16_lossy(characters))
    }

    pub fn class_is_assignable_from(self, base: DnhHandle, candidate: DnhHandle) -> bool {
        !base.is_null()
            && !candidate.is_null()
            && self
                .v7()
                .is_some_and(|api| unsafe { (api.class_is_assignable_from)(base, candidate) })
    }

    pub fn class_parent(self, class: DnhHandle) -> DnhHandle {
        if class.is_null() {
            return null_mut();
        }
        self.v8()
            .map(|api| unsafe { (api.class_parent)(class) })
            .unwrap_or(null_mut())
    }

    pub fn object_is(self, object: DnhHandle, base: DnhHandle) -> bool {
        if object.is_null() || base.is_null() {
            return false;
        }
        let candidate = unsafe { (self.v2().get_object_class)(object) };
        self.class_is_assignable_from(base, candidate)
    }

    pub fn class_name(self, class: DnhHandle) -> String {
        if class.is_null() {
            return "<unknown>".to_owned();
        }
        let mut info = DnhClassInfoV2 {
            struct_size: size_of::<DnhClassInfoV2>() as u32,
            name: null(),
            namespace_name: null(),
            parent_name: null(),
            field_count: 0,
            method_count: 0,
            is_value_type: 0,
            reserved: [0; 3],
        };
        if !unsafe { (self.v2().copy_class_info)(class, &mut info) } || info.name.is_null() {
            return "<unknown>".to_owned();
        }
        let name = unsafe { CStr::from_ptr(info.name) }.to_string_lossy();
        if info.namespace_name.is_null() {
            return name.into_owned();
        }
        let namespace = unsafe { CStr::from_ptr(info.namespace_name) }.to_string_lossy();
        if namespace.is_empty() {
            name.into_owned()
        } else {
            format!("{namespace}.{name}")
        }
    }

    pub fn object_class_name(self, object: DnhHandle) -> String {
        if object.is_null() {
            return "<null>".to_owned();
        }
        let class = unsafe { (self.v2().get_object_class)(object) };
        self.class_name(class)
    }

    pub fn find_objects(self, class: DnhHandle) -> Vec<DnhHandle> {
        if class.is_null() {
            return Vec::new();
        }
        let count = unsafe { (self.v2().find_objects)(class, null_mut(), 0) };
        let mut objects = vec![null_mut(); count];
        if count != 0 {
            unsafe { (self.v2().find_objects)(class, objects.as_mut_ptr(), objects.len()) };
        }
        objects
    }

    pub fn array_references(self, array: DnhHandle) -> Vec<DnhHandle> {
        if array.is_null() {
            return Vec::new();
        }
        let length = unsafe { (self.v2().array_length)(array) };
        let data = unsafe { (self.v2().array_data)(array) };
        if length == 0 || data.is_null() {
            return Vec::new();
        }
        unsafe { std::slice::from_raw_parts(data.cast::<DnhHandle>(), length) }.to_vec()
    }

    pub fn read_reference(self, object: DnhHandle, name: &CStr) -> Option<DnhHandle> {
        if object.is_null() {
            return None;
        }
        let class = unsafe { (self.v2().get_object_class)(object) };
        if class.is_null() {
            return None;
        }
        let field = unsafe { (self.v2().get_field)(class, name.as_ptr()) };
        if !field.is_null() {
            let mut value = null_mut();
            return unsafe {
                (self.v2().field_get_value)(object, field, (&mut value as *mut DnhHandle).cast())
            }
            .then_some(value);
        }
        let getter = std::ffi::CString::new(format!("get_{}", name.to_string_lossy())).ok()?;
        let method = self.instance_method(class, &getter, &[])?;
        self.invoke(method, object, &[])
    }

    pub fn read_value<T: Copy>(self, object: DnhHandle, name: &CStr) -> Option<T> {
        if object.is_null() {
            return None;
        }
        let class = unsafe { (self.v2().get_object_class)(object) };
        if class.is_null() {
            return None;
        }
        let field = unsafe { (self.v2().get_field)(class, name.as_ptr()) };
        if !field.is_null() {
            let mut value = MaybeUninit::<T>::uninit();
            if unsafe {
                (self.v2().field_get_value)(object, field, value.as_mut_ptr().cast::<c_void>())
            } {
                return Some(unsafe { value.assume_init() });
            }
            return None;
        }
        let getter = std::ffi::CString::new(format!("get_{}", name.to_string_lossy())).ok()?;
        let method = self.instance_method(class, &getter, &[])?;
        self.unbox(self.invoke(method, object, &[])?)
    }

    pub fn field_name(self, field: DnhHandle) -> String {
        let mut info = DnhFieldInfoV2 {
            struct_size: size_of::<DnhFieldInfoV2>() as u32,
            name: null(),
            type_name: null(),
            offset: 0,
            is_static: 0,
            reserved: [0; 3],
        };
        if !unsafe { (self.v2().copy_field_info)(field, &mut info) } || info.name.is_null() {
            return "<unknown>".to_owned();
        }
        unsafe { CStr::from_ptr(info.name) }
            .to_string_lossy()
            .into_owned()
    }

    pub fn method_name(self, method: DnhHandle) -> String {
        let mut info = DnhMethodInfoV2 {
            struct_size: size_of::<DnhMethodInfoV2>() as u32,
            name: null(),
            return_type_name: null(),
            parameter_count: 0,
            flags: 0,
            is_static: 0,
            reserved: [0; 3],
        };
        if !unsafe { (self.v2().copy_method_info)(method, &mut info) } || info.name.is_null() {
            return "<unknown>".to_owned();
        }
        unsafe { CStr::from_ptr(info.name) }
            .to_string_lossy()
            .into_owned()
    }

    pub fn array_from_slice<T: Copy>(self, element_class: DnhHandle, values: &[T]) -> DnhHandle {
        let array = unsafe { (self.v2().array_new)(element_class, values.len()) };
        if array.is_null() || values.is_empty() {
            return array;
        }
        let destination = unsafe { (self.v2().array_data)(array) };
        if destination.is_null() {
            return null_mut();
        }
        unsafe {
            std::ptr::copy_nonoverlapping(values.as_ptr(), destination.cast::<T>(), values.len());
        }
        array
    }

    pub fn gc_new(self, object: DnhHandle, pinned: bool) -> DnhGcHandleV4 {
        unsafe { (self.v4().gc_handle_new_v4)(object, pinned) }
    }

    pub fn gc_target(self, handle: DnhGcHandleV4) -> DnhHandle {
        unsafe { (self.v4().gc_handle_target_v4)(handle) }
    }

    pub fn gc_free(self, handle: DnhGcHandleV4) {
        if handle != 0 {
            unsafe { (self.v4().gc_handle_free_v4)(handle) };
        }
    }
}
