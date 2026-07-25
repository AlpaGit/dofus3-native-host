use dofus_native_mod_api::{
    DNH_ABI_VERSION, DNH_ERROR, DNH_OK, DnhHostApiV1, DnhLogLevel, DnhModInfoV1,
};
use std::ffi::c_char;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

static HOST: AtomicPtr<DnhHostApiV1> = AtomicPtr::new(std::ptr::null_mut());
static TICK_REPORTED: AtomicBool = AtomicBool::new(false);

static MOD_ID: &[u8] = b"org.nexytrus.native-example\0";
static MOD_NAME: &[u8] = b"Native Example\0";
static MOD_VERSION: &[u8] = b"0.1.0\0";
static MOD_AUTHOR: &[u8] = b"Nexytrus contributors\0";

struct SyncModInfo(DnhModInfoV1);
unsafe impl Sync for SyncModInfo {}

static MOD_INFO: SyncModInfo = SyncModInfo(DnhModInfoV1 {
    abi_version: DNH_ABI_VERSION,
    struct_size: size_of::<DnhModInfoV1>() as u32,
    id: MOD_ID.as_ptr().cast::<c_char>(),
    name: MOD_NAME.as_ptr().cast::<c_char>(),
    version: MOD_VERSION.as_ptr().cast::<c_char>(),
    author: MOD_AUTHOR.as_ptr().cast::<c_char>(),
});

fn log(level: DnhLogLevel, message: &str) {
    let host = HOST.load(Ordering::Acquire);
    if let Some(host) = unsafe { host.as_ref() } {
        unsafe { (host.log)(level, message.as_ptr(), message.len()) };
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn DNM_Query(host_abi_version: u32) -> *const DnhModInfoV1 {
    if host_abi_version != DNH_ABI_VERSION {
        return std::ptr::null();
    }
    &MOD_INFO.0
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `host_api` must point to a process-lifetime `DnhHostApiV1` supplied by a
/// compatible Dofus Native Host instance.
pub unsafe extern "system" fn DNM_Load(host_api: *const DnhHostApiV1) -> i32 {
    let Some(host) = (unsafe { host_api.as_ref() }) else {
        return DNH_ERROR;
    };
    if host.abi_version != DNH_ABI_VERSION || host.struct_size < size_of::<DnhHostApiV1>() as u32 {
        return DNH_ERROR;
    }
    if HOST
        .compare_exchange(
            std::ptr::null_mut(),
            host_api.cast_mut(),
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        return DNH_ERROR;
    }

    let unity_ready = unsafe { ((*host.unity).is_ready)() };
    log(
        DnhLogLevel::Info,
        if unity_ready {
            "Native Example loaded; UnityResolve is ready."
        } else {
            "Native Example loaded; UnityResolve is not ready."
        },
    );
    DNH_OK
}

#[unsafe(no_mangle)]
pub extern "system" fn DNM_Tick() {
    if !TICK_REPORTED.swap(true, Ordering::AcqRel) {
        log(
            DnhLogLevel::Info,
            "Native Example received its first Unity-thread tick.",
        );
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn DNM_Unload() {
    log(DnhLogLevel::Info, "Native Example unloaded.");
    TICK_REPORTED.store(false, Ordering::Release);
}
