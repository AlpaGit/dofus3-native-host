use dofus_native_mod_api::{
    DNH_ABI_VERSION_8, DNH_ERROR, DNH_MEMBER_STORAGE_INSTANCE, DNH_OK, DnhHandle, DnhHostApiV1,
    DnhLogLevel, DnhModInfoV1,
};
use dofus_native_mod_sdk::Runtime;
use std::collections::HashMap;
use std::ffi::{c_char, c_void};
use std::mem::size_of;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Mutex, OnceLock};

const FAST_FORWARD_SECONDS: f32 = 3600.0;

type AnimationRunFn = unsafe extern "system" fn(DnhHandle, f32, DnhHandle);
type StartAnimationFn = unsafe extern "system" fn(DnhHandle, DnhHandle, i32, f32, u8, DnhHandle);
type ServiceEnabledSetterFn = unsafe extern "system" fn(DnhHandle, u8, DnhHandle);

#[derive(Clone, Copy)]
struct Capture {
    runtime: Runtime,
    fight_entities_class: DnhHandle,
}

unsafe impl Send for Capture {}

static CAPTURE: OnceLock<Mutex<Option<Capture>>> = OnceLock::new();
static ANIMATION_LOOPS: OnceLock<Mutex<HashMap<usize, bool>>> = OnceLock::new();
static IN_COMBAT: AtomicBool = AtomicBool::new(false);

static RUN_TARGET: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static RUN_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static START_TARGET: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static START_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static SERVICE_TARGET: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static SERVICE_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

fn capture() -> &'static Mutex<Option<Capture>> {
    CAPTURE.get_or_init(|| Mutex::new(None))
}

fn animation_loops() -> &'static Mutex<HashMap<usize, bool>> {
    ANIMATION_LOOPS.get_or_init(|| Mutex::new(HashMap::new()))
}

unsafe extern "system" fn animation_run_detour(
    this: DnhHandle,
    current_time: f32,
    method_info: DnhHandle,
) {
    let original = RUN_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    let original: AnimationRunFn = unsafe { std::mem::transmute(original) };
    let should_finish = IN_COMBAT.load(Ordering::Acquire)
        && !this.is_null()
        && animation_loops()
            .lock()
            .ok()
            .and_then(|animations| animations.get(&(this as usize)).copied())
            .is_some_and(|loops| !loops);
    let effective_time = if should_finish && current_time.is_finite() {
        current_time + FAST_FORWARD_SECONDS
    } else {
        current_time
    };
    unsafe { original(this, effective_time, method_info) };
}

unsafe extern "system" fn start_animation_detour(
    this: DnhHandle,
    animation: DnhHandle,
    frame: i32,
    frame_time: f32,
    loops: u8,
    method_info: DnhHandle,
) {
    let original = START_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    let original: StartAnimationFn = unsafe { std::mem::transmute(original) };
    unsafe { original(this, animation, frame, frame_time, loops, method_info) };
    if !this.is_null()
        && let Ok(mut animations) = animation_loops().lock()
    {
        animations.insert(this as usize, loops != 0);
    }
}

unsafe extern "system" fn service_enabled_detour(
    this: DnhHandle,
    enabled: u8,
    method_info: DnhHandle,
) {
    let original = SERVICE_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    let original: ServiceEnabledSetterFn = unsafe { std::mem::transmute(original) };
    unsafe { original(this, enabled, method_info) };

    let state = capture().lock().ok().and_then(|state| *state);
    if let Some(state) = state
        && state.runtime.object_is(this, state.fight_entities_class)
    {
        let active = enabled != 0;
        let previous = IN_COMBAT.swap(active, Ordering::AcqRel);
        if previous != active {
            state.runtime.log(
                DnhLogLevel::Info,
                if active {
                    "Combat Animation Skipper: combat detected, action animations now finish instantly."
                } else {
                    "Combat Animation Skipper: combat ended, normal animation timing restored."
                },
            );
        }
    }
}

