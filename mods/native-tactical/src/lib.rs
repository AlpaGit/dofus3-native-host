use dofus_native_mod_api::{
    DNH_ABI_VERSION_8, DNH_ERROR, DNH_MEMBER_STORAGE_INSTANCE, DNH_MEMBER_STORAGE_STATIC, DNH_OK,
    DnhClassSignatureV3, DnhFieldSignatureV3, DnhGcHandleV4, DnhHandle, DnhHostApiV1, DnhLogLevel,
    DnhMethodInfoV2, DnhMethodSignatureV3, DnhModInfoV1,
};
use dofus_native_mod_sdk::Runtime;
use std::ffi::{CStr, c_char};
use std::mem::size_of;
use std::ptr::{null, null_mut};
use std::sync::{Mutex, MutexGuard};

const MAP_CELL_COUNT: usize = 560;
const MAP_WIDTH: i32 = 14;
const CELL_WIDTH: f32 = 86.0;
const HALF_CELL_WIDTH: f32 = 43.0;
const HALF_CELL_HEIGHT: f32 = 21.5;
const ORIGIN_X: f32 = -580.5;
const ORIGIN_Y: f32 = 483.75;
const CELLS_BUILT_PER_TICK: usize = MAP_CELL_COUNT;
const VK_F8: i32 = 0x77;
const METHOD_ATTRIBUTE_SPECIAL_NAME: u32 = 0x0800;

