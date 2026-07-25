use dofus_native_mod_api::{
    DNH_ABI_VERSION_4, DNH_ERROR, DNH_MEMBER_STORAGE_STATIC, DNH_OK, DnhGcHandleV4, DnhHandle,
    DnhHostApiV1, DnhLogLevel, DnhModInfoV1,
};
use dofus_native_mod_sdk::Runtime;
use std::ffi::c_char;
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
const VK_F8: i32 = 0x77;

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

#[repr(C)]
#[derive(Clone, Copy)]
struct Color {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
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
    mesh_class: DnhHandle,
    mesh_filter_class: DnhHandle,
    mesh_renderer_class: DnhHandle,
    renderer_class: DnhHandle,
    material_class: DnhHandle,
    vector3_class: DnhHandle,
    color_class: DnhHandle,
    int32_class: DnhHandle,
    map_id_field: DnhHandle,
    metadata_fields: Vec<DnhHandle>,
    map_root_fields: Vec<DnhHandle>,
    game_object_ctor: DnhHandle,
    game_object_find: DnhHandle,
    game_object_get_transform: DnhHandle,
    game_object_add_component: DnhHandle,
    game_object_get_components_in_children: DnhHandle,
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
    object_destroy: DnhHandle,
    mesh_ctor: DnhHandle,
    mesh_set_vertices: DnhHandle,
    mesh_set_colors: DnhHandle,
    mesh_set_triangles: DnhHandle,
    mesh_recalculate_bounds: DnhHandle,
    mesh_filter_set_shared_mesh: DnhHandle,
    renderer_get_enabled: DnhHandle,
    renderer_set_enabled: DnhHandle,
    renderer_set_shared_material: DnhHandle,
    renderer_set_sorting_order: DnhHandle,
    shader_find: DnhHandle,
    material_ctor: DnhHandle,
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
    mesh_gc: DnhGcHandleV4,
    material_gc: DnhGcHandleV4,
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

fn resolve_bindings(runtime: Runtime) -> Option<Bindings> {
    let map_renderer_class = runtime.class(c"Core.dll", c"Core.Rendering", c"MapRenderer");
    let game_object_class =
        runtime.class(c"UnityEngine.CoreModule.dll", c"UnityEngine", c"GameObject");
    let transform_class =
        runtime.class(c"UnityEngine.CoreModule.dll", c"UnityEngine", c"Transform");
    let component_class =
        runtime.class(c"UnityEngine.CoreModule.dll", c"UnityEngine", c"Component");
    let object_class = runtime.class(c"UnityEngine.CoreModule.dll", c"UnityEngine", c"Object");
    let mesh_class = runtime.class(c"UnityEngine.CoreModule.dll", c"UnityEngine", c"Mesh");
    let mesh_filter_class =
        runtime.class(c"UnityEngine.CoreModule.dll", c"UnityEngine", c"MeshFilter");
    let mesh_renderer_class = runtime.class(
        c"UnityEngine.CoreModule.dll",
        c"UnityEngine",
        c"MeshRenderer",
    );
    let renderer_class = runtime.class(c"UnityEngine.CoreModule.dll", c"UnityEngine", c"Renderer");
    let material_class = runtime.class(c"UnityEngine.CoreModule.dll", c"UnityEngine", c"Material");
    let shader_class = runtime.class(c"UnityEngine.CoreModule.dll", c"UnityEngine", c"Shader");
    let vector3_class = runtime.class(c"UnityEngine.CoreModule.dll", c"UnityEngine", c"Vector3");
    let color_class = runtime.class(c"UnityEngine.CoreModule.dll", c"UnityEngine", c"Color");
    let int32_class = runtime.class(c"mscorlib.dll", c"System", c"Int32");

    if [
        map_renderer_class,
        game_object_class,
        mesh_class,
        mesh_filter_class,
        mesh_renderer_class,
        renderer_class,
        material_class,
        vector3_class,
        color_class,
        int32_class,
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
    let game_object_add_component =
        required(runtime.instance_method(game_object_class, c"AddComponent", &[c"System.Type"]))?;
    let game_object_get_components_in_children = required(runtime.instance_method(
        game_object_class,
        c"GetComponentsInChildren",
        &[c"System.Type", c"System.Boolean"],
    ))?;
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
    let mesh_ctor = required(runtime.instance_method(mesh_class, c".ctor", &[]))?;
    let mesh_set_vertices = required(runtime.instance_method(
        mesh_class,
        c"set_vertices",
        &[c"UnityEngine.Vector3[]"],
    ))?;
    let mesh_set_colors =
        required(runtime.instance_method(mesh_class, c"set_colors", &[c"UnityEngine.Color[]"]))?;
    let mesh_set_triangles =
        required(runtime.instance_method(mesh_class, c"set_triangles", &[c"System.Int32[]"]))?;
    let mesh_recalculate_bounds =
        required(runtime.instance_method(mesh_class, c"RecalculateBounds", &[]))?;
    let mesh_filter_set_shared_mesh = required(runtime.instance_method(
        mesh_filter_class,
        c"set_sharedMesh",
        &[c"UnityEngine.Mesh"],
    ))?;
    let renderer_get_enabled =
        required(runtime.instance_method(renderer_class, c"get_enabled", &[]))?;
    let renderer_set_enabled =
        required(runtime.instance_method(renderer_class, c"set_enabled", &[c"System.Boolean"]))?;
    let renderer_set_shared_material = required(runtime.instance_method(
        renderer_class,
        c"set_sharedMaterial",
        &[c"UnityEngine.Material"],
    ))?;
    let renderer_set_sorting_order =
        required(runtime.instance_method(renderer_class, c"set_sortingOrder", &[c"System.Int32"]))?;
    let shader_find = required(runtime.method(
        shader_class,
        c"Find",
        &[c"System.String"],
        DNH_MEMBER_STORAGE_STATIC,
    ))?;
    let material_ctor =
        required(runtime.instance_method(material_class, c".ctor", &[c"UnityEngine.Shader"]))?;

    runtime.log(
        DnhLogLevel::Info,
        &format!(
            "MapRenderer signature: mapId={}, metadataCandidates={}, rootCandidates={}.",
            runtime.field_name(map_id_field),
            metadata_fields.len(),
            map_root_fields.len()
        ),
    );

    Some(Bindings {
        map_renderer_class,
        game_object_class,
        mesh_class,
        mesh_filter_class,
        mesh_renderer_class,
        renderer_class,
        material_class,
        vector3_class,
        color_class,
        int32_class,
        map_id_field,
        metadata_fields,
        map_root_fields,
        game_object_ctor,
        game_object_find,
        game_object_get_transform,
        game_object_add_component,
        game_object_get_components_in_children,
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
        object_destroy,
        mesh_ctor,
        mesh_set_vertices,
        mesh_set_colors,
        mesh_set_triangles,
        mesh_recalculate_bounds,
        mesh_filter_set_shared_mesh,
        renderer_get_enabled,
        renderer_set_enabled,
        renderer_set_shared_material,
        renderer_set_sorting_order,
        shader_find,
        material_ctor,
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

fn add_component(state: &State, game_object: DnhHandle, class: DnhHandle) -> DnhHandle {
    let type_object = unsafe { (state.runtime.v2().class_type_object)(class) };
    state
        .runtime
        .invoke(
            state.bindings.game_object_add_component,
            game_object,
            &[type_object],
        )
        .unwrap_or(null_mut())
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

fn build_overlay(
    state: &mut State,
    normal_root: DnhHandle,
    map_id: i64,
    cells: &[CellKind; MAP_CELL_COUNT],
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
        return false;
    }

    let mut vertices = Vec::with_capacity(MAP_CELL_COUNT * 4);
    let mut colors = Vec::with_capacity(MAP_CELL_COUNT * 4);
    let mut triangles = Vec::with_capacity(MAP_CELL_COUNT * 6);
    for (cell_id, &kind) in cells.iter().enumerate() {
        if kind == CellKind::Hidden {
            continue;
        }
        let cell_id = cell_id as i32;
        let row = cell_id / MAP_WIDTH;
        let column = cell_id % MAP_WIDTH;
        let x = ORIGIN_X + column as f32 * CELL_WIDTH + (row & 1) as f32 * HALF_CELL_WIDTH;
        let y = ORIGIN_Y - row as f32 * HALF_CELL_HEIGHT;
        let first = vertices.len() as i32;
        vertices.extend_from_slice(&[
            Vector3 {
                x: x - HALF_CELL_WIDTH,
                y,
                z: 0.0,
            },
            Vector3 {
                x,
                y: y + HALF_CELL_HEIGHT,
                z: 0.0,
            },
            Vector3 {
                x: x + HALF_CELL_WIDTH,
                y,
                z: 0.0,
            },
            Vector3 {
                x,
                y: y - HALF_CELL_HEIGHT,
                z: 0.0,
            },
        ]);
        let color = if kind == CellKind::LineOfSightObstacle {
            Color {
                r: 0.32,
                g: 0.32,
                b: 0.32,
                a: 1.0,
            }
        } else if row & 1 == 0 {
            Color {
                r: 0.72,
                g: 0.72,
                b: 0.72,
                a: 1.0,
            }
        } else {
            Color {
                r: 0.52,
                g: 0.52,
                b: 0.52,
                a: 1.0,
            }
        };
        colors.extend_from_slice(&[color; 4]);
        triangles.extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
    }

    let vertex_array = state
        .runtime
        .array_from_slice(state.bindings.vector3_class, &vertices);
    let color_array = state
        .runtime
        .array_from_slice(state.bindings.color_class, &colors);
    let triangle_array = state
        .runtime
        .array_from_slice(state.bindings.int32_class, &triangles);
    if vertex_array.is_null() || color_array.is_null() || triangle_array.is_null() {
        return false;
    }

    let mesh = unsafe { (state.runtime.v2().object_new)(state.bindings.mesh_class) };
    if mesh.is_null() {
        return false;
    }
    state.runtime.invoke(state.bindings.mesh_ctor, mesh, &[]);
    state
        .runtime
        .invoke(state.bindings.mesh_set_vertices, mesh, &[vertex_array]);
    state
        .runtime
        .invoke(state.bindings.mesh_set_colors, mesh, &[color_array]);
    state
        .runtime
        .invoke(state.bindings.mesh_set_triangles, mesh, &[triangle_array]);
    state
        .runtime
        .invoke(state.bindings.mesh_recalculate_bounds, mesh, &[]);

    let mesh_filter = add_component(state, overlay, state.bindings.mesh_filter_class);
    let mesh_renderer = add_component(state, overlay, state.bindings.mesh_renderer_class);
    if mesh_filter.is_null() || mesh_renderer.is_null() {
        return false;
    }
    state.runtime.invoke(
        state.bindings.mesh_filter_set_shared_mesh,
        mesh_filter,
        &[mesh],
    );

    let mut shader_name = state.runtime.string("Sprites/Default");
    let mut shader = state
        .runtime
        .invoke(state.bindings.shader_find, null_mut(), &[shader_name])
        .unwrap_or(null_mut());
    if shader.is_null() {
        shader_name = state.runtime.string("UI/Default");
        shader = state
            .runtime
            .invoke(state.bindings.shader_find, null_mut(), &[shader_name])
            .unwrap_or(null_mut());
    }
    if shader.is_null() {
        return false;
    }
    let material = unsafe { (state.runtime.v2().object_new)(state.bindings.material_class) };
    if material.is_null() {
        return false;
    }
    state
        .runtime
        .invoke(state.bindings.material_ctor, material, &[shader]);
    state.runtime.invoke(
        state.bindings.renderer_set_shared_material,
        mesh_renderer,
        &[material],
    );
    let sorting_order = -560_i32;
    state.runtime.invoke(
        state.bindings.renderer_set_sorting_order,
        mesh_renderer,
        &[argument(&sorting_order)],
    );

    state.overlay_root_gc = state.runtime.gc_new(overlay, false);
    state.mesh_gc = state.runtime.gc_new(mesh, false);
    state.material_gc = state.runtime.gc_new(material, false);
    state.overlay_root_gc != 0
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
    let mut overlay = std::mem::take(&mut state.overlay_root_gc);
    let mut mesh = std::mem::take(&mut state.mesh_gc);
    let mut material = std::mem::take(&mut state.material_gc);
    destroy_handle(state, &mut overlay);
    destroy_handle(state, &mut mesh);
    destroy_handle(state, &mut material);
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

fn enable_current_map(state: &mut State, log_failure: bool) -> bool {
    let Some((renderer, normal_root, map_id, cells)) = try_resolve_map(state) else {
        if log_failure {
            state.runtime.log(
                DnhLogLevel::Warn,
                "Current map or its 560 tactical cells are not ready yet.",
            );
        }
        return false;
    };
    suppress_map(state, normal_root);
    if !build_overlay(state, normal_root, map_id, &cells) {
        restore_renderers(state);
        state.runtime.log(
            DnhLogLevel::Error,
            "Could not build the native tactical mesh.",
        );
        return false;
    }
    state.map_renderer_gc = state.runtime.gc_new(renderer, false);
    state.map_id = map_id;
    state.runtime.log(
        DnhLogLevel::Info,
        &format!("Native tactical mode enabled on map {map_id} (560 cells, one mesh)."),
    );
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
        enable_current_map(state, true);
    }
}

fn follow_map_changes(state: &mut State) {
    if !state.enabled || !state.tick_count.is_multiple_of(30) {
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

static MOD_ID: &[u8] = b"bubble.dofus3.native-tactical\0";
static MOD_NAME: &[u8] = b"Dofus 3 Native Tactical\0";
static MOD_VERSION: &[u8] = b"4.1.0\0";
static MOD_AUTHOR: &[u8] = b"Bubble\0";

struct SyncModInfo(DnhModInfoV1);
unsafe impl Sync for SyncModInfo {}

static MOD_INFO: SyncModInfo = SyncModInfo(DnhModInfoV1 {
    abi_version: DNH_ABI_VERSION_4,
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
    if host_abi_version == DNH_ABI_VERSION_4 {
        &MOD_INFO.0
    } else {
        null()
    }
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `host_api` must be the process-lifetime ABI v4 table supplied by the host.
pub unsafe extern "system" fn DNM_Load(host_api: *const DnhHostApiV1) -> i32 {
    let Some(runtime) = (unsafe { Runtime::bind(host_api) }) else {
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
        mesh_gc: 0,
        material_gc: 0,
        suppressed: Vec::new(),
    });
    runtime.log(
        DnhLogLevel::Info,
        "Ready. Press F8 to toggle native tactical mode (Rust SDK, pointer-safe ABI v4).",
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
    follow_map_changes(state);
}

#[unsafe(no_mangle)]
pub extern "system" fn DNM_Unload() {
    let mut guard = state();
    if let Some(mut state) = guard.take() {
        state.enabled = false;
        disable_current_map(&mut state, true);
    }
}
