use dofus_native_mod_api::{
    DNH_ABI_VERSION_4, DNH_MEMBER_STORAGE_INSTANCE, DnhFieldInfoV2, DnhFieldSignatureV3,
    DnhGcHandleV4, DnhHandle, DnhHostApiV1, DnhHostApiV4, DnhLogLevel, DnhMethodSignatureV3,
    DnhUnityApiV2, DnhUnityApiV3, DnhUnityApiV4,
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
    /// `host_api` must be a process-lifetime ABI v4 table supplied by the
    /// native host.
    pub unsafe fn bind(host_api: *const DnhHostApiV1) -> Option<Self> {
        let host = NonNull::new(host_api.cast_mut().cast::<DnhHostApiV4>())?;
        let host_ref = unsafe { host.as_ref() };
        if host_ref.abi_version != DNH_ABI_VERSION_4
            || host_ref.struct_size < size_of::<DnhHostApiV4>() as u32
        {
            return None;
        }
        let unity = NonNull::new(host_ref.unity.cast_mut())?;
        let unity_ref = unsafe { unity.as_ref() };
        if unity_ref.v3.v2.abi_version != DNH_ABI_VERSION_4
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

    pub fn unbox<T: Copy>(self, boxed: DnhHandle) -> Option<T> {
        if boxed.is_null() {
            return None;
        }
        let value = unsafe { (self.v2().object_unbox)(boxed) };
        if value.is_null() {
            return None;
        }
        Some(unsafe { value.cast::<T>().read_unaligned() })
    }

    pub fn string(self, value: &str) -> DnhHandle {
        let Ok(value) = std::ffi::CString::new(value) else {
            return null_mut();
        };
        unsafe { (self.v2().string_new_utf8)(value.as_ptr()) }
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