#[repr(C)]
#[derive(Clone, Copy)]
struct Vector3 {
    x: f32,
    y: f32,
    z: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Quaternion {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CellKind {
    Hidden,
    LineOfSightObstacle,
    Walkable,
}

#[derive(Clone, Copy)]
struct SuppressedRenderer {
    gc_handle: DnhGcHandleV4,
    was_enabled: bool,
}

struct Bindings {
    map_renderer_class: DnhHandle,
    game_object_class: DnhHandle,
    renderer_class: DnhHandle,
    sprite_renderer_class: DnhHandle,
    tactical_cell_class: DnhHandle,
    map_id_field: DnhHandle,
    metadata_fields: Vec<DnhHandle>,
    map_root_fields: Vec<DnhHandle>,
    game_object_ctor: DnhHandle,
    game_object_find: DnhHandle,
    game_object_get_transform: DnhHandle,
    game_object_get_component: DnhHandle,
    game_object_get_components_in_children: DnhHandle,
    game_object_set_active: DnhHandle,
    component_get_game_object: DnhHandle,
    transform_find: DnhHandle,
    transform_get_parent: DnhHandle,
    transform_set_parent: DnhHandle,
    transform_get_local_position: DnhHandle,
    transform_set_local_position: DnhHandle,
    transform_get_local_rotation: DnhHandle,
    transform_set_local_rotation: DnhHandle,
    transform_get_local_scale: DnhHandle,
    transform_set_local_scale: DnhHandle,
    object_instantiate: DnhHandle,
    object_destroy: DnhHandle,
    object_set_name: DnhHandle,
    renderer_get_enabled: DnhHandle,
    renderer_set_enabled: DnhHandle,
    sprite_renderer_get_sprite: DnhHandle,
    sprite_renderer_set_sprite: DnhHandle,
    tactical_set_cell_id: DnhHandle,
    tactical_apply_kind: DnhHandle,
    tactical_sprite_fields: Vec<DnhHandle>,
    addressable_load: Option<DnhHandle>,
    addressable_release: Option<DnhHandle>,
}

struct BuildProgress {
    cells: [CellKind; MAP_CELL_COUNT],
    next_cell: usize,
}

struct State {
    runtime: Runtime,
    bindings: Bindings,
    enabled: bool,
    f8_was_down: bool,
    tick_count: u64,
    map_id: i64,
    map_renderer_gc: DnhGcHandleV4,
    overlay_root_gc: DnhGcHandleV4,
    prefab_gc: DnhGcHandleV4,
    prefab_is_addressable: bool,
    load_task_gc: DnhGcHandleV4,
    build: Option<BuildProgress>,
    suppressed: Vec<SuppressedRenderer>,
}

// The host invokes every lifecycle callback on Unity's owning thread.
unsafe impl Send for State {}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn state() -> MutexGuard<'static, Option<State>> {
    STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn argument<T>(value: &T) -> DnhHandle {
    std::ptr::from_ref(value).cast_mut().cast()
}

fn required(handle: Option<DnhHandle>) -> Option<DnhHandle> {
    handle.filter(|value| !value.is_null())
}

fn method_info(runtime: Runtime, method: DnhHandle) -> Option<DnhMethodInfoV2> {
    let mut info = DnhMethodInfoV2 {
        struct_size: size_of::<DnhMethodInfoV2>() as u32,
        name: null(),
        return_type_name: null(),
        parameter_count: 0,
        flags: 0,
        is_static: 0,
        reserved: [0; 3],
    };
    unsafe { (runtime.v2().copy_method_info)(method, &mut info) }.then_some(info)
}

fn text(pointer: *const c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}

fn declared_methods(runtime: Runtime, class: DnhHandle) -> Vec<DnhHandle> {
    let count = unsafe { (runtime.v2().get_class_methods)(class, null_mut(), 0) };
    let mut methods = vec![null_mut(); count];
    if count != 0 {
        unsafe {
            (runtime.v2().get_class_methods)(class, methods.as_mut_ptr(), methods.len());
        }
    }
    methods
}

fn resolve_tactical_class(runtime: Runtime) -> Option<DnhHandle> {
    let exact = runtime.class(
        c"Core.dll",
        c"Core.Rendering.Sprites.Interactive",
        c"TacticalDebugCell",
    );
    if !exact.is_null() {
        return Some(exact);
    }

    let sprite_field = DnhFieldSignatureV3 {
        struct_size: size_of::<DnhFieldSignatureV3>() as u32,
        name: null(),
        type_name: c"UnityEngine.Sprite".as_ptr(),
        minimum_offset: 1,
        maximum_offset: 0,
        storage: DNH_MEMBER_STORAGE_INSTANCE,
        reserved: [0; 3],
    };
    let int_parameter_types = [c"System.Int32".as_ptr()];
    let int_setter = DnhMethodSignatureV3 {
        struct_size: size_of::<DnhMethodSignatureV3>() as u32,
        name: null(),
        return_type_name: c"System.Void".as_ptr(),
        parameter_count: 1,
        parameter_type_names: int_parameter_types.as_ptr(),
        parameter_type_count: 1,
        storage: DNH_MEMBER_STORAGE_INSTANCE,
        reserved: [0; 3],
    };
    let signature = DnhClassSignatureV3 {
        struct_size: size_of::<DnhClassSignatureV3>() as u32,
        name: null(),
        namespace_name: null(),
        parent_name: null(),
        required_fields: &sprite_field,
        required_field_count: 1,
        required_methods: &int_setter,
        required_method_count: 1,
        minimum_field_count: 2,
        minimum_method_count: 2,
    };
    let count = unsafe {
        (runtime.v3().find_classes_by_signature)(c"Core.dll".as_ptr(), &signature, null_mut(), 0)
    };
    let mut classes = vec![null_mut(); count];
    if count != 0 {
        unsafe {
            (runtime.v3().find_classes_by_signature)(
                c"Core.dll".as_ptr(),
                &signature,
                classes.as_mut_ptr(),
                classes.len(),
            );
        }
    }
    runtime.log(
        DnhLogLevel::Info,
        &format!(
            "Native Tactical resolver: {count} Core class candidate(s) expose Sprite fields and a void(int) method."
        ),
    );
    let matches = classes
        .into_iter()
        .filter(|class| {
            let sprite_count = runtime.fields_by_type(*class, c"UnityEngine.Sprite").len();
            let methods_resolved = resolve_tactical_methods(runtime, *class).is_some();
            runtime.log(
                DnhLogLevel::Info,
                &format!(
                    "Native Tactical resolver candidate {}: spriteFields={sprite_count}, tacticalMethods={methods_resolved}.",
                    runtime.class_name(*class)
                ),
            );
            sprite_count >= 2 && methods_resolved
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        runtime.log(
            DnhLogLevel::Error,
            &format!(
                "Native Tactical resolver expected one structural TacticalDebugCell match, found {}.",
                matches.len()
            ),
        );
    }
    matches.first().copied()
}

fn resolve_tactical_methods(runtime: Runtime, class: DnhHandle) -> Option<(DnhHandle, DnhHandle)> {
    let mut id_setter = None;
    let mut apply_kind = None;
    for method in declared_methods(runtime, class) {
        let Some(info) = method_info(runtime, method) else {
            continue;
        };
        if info.is_static != 0
            || info.parameter_count != 1
            || text(info.return_type_name) != "System.Void"
        {
            continue;
        }
        let parameter = unsafe { (runtime.v2().method_parameter_type_name)(method, 0) };
        let parameter = text(parameter);
        if parameter == "System.Int32" {
            id_setter.get_or_insert(method);
        } else if parameter != "System.Single" && info.flags & METHOD_ATTRIBUTE_SPECIAL_NAME == 0 {
            apply_kind.get_or_insert(method);
        }
    }
    Some((id_setter?, apply_kind?))
}

fn resolve_generic_release(
    runtime: Runtime,
    addressable_class: DnhHandle,
    game_object_class: DnhHandle,
) -> Option<DnhHandle> {
    declared_methods(runtime, addressable_class)
        .into_iter()
        .find(|method| {
            let Some(info) = method_info(runtime, *method) else {
                return false;
            };
            if info.is_static == 0
                || info.parameter_count != 1
                || text(info.name) != "Release"
                || text(info.return_type_name) != "System.Void"
            {
                return false;
            }
            let parameter = unsafe { (runtime.v2().method_parameter_type_name)(*method, 0) };
            matches!(text(parameter).as_str(), "T" | "T0" | "!!0")
        })
        .and_then(|method| runtime.inflate_generic_method(method, &[game_object_class]))
}

fn resolve_bindings(runtime: Runtime) -> Option<Bindings> {
    let map_renderer_class = runtime.class(c"Core.dll", c"Core.Rendering", c"MapRenderer");
    let game_object_class =
        runtime.class(c"UnityEngine.CoreModule.dll", c"UnityEngine", c"GameObject");
    let transform_class =
        runtime.class(c"UnityEngine.CoreModule.dll", c"UnityEngine", c"Transform");
    let component_class =
        runtime.class(c"UnityEngine.CoreModule.dll", c"UnityEngine", c"Component");
    let object_class = runtime.class(c"UnityEngine.CoreModule.dll", c"UnityEngine", c"Object");
    let sprite_renderer_class = runtime.class(
        c"UnityEngine.CoreModule.dll",
        c"UnityEngine",
        c"SpriteRenderer",
    );
    let renderer_class = runtime.class(c"UnityEngine.CoreModule.dll", c"UnityEngine", c"Renderer");
    let tactical_cell_class = resolve_tactical_class(runtime)?;
    let addressable_class = runtime.class(
        c"Ankama.AddressableUtility.Runtime.dll",
        c"Ankama.AddressableUtilities.Runtime",
        c"AddressableUtility",
    );

    if [
        map_renderer_class,
        game_object_class,
        renderer_class,
        sprite_renderer_class,
        tactical_cell_class,
        addressable_class,
    ]
    .iter()
    .any(|value| value.is_null())
    {
        runtime.log(
            DnhLogLevel::Error,
            "A required Unity or Dofus class was not resolved.",
        );
        return None;
    }

    let mut map_class = map_renderer_class;
    for _ in 0..12 {
        if map_class.is_null() {
            break;
        }
        runtime.log(
            DnhLogLevel::Info,
            &format!(
                "Native Tactical MapRenderer hierarchy {}: int64={}, int32={}, metadata={}, gameObjects={}.",
                runtime.class_name(map_class),
                runtime.fields_by_type(map_class, c"System.Int64").len(),
                runtime.fields_by_type(map_class, c"System.Int32").len(),
                runtime
                    .fields_by_type(map_class, c"Core.World.Metadata.Maps.MapMetadata")
                    .len(),
                runtime
                    .fields_by_type(map_class, c"UnityEngine.GameObject")
                    .len(),
            ),
        );
        map_class = runtime.class_parent(map_class);
    }

    let map_id_fields = runtime.fields_by_type(map_renderer_class, c"System.Int64");
    let map_id_field = *map_id_fields.first()?;
    let metadata_fields =
        runtime.fields_by_type(map_renderer_class, c"Core.World.Metadata.Maps.MapMetadata");
    let map_root_fields = runtime.fields_by_type(map_renderer_class, c"UnityEngine.GameObject");
    if metadata_fields.is_empty() || map_root_fields.is_empty() {
        runtime.log(
            DnhLogLevel::Error,
            "MapRenderer structural signature did not resolve metadata/root fields.",
        );
        return None;
    }

    let game_object_ctor =
        required(runtime.instance_method(game_object_class, c".ctor", &[c"System.String"]))?;
    let game_object_find = required(runtime.method(
        game_object_class,
        c"Find",
        &[c"System.String"],
        DNH_MEMBER_STORAGE_STATIC,
    ))?;
    let game_object_get_transform =
        required(runtime.instance_method(game_object_class, c"get_transform", &[]))?;
    let game_object_get_component =
        required(runtime.instance_method(game_object_class, c"GetComponent", &[c"System.Type"]))?;
    let game_object_get_components_in_children = required(runtime.instance_method(
        game_object_class,
        c"GetComponentsInChildren",
        &[c"System.Type", c"System.Boolean"],
    ))?;
    let game_object_set_active =
        required(runtime.instance_method(game_object_class, c"SetActive", &[c"System.Boolean"]))?;
    let component_get_game_object =
        required(runtime.instance_method(component_class, c"get_gameObject", &[]))?;
    let transform_find =
        required(runtime.instance_method(transform_class, c"Find", &[c"System.String"]))?;
    let transform_get_parent =
        required(runtime.instance_method(transform_class, c"get_parent", &[]))?;
    let transform_set_parent = required(runtime.instance_method(
        transform_class,
        c"SetParent",
        &[c"UnityEngine.Transform", c"System.Boolean"],
    ))?;
    let transform_get_local_position =
        required(runtime.instance_method(transform_class, c"get_localPosition", &[]))?;
    let transform_set_local_position = required(runtime.instance_method(
        transform_class,
        c"set_localPosition",
        &[c"UnityEngine.Vector3"],
    ))?;
    let transform_get_local_rotation =
        required(runtime.instance_method(transform_class, c"get_localRotation", &[]))?;
    let transform_set_local_rotation = required(runtime.instance_method(
        transform_class,
        c"set_localRotation",
        &[c"UnityEngine.Quaternion"],
    ))?;
    let transform_get_local_scale =
        required(runtime.instance_method(transform_class, c"get_localScale", &[]))?;
    let transform_set_local_scale = required(runtime.instance_method(
        transform_class,
        c"set_localScale",
        &[c"UnityEngine.Vector3"],
    ))?;
    let object_destroy = required(runtime.method(
        object_class,
        c"Destroy",
        &[c"UnityEngine.Object"],
        DNH_MEMBER_STORAGE_STATIC,
    ))?;
    let object_instantiate = required(runtime.method(
        object_class,
        c"Instantiate",
        &[c"UnityEngine.Object"],
        DNH_MEMBER_STORAGE_STATIC,
    ))?;
    let object_set_name =
        required(runtime.instance_method(object_class, c"set_name", &[c"System.String"]))?;
    let renderer_get_enabled =
        required(runtime.instance_method(renderer_class, c"get_enabled", &[]))?;
    let renderer_set_enabled =
        required(runtime.instance_method(renderer_class, c"set_enabled", &[c"System.Boolean"]))?;
    let sprite_renderer_get_sprite =
        required(runtime.instance_method(sprite_renderer_class, c"get_sprite", &[]))?;
    let sprite_renderer_set_sprite = required(runtime.instance_method(
        sprite_renderer_class,
        c"set_sprite",
        &[c"UnityEngine.Sprite"],
    ))?;
    runtime.log(
        DnhLogLevel::Info,
        "Native Tactical: Unity object/transform/renderer bindings resolved.",
    );
    let Some((tactical_set_cell_id, tactical_apply_kind)) =
        resolve_tactical_methods(runtime, tactical_cell_class)
    else {
        runtime.log(
            DnhLogLevel::Error,
            &format!(
                "Native Tactical could not resolve the cell methods on {}.",
                runtime.class_name(tactical_cell_class)
            ),
        );
        for method in declared_methods(runtime, tactical_cell_class) {
            let Some(info) = method_info(runtime, method) else {
                continue;
            };
            let parameters = (0..info.parameter_count)
                .map(|index| {
                    text(unsafe { (runtime.v2().method_parameter_type_name)(method, index) })
                })
                .collect::<Vec<_>>()
                .join(", ");
            runtime.log(
                DnhLogLevel::Info,
                &format!(
                    "Native Tactical cell method {}({parameters}) -> {}, static={}, flags=0x{:x}.",
                    text(info.name),
                    text(info.return_type_name),
                    info.is_static,
                    info.flags
                ),
            );
        }
        return None;
    };
    let tactical_sprite_fields = runtime.fields_by_type(tactical_cell_class, c"UnityEngine.Sprite");
    if tactical_sprite_fields.len() < 2 {
        runtime.log(
            DnhLogLevel::Error,
            "TacticalDebugCell did not expose its two serialized Sprite fields.",
        );
        return None;
    }
    runtime.log(
        DnhLogLevel::Info,
        &format!(
            "Native Tactical: cell methods and {} Sprite field(s) resolved.",
            tactical_sprite_fields.len()
        ),
    );

    let load_generic = required(runtime.method(
        addressable_class,
        c"LoadAssetAsync",
        &[c"System.String", c"System.Int32"],
        DNH_MEMBER_STORAGE_STATIC,
    ))?;
    runtime.log(
        DnhLogLevel::Info,
        "Native Tactical: generic AddressableUtility.LoadAssetAsync method resolved.",
    );
    let addressable_load =
        required(runtime.inflate_generic_method(load_generic, &[game_object_class]));
    if addressable_load.is_none() {
        runtime.log(
            DnhLogLevel::Warn,
            "Native Tactical: LoadAssetAsync<GameObject> inflation is unavailable; loaded TacticalDebugCell objects will be used instead.",
        );
    }
    let addressable_release =
        resolve_generic_release(runtime, addressable_class, game_object_class);

    runtime.log(
        DnhLogLevel::Info,
        &format!(
            "MapRenderer signature: mapId={}, metadataCandidates={}, rootCandidates={}; tactical prefab resolved structurally.",
            runtime.field_name(map_id_field),
            metadata_fields.len(),
            map_root_fields.len()
        ),
    );

    Some(Bindings {
        map_renderer_class,
        game_object_class,
        renderer_class,
        sprite_renderer_class,
        tactical_cell_class,
        map_id_field,
        metadata_fields,
        map_root_fields,
        game_object_ctor,
        game_object_find,
        game_object_get_transform,
        game_object_get_component,
        game_object_get_components_in_children,
        game_object_set_active,
        component_get_game_object,
        transform_find,
        transform_get_parent,
        transform_set_parent,
        transform_get_local_position,
        transform_set_local_position,
        transform_get_local_rotation,
        transform_set_local_rotation,
        transform_get_local_scale,
        transform_set_local_scale,
        object_instantiate,
        object_destroy,
        object_set_name,
        renderer_get_enabled,
        renderer_set_enabled,
        sprite_renderer_get_sprite,
        sprite_renderer_set_sprite,
        tactical_set_cell_id,
        tactical_apply_kind,
        tactical_sprite_fields,
        addressable_load,
        addressable_release,
    })
}

fn find_game_object(state: &State, name: &str) -> DnhHandle {
    let managed_name = state.runtime.string(name);
    state
        .runtime
        .invoke(state.bindings.game_object_find, null_mut(), &[managed_name])
        .unwrap_or(null_mut())
}

fn read_cells(runtime: Runtime, metadata: DnhHandle) -> Option<[CellKind; MAP_CELL_COUNT]> {
    let map_data = runtime.read_reference(metadata, c"mapData")?;
    let raw_cells = runtime.read_reference(map_data, c"cellsData")?;
    let list_class = unsafe { (runtime.v2().get_object_class)(raw_cells) };
    let count_method = runtime.instance_method(list_class, c"get_Count", &[])?;
    let item_method = runtime.instance_method(list_class, c"get_Item", &[c"System.Int32"])?;
    let count: i32 = runtime.unbox(runtime.invoke(count_method, raw_cells, &[])?)?;
    if count < MAP_CELL_COUNT as i32 || count > 10_000 {
        return None;
    }

    let mut cells = [CellKind::Hidden; MAP_CELL_COUNT];
    let mut found = [false; MAP_CELL_COUNT];
    let mut found_count = 0;
    for index in 0..count {
        let cell = runtime
            .invoke(item_method, raw_cells, &[argument(&index)])
            .unwrap_or(null_mut());
        let Some(cell_id) = runtime.read_value::<i32>(cell, c"cellNumber") else {
            continue;
        };
        if !(0..MAP_CELL_COUNT as i32).contains(&cell_id) {
            continue;
        }
        let Some(movable) = runtime.read_value::<bool>(cell, c"mov") else {
            continue;
        };
        let Some(line_of_sight) = runtime.read_value::<bool>(cell, c"los") else {
            continue;
        };
        let cell_index = cell_id as usize;
        cells[cell_index] = if movable {
            CellKind::Walkable
        } else if line_of_sight {
            CellKind::Hidden
        } else {
            CellKind::LineOfSightObstacle
        };
        if !found[cell_index] {
            found[cell_index] = true;
            found_count += 1;
        }
    }
    (found_count == MAP_CELL_COUNT).then_some(cells)
}

fn has_map_visual_branch(state: &State, game_object: DnhHandle) -> bool {
    let transform = state
        .runtime
        .invoke(state.bindings.game_object_get_transform, game_object, &[])
        .unwrap_or(null_mut());
    if transform.is_null() {
        return false;
    }
    ["MergedBackground", "Layer_0", "MergedForeground"]
        .iter()
        .any(|name| {
            let managed_name = state.runtime.string(name);
            state
                .runtime
                .invoke(state.bindings.transform_find, transform, &[managed_name])
                .is_some_and(|value| !value.is_null())
        })
}

fn resolve_map_root(state: &State, renderer: DnhHandle, map_id: i64) -> DnhHandle {
    let mut fallback: DnhHandle = null_mut();
    for &field in &state.bindings.map_root_fields {
        let mut candidate: DnhHandle = null_mut();
        let read = unsafe {
            (state.runtime.v2().field_get_value)(
                renderer,
                field,
                (&mut candidate as *mut DnhHandle).cast(),
            )
        };
        if !read || candidate.is_null() {
            continue;
        }
        if fallback.is_null() {
            fallback = candidate;
        }
        if has_map_visual_branch(state, candidate) {
            return candidate;
        }
    }
    if fallback.is_null() {
        find_game_object(state, &format!("map_{map_id}"))
    } else {
        fallback
    }
}

fn try_resolve_map(
    state: &State,
) -> Option<(DnhHandle, DnhHandle, i64, [CellKind; MAP_CELL_COUNT])> {
    let count = unsafe {
        (state.runtime.v2().find_objects)(state.bindings.map_renderer_class, null_mut(), 0)
    };
    if count == 0 || count > 128 {
        return None;
    }
    let mut renderers = vec![null_mut(); count];
    unsafe {
        (state.runtime.v2().find_objects)(
            state.bindings.map_renderer_class,
            renderers.as_mut_ptr(),
            renderers.len(),
        );
    }
    for renderer in renderers {
        let mut map_id = 0_i64;
        if renderer.is_null()
            || !unsafe {
                (state.runtime.v2().field_get_value)(
                    renderer,
                    state.bindings.map_id_field,
                    (&mut map_id as *mut i64).cast(),
                )
            }
            || map_id <= 0
        {
            continue;
        }
        let root = resolve_map_root(state, renderer, map_id);
        if root.is_null() {
            continue;
        }
        for &metadata_field in &state.bindings.metadata_fields {
            let mut metadata = null_mut();
            if !unsafe {
                (state.runtime.v2().field_get_value)(
                    renderer,
                    metadata_field,
                    (&mut metadata as *mut DnhHandle).cast(),
                )
            } {
                continue;
            }
            if let Some(cells) = read_cells(state.runtime, metadata) {
                return Some((renderer, root, map_id, cells));
            }
        }
    }
    None
}

fn copy_transform(state: &State, source_object: DnhHandle, target_object: DnhHandle) -> bool {
    let Some(source) =
        state
            .runtime
            .invoke(state.bindings.game_object_get_transform, source_object, &[])
    else {
        return false;
    };
    let Some(target) =
        state
            .runtime
            .invoke(state.bindings.game_object_get_transform, target_object, &[])
    else {
        return false;
    };
    let parent = state
        .runtime
        .invoke(state.bindings.transform_get_parent, source, &[])
        .unwrap_or(null_mut());
    let world_position_stays = false;
    state.runtime.invoke(
        state.bindings.transform_set_parent,
        target,
        &[parent, argument(&world_position_stays)],
    );

    let position = state
        .runtime
        .invoke(state.bindings.transform_get_local_position, source, &[]);
    let rotation = state
        .runtime
        .invoke(state.bindings.transform_get_local_rotation, source, &[]);
    let scale = state
        .runtime
        .invoke(state.bindings.transform_get_local_scale, source, &[]);
    let Some(position) = position.and_then(|value| state.runtime.unbox::<Vector3>(value)) else {
        return false;
    };
    let Some(rotation) = rotation.and_then(|value| state.runtime.unbox::<Quaternion>(value)) else {
        return false;
    };
    let Some(scale) = scale.and_then(|value| state.runtime.unbox::<Vector3>(value)) else {
        return false;
    };
    state.runtime.invoke(
        state.bindings.transform_set_local_position,
        target,
        &[argument(&position)],
    );
    state.runtime.invoke(
        state.bindings.transform_set_local_rotation,
        target,
        &[argument(&rotation)],
    );
    state.runtime.invoke(
        state.bindings.transform_set_local_scale,
        target,
        &[argument(&scale)],
    );
    true
}

fn restore_renderers(state: &mut State) {
    for suppressed in state.suppressed.drain(..) {
        let renderer = state.runtime.gc_target(suppressed.gc_handle);
        if !renderer.is_null() {
            state.runtime.invoke(
                state.bindings.renderer_set_enabled,
                renderer,
                &[argument(&suppressed.was_enabled)],
            );
        }
        state.runtime.gc_free(suppressed.gc_handle);
    }
}

fn suppress_branch(state: &mut State, root: DnhHandle, branch_name: &str) {
    let root_transform = state
        .runtime
        .invoke(state.bindings.game_object_get_transform, root, &[])
        .unwrap_or(null_mut());
    if root_transform.is_null() {
        return;
    }
    let name = state.runtime.string(branch_name);
    let branch_transform = state
        .runtime
        .invoke(state.bindings.transform_find, root_transform, &[name])
        .unwrap_or(null_mut());
    if branch_transform.is_null() {
        return;
    }
    let branch_object = state
        .runtime
        .invoke(
            state.bindings.component_get_game_object,
            branch_transform,
            &[],
        )
        .unwrap_or(null_mut());
    if branch_object.is_null() {
        return;
    }
    let renderer_type =
        unsafe { (state.runtime.v2().class_type_object)(state.bindings.renderer_class) };
    let include_inactive = true;
    let array = state
        .runtime
        .invoke(
            state.bindings.game_object_get_components_in_children,
            branch_object,
            &[renderer_type, argument(&include_inactive)],
        )
        .unwrap_or(null_mut());
    if array.is_null() {
        return;
    }
    let count = unsafe { (state.runtime.v2().array_length)(array) };
    let data = unsafe { (state.runtime.v2().array_data)(array) };
    if data.is_null() {
        return;
    }
    let renderers = unsafe { std::slice::from_raw_parts(data.cast::<DnhHandle>(), count) };
    for &renderer in renderers {
        let Some(was_enabled) = state
            .runtime
            .invoke(state.bindings.renderer_get_enabled, renderer, &[])
            .and_then(|value| state.runtime.unbox::<bool>(value))
        else {
            continue;
        };
        let gc_handle = state.runtime.gc_new(renderer, false);
        if gc_handle != 0 {
            state.suppressed.push(SuppressedRenderer {
                gc_handle,
                was_enabled,
            });
        }
        let disabled = false;
        state.runtime.invoke(
            state.bindings.renderer_set_enabled,
            renderer,
            &[argument(&disabled)],
        );
    }
}

fn suppress_map(state: &mut State, root: DnhHandle) {
    for branch in [
        "MergedBackground",
        "Layer_0",
        "MergedForeground",
        "ParticlesParent",
        "AnimatedObjectRoot",
    ] {
        suppress_branch(state, root, branch);
    }
}

fn component(state: &State, game_object: DnhHandle, class: DnhHandle) -> DnhHandle {
    let type_object = unsafe { (state.runtime.v2().class_type_object)(class) };
    state
        .runtime
        .invoke(
            state.bindings.game_object_get_component,
            game_object,
            &[type_object],
        )
        .unwrap_or(null_mut())
}

fn set_active(state: &State, game_object: DnhHandle, active: bool) {
    state.runtime.invoke(
        state.bindings.game_object_set_active,
        game_object,
        &[argument(&active)],
    );
}

fn serialized_sprites(
    state: &State,
    tactical_cell: DnhHandle,
    current: DnhHandle,
) -> (DnhHandle, DnhHandle) {
    let mut candidates = Vec::new();
    let mut movement = null_mut();
    let mut line_of_sight = null_mut();
    for &field in &state.bindings.tactical_sprite_fields {
        let mut sprite: DnhHandle = null_mut();
        if !unsafe {
            (state.runtime.v2().field_get_value)(
                tactical_cell,
                field,
                (&mut sprite as *mut DnhHandle).cast(),
            )
        } || sprite.is_null()
        {
            continue;
        }
        let name = state.runtime.field_name(field).to_ascii_lowercase();
        if name.contains("mov") {
            movement = sprite;
        } else if name.contains("los") {
            line_of_sight = sprite;
        }
        if !candidates.contains(&sprite) {
            candidates.push(sprite);
        }
    }
    if movement.is_null() {
        movement = if candidates.contains(&current) {
            current
        } else {
            candidates.first().copied().unwrap_or(null_mut())
        };
    }
    if line_of_sight.is_null() {
        line_of_sight = candidates
            .iter()
            .copied()
            .find(|sprite| *sprite != movement)
            .or_else(|| candidates.last().copied())
            .unwrap_or(null_mut());
    }
    (movement, line_of_sight)
}

fn build_cell(
    state: &State,
    root: DnhHandle,
    prefab: DnhHandle,
    cell_id: usize,
    kind: CellKind,
) -> bool {
    let cell = state
        .runtime
        .invoke(state.bindings.object_instantiate, null_mut(), &[prefab])
        .unwrap_or(null_mut());
    if cell.is_null() {
        return false;
    }
    set_active(state, cell, false);
    let name = state.runtime.string(&format!("cell::{cell_id}"));
    state
        .runtime
        .invoke(state.bindings.object_set_name, cell, &[name]);

    let cell_transform = state
        .runtime
        .invoke(state.bindings.game_object_get_transform, cell, &[])
        .unwrap_or(null_mut());
    let root_transform = state
        .runtime
        .invoke(state.bindings.game_object_get_transform, root, &[])
        .unwrap_or(null_mut());
    if cell_transform.is_null() || root_transform.is_null() {
        return false;
    }
    let world_position_stays = false;
    state.runtime.invoke(
        state.bindings.transform_set_parent,
        cell_transform,
        &[root_transform, argument(&world_position_stays)],
    );
    let cell_id_i32 = cell_id as i32;
    let row = cell_id_i32 / MAP_WIDTH;
    let column = cell_id_i32 % MAP_WIDTH;
    let position = Vector3 {
        x: ORIGIN_X + column as f32 * CELL_WIDTH + (row & 1) as f32 * HALF_CELL_WIDTH,
        y: ORIGIN_Y - row as f32 * HALF_CELL_HEIGHT,
        z: 0.0,
    };
    state.runtime.invoke(
        state.bindings.transform_set_local_position,
        cell_transform,
        &[argument(&position)],
    );

    // Awake caches the SpriteRenderer, so activate before applying the native
    // TacticalDebugCell classification exactly like the former MelonLoader mod.
    set_active(state, cell, true);
    let tactical = component(state, cell, state.bindings.tactical_cell_class);
    let sprite_renderer = component(state, cell, state.bindings.sprite_renderer_class);
    if tactical.is_null() || sprite_renderer.is_null() {
        return false;
    }
    state.runtime.invoke(
        state.bindings.tactical_set_cell_id,
        tactical,
        &[argument(&cell_id_i32)],
    );
    let native_kind = match kind {
        CellKind::Hidden => 1_i32,
        CellKind::LineOfSightObstacle => 2_i32,
        CellKind::Walkable => 3_i32,
    };
    state.runtime.invoke(
        state.bindings.tactical_apply_kind,
        tactical,
        &[argument(&native_kind)],
    );

    let current = state
        .runtime
        .invoke(
            state.bindings.sprite_renderer_get_sprite,
            sprite_renderer,
            &[],
        )
        .unwrap_or(null_mut());
    let (movement, line_of_sight) = serialized_sprites(state, tactical, current);
    let visible = kind != CellKind::Hidden;
    state.runtime.invoke(
        state.bindings.renderer_set_enabled,
        sprite_renderer,
        &[argument(&visible)],
    );
    let expected = match kind {
        CellKind::Hidden => null_mut(),
        CellKind::LineOfSightObstacle => line_of_sight,
        CellKind::Walkable => movement,
    };
    if !expected.is_null() {
        state.runtime.invoke(
            state.bindings.sprite_renderer_set_sprite,
            sprite_renderer,
            &[expected],
        );
    }
    true
}

fn begin_overlay(
    state: &mut State,
    renderer: DnhHandle,
    normal_root: DnhHandle,
    map_id: i64,
    cells: [CellKind; MAP_CELL_COUNT],
) -> bool {
    let overlay = unsafe { (state.runtime.v2().object_new)(state.bindings.game_object_class) };
    if overlay.is_null() {
        return false;
    }
    let name = state
        .runtime
        .string(&format!("NativeTacticalOverlay_{map_id}"));
    state
        .runtime
        .invoke(state.bindings.game_object_ctor, overlay, &[name]);
    if !copy_transform(state, normal_root, overlay) {
        state
            .runtime
            .invoke(state.bindings.object_destroy, null_mut(), &[overlay]);
        return false;
    }
    let overlay_gc = state.runtime.gc_new(overlay, false);
    let renderer_gc = state.runtime.gc_new(renderer, false);
    if overlay_gc == 0 || renderer_gc == 0 {
        if overlay_gc != 0 {
            state.runtime.gc_free(overlay_gc);
        }
        if renderer_gc != 0 {
            state.runtime.gc_free(renderer_gc);
        }
        state
            .runtime
            .invoke(state.bindings.object_destroy, null_mut(), &[overlay]);
        return false;
    }
    suppress_map(state, normal_root);
    state.overlay_root_gc = overlay_gc;
    state.map_renderer_gc = renderer_gc;
    state.map_id = map_id;
    state.build = Some(BuildProgress {
        cells,
        next_cell: 0,
    });
    true
}

fn process_build(state: &mut State) {
    let Some(mut build) = state.build.take() else {
        return;
    };
    let root = state.runtime.gc_target(state.overlay_root_gc);
    let prefab = state.runtime.gc_target(state.prefab_gc);
    if root.is_null() || prefab.is_null() {
        state.runtime.log(
            DnhLogLevel::Error,
            "The tactical prefab or overlay root disappeared while building.",
        );
        disable_current_map(state, true);
        return;
    }
    let end = (build.next_cell + CELLS_BUILT_PER_TICK).min(MAP_CELL_COUNT);
    while build.next_cell < end {
        let cell_id = build.next_cell;
        if !build_cell(state, root, prefab, cell_id, build.cells[cell_id]) {
            state.runtime.log(
                DnhLogLevel::Error,
                &format!("Could not instantiate native tactical sprite for cell {cell_id}."),
            );
            disable_current_map(state, true);
            return;
        }
        build.next_cell += 1;
    }
    if build.next_cell == MAP_CELL_COUNT {
        state.runtime.log(
            DnhLogLevel::Info,
            &format!(
                "Native tactical mode enabled on map {} (560 native tacticalCell sprites).",
                state.map_id
            ),
        );
    } else {
        state.build = Some(build);
    }
}

fn destroy_handle(state: &State, handle: &mut DnhGcHandleV4) {
    if *handle == 0 {
        return;
    }
    let object = state.runtime.gc_target(*handle);
    if !object.is_null() {
        state
            .runtime
            .invoke(state.bindings.object_destroy, null_mut(), &[object]);
    }
    state.runtime.gc_free(*handle);
    *handle = 0;
}

fn disable_current_map(state: &mut State, restore: bool) {
    state.build = None;
    let mut overlay = std::mem::take(&mut state.overlay_root_gc);
    destroy_handle(state, &mut overlay);
    if restore {
        restore_renderers(state);
    } else {
        for suppressed in state.suppressed.drain(..) {
            state.runtime.gc_free(suppressed.gc_handle);
        }
    }
    if state.map_renderer_gc != 0 {
        state.runtime.gc_free(state.map_renderer_gc);
        state.map_renderer_gc = 0;
    }
    state.map_id = 0;
}

fn start_prefab_load(state: &mut State) -> bool {
    if state.prefab_gc != 0 || state.load_task_gc != 0 {
        return true;
    }
    for tactical_cell in state
        .runtime
        .find_objects(state.bindings.tactical_cell_class)
    {
        let game_object = state
            .runtime
            .invoke(state.bindings.component_get_game_object, tactical_cell, &[])
            .unwrap_or(null_mut());
        if game_object.is_null() {
            continue;
        }
        let prefab_gc = state.runtime.gc_new(game_object, false);
        if prefab_gc == 0 {
            continue;
        }
        state.prefab_gc = prefab_gc;
        state.prefab_is_addressable = false;
        state.runtime.log(
            DnhLogLevel::Info,
            "Reusing a TacticalDebugCell GameObject already loaded by the game.",
        );
        return true;
    }
    let Some(addressable_load) = state.bindings.addressable_load else {
        state.runtime.log(
            DnhLogLevel::Error,
            "No loaded TacticalDebugCell object or usable Addressables loader is available.",
        );
        return false;
    };
    let address = state.runtime.string("tacticalCell");
    let index_choice = 0_i32;
    let task = state
        .runtime
        .invoke(
            addressable_load,
            null_mut(),
            &[address, argument(&index_choice)],
        )
        .unwrap_or(null_mut());
    if task.is_null() {
        state.runtime.log(
            DnhLogLevel::Error,
            "AddressableUtility.LoadAssetAsync<GameObject>(\"tacticalCell\") failed.",
        );
        return false;
    }
    state.load_task_gc = state.runtime.gc_new(task, false);
    if state.load_task_gc == 0 {
        return false;
    }
    state.runtime.log(
        DnhLogLevel::Info,
        "Loading the game's native 'tacticalCell' Addressable...",
    );
    true
}

fn poll_prefab_load(state: &mut State) {
    if state.load_task_gc == 0 {
        return;
    }
    let task = state.runtime.gc_target(state.load_task_gc);
    if task.is_null() {
        state.runtime.gc_free(state.load_task_gc);
        state.load_task_gc = 0;
        return;
    }
    let task_class = unsafe { (state.runtime.v2().get_object_class)(task) };
    let is_completed =
        unsafe { (state.runtime.v2().get_method)(task_class, c"get_IsCompleted".as_ptr(), 0) };
    let result = unsafe { (state.runtime.v2().get_method)(task_class, c"get_Result".as_ptr(), 0) };
    if is_completed.is_null() || result.is_null() {
        state.runtime.log(
            DnhLogLevel::Error,
            "Could not resolve Task<GameObject> completion methods.",
        );
        state.runtime.gc_free(state.load_task_gc);
        state.load_task_gc = 0;
        return;
    }
    let completed = state
        .runtime
        .invoke(is_completed, task, &[])
        .and_then(|value| state.runtime.unbox::<bool>(value))
        .unwrap_or(false);
    if !completed {
        return;
    }
    let prefab = state
        .runtime
        .invoke(result, task, &[])
        .unwrap_or(null_mut());
    state.runtime.gc_free(state.load_task_gc);
    state.load_task_gc = 0;
    if prefab.is_null() {
        state.runtime.log(
            DnhLogLevel::Error,
            "The 'tacticalCell' Addressable completed without a GameObject.",
        );
        return;
    }
    state.prefab_gc = state.runtime.gc_new(prefab, false);
    if state.prefab_gc == 0 {
        return;
    }
    state.prefab_is_addressable = true;
    state.runtime.log(
        DnhLogLevel::Info,
        "Native tacticalCell prefab loaded; its serialized sprites will be reused directly.",
    );
    if state.enabled && state.map_id == 0 {
        enable_current_map(state, false);
    }
}

fn enable_current_map(state: &mut State, log_failure: bool) -> bool {
    if state.prefab_gc == 0 {
        return start_prefab_load(state);
    }
    let Some((renderer, normal_root, map_id, cells)) = try_resolve_map(state) else {
        if log_failure {
            state.runtime.log(
                DnhLogLevel::Warn,
                "Current map or its 560 tactical cells are not ready yet.",
            );
        }
        return false;
    };
    if !begin_overlay(state, renderer, normal_root, map_id, cells) {
        state.runtime.log(
            DnhLogLevel::Error,
            "Could not begin the native tactical sprite overlay.",
        );
        return false;
    }
    true
}

fn toggle(state: &mut State) {
    if state.enabled {
        state.enabled = false;
        disable_current_map(state, true);
        state
            .runtime
            .log(DnhLogLevel::Info, "Native tactical mode disabled.");
    } else {
        state.enabled = true;
        if !enable_current_map(state, true) {
            state.enabled = false;
        }
    }
}

fn follow_map_changes(state: &mut State) {
    if !state.enabled {
        return;
    }
    let renderer = state.runtime.gc_target(state.map_renderer_gc);
    let mut current_id = 0_i64;
    if !renderer.is_null()
        && unsafe {
            (state.runtime.v2().field_get_value)(
                renderer,
                state.bindings.map_id_field,
                (&mut current_id as *mut i64).cast(),
            )
        }
        && current_id == state.map_id
    {
        return;
    }
    if state.map_id != 0 {
        state.runtime.log(
            DnhLogLevel::Info,
            "Map changed; rebuilding the native tactical overlay.",
        );
        disable_current_map(state, false);
    }
    enable_current_map(state, false);
}

fn release_prefab(state: &mut State) {
    if state.load_task_gc != 0 {
        state.runtime.gc_free(state.load_task_gc);
        state.load_task_gc = 0;
    }
    if state.prefab_gc == 0 {
        return;
    }
    let prefab = state.runtime.gc_target(state.prefab_gc);
    if state.prefab_is_addressable
        && !prefab.is_null()
        && let Some(release) = state.bindings.addressable_release
    {
        state.runtime.invoke(release, null_mut(), &[prefab]);
    }
    state.runtime.gc_free(state.prefab_gc);
    state.prefab_gc = 0;
    state.prefab_is_addressable = false;
}

static MOD_ID: &[u8] = b"bubble.dofus3.native-tactical\0";
static MOD_NAME: &[u8] = b"Dofus 3 Native Tactical\0";
static MOD_VERSION: &[u8] = b"5.0.2\0";
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

#[link(name = "user32")]
unsafe extern "system" {
    fn GetAsyncKeyState(virtual_key: i32) -> i16;
}

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
    if host_api.is_null() {
        return DNH_ERROR;
    }
    let entry_message = b"Native Tactical: DNM_Load entered.";
    unsafe {
        ((*host_api).log)(
            DnhLogLevel::Info,
            entry_message.as_ptr(),
            entry_message.len(),
        );
    }
    let Some(runtime) = (unsafe { Runtime::bind(host_api) }) else {
        let error_message = b"Native Tactical: Runtime rejected the negotiated host ABI.";
        unsafe {
            ((*host_api).log)(
                DnhLogLevel::Error,
                error_message.as_ptr(),
                error_message.len(),
            );
        }
        return DNH_ERROR;
    };
    let Some(bindings) = resolve_bindings(runtime) else {
        return DNH_ERROR;
    };
    let mut guard = state();
    if guard.is_some() {
        return DNH_ERROR;
    }
    *guard = Some(State {
        runtime,
        bindings,
        enabled: false,
        f8_was_down: false,
        tick_count: 0,
        map_id: 0,
        map_renderer_gc: 0,
        overlay_root_gc: 0,
        prefab_gc: 0,
        prefab_is_addressable: false,
        load_task_gc: 0,
        build: None,
        suppressed: Vec::new(),
    });
    runtime.log(
        DnhLogLevel::Info,
        "Ready. Press F8 to toggle native tactical mode (Rust SDK, native sprites, ABI v8).",
    );
    DNH_OK
}

#[unsafe(no_mangle)]
pub extern "system" fn DNM_Tick() {
    let mut guard = state();
    let Some(state) = guard.as_mut() else {
        return;
    };
    state.tick_count = state.tick_count.wrapping_add(1);
    let f8_down = unsafe { GetAsyncKeyState(VK_F8) } < 0;
    if f8_down && !state.f8_was_down {
        toggle(state);
    }
    state.f8_was_down = f8_down;
    poll_prefab_load(state);
    process_build(state);
    follow_map_changes(state);
}

#[unsafe(no_mangle)]
pub extern "system" fn DNM_Unload() {
    let mut guard = state();
    if let Some(mut state) = guard.take() {
        state.enabled = false;
        disable_current_map(&mut state, true);
        release_prefab(&mut state);
    }
}