fn method_address(runtime: Runtime, method: DnhHandle, label: &str) -> Option<DnhHandle> {
    let address = unsafe { (runtime.v2().method_address)(method) };
    if address.is_null() {
        runtime.log(
            DnhLogLevel::Error,
            &format!("Combat Animation Skipper: {label} has no native address."),
        );
        None
    } else {
        Some(address)
    }
}

fn find_service_state_setter(runtime: Runtime, fight_class: DnhHandle) -> Option<DnhHandle> {
    let mut class = runtime.class_parent(fight_class);
    for _ in 0..16 {
        if class.is_null() {
            break;
        }
        let boolean_fields = runtime.fields_by_type(class, c"System.Boolean");
        let getters = runtime.methods_by_signature(
            class,
            None,
            Some(c"System.Boolean"),
            &[],
            DNH_MEMBER_STORAGE_INSTANCE,
        );
        let setters = runtime
            .methods_by_signature(
                class,
                None,
                Some(c"System.Void"),
                &[c"System.Boolean"],
                DNH_MEMBER_STORAGE_INSTANCE,
            )
            .into_iter()
            .filter(|method| runtime.method_name(*method) != ".ctor")
            .collect::<Vec<_>>();
        if boolean_fields.len() == 1 && getters.len() == 1 && setters.len() == 1 {
            runtime.log(
                DnhLogLevel::Info,
                &format!(
                    "Combat Animation Skipper: fight lifecycle resolved structurally in {}.",
                    runtime.class_name(class)
                ),
            );
            return setters.first().copied();
        }
        class = runtime.class_parent(class);
    }
    runtime.log(
        DnhLogLevel::Error,
        "Combat Animation Skipper: fight lifecycle base class could not be resolved by signature.",
    );
    None
}

fn create_and_enable(
    runtime: Runtime,
    target: DnhHandle,
    detour: DnhHandle,
    target_slot: &AtomicPtr<c_void>,
    original_slot: &AtomicPtr<c_void>,
    label: &str,
) -> bool {
    let Some(original) = runtime.create_hook(target, detour) else {
        runtime.log(
            DnhLogLevel::Error,
            &format!("Combat Animation Skipper: failed to create {label} hook."),
        );
        return false;
    };
    original_slot.store(original, Ordering::Release);
    target_slot.store(target, Ordering::Release);
    if runtime.enable_hook(target) {
        true
    } else {
        runtime.remove_hook(target);
        original_slot.store(null_mut(), Ordering::Release);
        target_slot.store(null_mut(), Ordering::Release);
        runtime.log(
            DnhLogLevel::Error,
            &format!("Combat Animation Skipper: failed to enable {label} hook."),
        );
        false
    }
}

fn remove_hook(
    runtime: Runtime,
    target_slot: &AtomicPtr<c_void>,
    original_slot: &AtomicPtr<c_void>,
) {
    let target = target_slot.swap(null_mut(), Ordering::AcqRel);
    if !target.is_null() {
        runtime.disable_hook(target);
        runtime.remove_hook(target);
    }
    original_slot.store(null_mut(), Ordering::Release);
}

fn remove_all_hooks(runtime: Runtime) {
    remove_hook(runtime, &RUN_TARGET, &RUN_ORIGINAL);
    remove_hook(runtime, &START_TARGET, &START_ORIGINAL);
    remove_hook(runtime, &SERVICE_TARGET, &SERVICE_ORIGINAL);
}

fn load(runtime: Runtime) -> Option<()> {
    let animator = runtime.class(
        c"Ankama.Animator2D.dll",
        c"Ankama.Animations",
        c"Animator2D",
    );
    let fight_entities = runtime.class(
        c"Core.dll",
        c"Core.Services.Fight.FightEntitiesService",
        c"FightEntitiesService",
    );
    if animator.is_null() || fight_entities.is_null() {
        runtime.log(
            DnhLogLevel::Error,
            "Combat Animation Skipper: Animator2D or FightEntitiesService is unavailable.",
        );
        return None;
    }

    let run = runtime.unique_method_by_signature(
        animator,
        None,
        Some(c"System.Void"),
        &[c"System.Single"],
        DNH_MEMBER_STORAGE_INSTANCE,
        "Animator2D.Run(float)",
    )?;
    let start = runtime.unique_method_by_signature(
        animator,
        None,
        Some(c"System.Void"),
        &[
            c"Ankama.Animations.Animation",
            c"System.Int32",
            c"System.Single",
            c"System.Boolean",
        ],
        DNH_MEMBER_STORAGE_INSTANCE,
        "Animator2D.StartAnimation(Animation,int,float,bool)",
    )?;
    let service_setter = find_service_state_setter(runtime, fight_entities)?;

    let run_target = method_address(runtime, run, "Animator2D.Run")?;
    let start_target = method_address(runtime, start, "Animator2D.StartAnimation")?;
    let service_target = method_address(runtime, service_setter, "fight lifecycle setter")?;

    if let Ok(mut state) = capture().lock() {
        *state = Some(Capture {
            runtime,
            fight_entities_class: fight_entities,
        });
    } else {
        return None;
    }
    IN_COMBAT.store(false, Ordering::Release);

    if !create_and_enable(
        runtime,
        service_target,
        service_enabled_detour as *const () as DnhHandle,
        &SERVICE_TARGET,
        &SERVICE_ORIGINAL,
        "fight lifecycle",
    ) || !create_and_enable(
        runtime,
        start_target,
        start_animation_detour as *const () as DnhHandle,
        &START_TARGET,
        &START_ORIGINAL,
        "animation start",
    ) || !create_and_enable(
        runtime,
        run_target,
        animation_run_detour as *const () as DnhHandle,
        &RUN_TARGET,
        &RUN_ORIGINAL,
        "animation update",
    ) {
        remove_all_hooks(runtime);
        if let Ok(mut state) = capture().lock() {
            *state = None;
        }
        return None;
    }

    runtime.log(
        DnhLogLevel::Info,
        "Combat Animation Skipper ready: non-looping Animator2D actions will finish instantly only during fights.",
    );
    Some(())
}

static MOD_ID: &[u8] = b"bubble.dofus3.combat-animation-skipper\0";
static MOD_NAME: &[u8] = b"Dofus Combat Animation Skipper\0";
static MOD_VERSION: &[u8] = b"1.0.0\0";
static MOD_AUTHOR: &[u8] = b"Bubble\0";

struct SyncModInfo(DnhModInfoV1);
unsafe impl Sync for SyncModInfo {}

static MOD_INFO: SyncModInfo = SyncModInfo(DnhModInfoV1 {
    abi_version: DNH_ABI_VERSION_8,
    struct_size: size_of::<DnhModInfoV1>() as u32,
    id: MOD_ID.as_ptr().cast::<c_char>(),
    name: MOD_NAME.as_ptr().cast::<c_char>(),
    version: MOD_VERSION.as_ptr().cast::<c_char>(),
    author: MOD_AUTHOR.as_ptr().cast::<c_char>(),
});

#[unsafe(no_mangle)]
pub extern "system" fn DNM_Query(host_abi_version: u32) -> *const DnhModInfoV1 {
    if host_abi_version == DNH_ABI_VERSION_8 {
        &MOD_INFO.0
    } else {
        null()
    }
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `host_api` must be the process-lifetime ABI v8 table supplied by the host.
pub unsafe extern "system" fn DNM_Load(host_api: *const DnhHostApiV1) -> i32 {
    let Some(runtime) = (unsafe { Runtime::bind(host_api) }) else {
        return DNH_ERROR;
    };
    if load(runtime).is_some() {
        DNH_OK
    } else {
        DNH_ERROR
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn DNM_Tick() {}

#[unsafe(no_mangle)]
pub extern "system" fn DNM_Unload() {
    let state = capture().lock().ok().and_then(|mut state| state.take());
    if let Some(state) = state {
        remove_all_hooks(state.runtime);
    }
    IN_COMBAT.store(false, Ordering::Release);
    if let Ok(mut animations) = animation_loops().lock() {
        animations.clear();
    }
}
