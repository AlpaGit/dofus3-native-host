use dofus_native_mod_api::{
    DNH_ABI_VERSION_6, DNH_ERROR, DNH_MEMBER_STORAGE_STATIC, DNH_OK, DnhGcHandleV4, DnhHandle,
    DnhHostApiV1, DnhLogLevel, DnhModInfoV1,
};
use dofus_native_mod_sdk::Runtime;
use serde_json::json;
use std::collections::{BTreeSet, VecDeque};
use std::ffi::c_char;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

const VK_F9: i32 = 0x78;
const RENDERERS_PER_TICK: usize = 12;
const MAX_TEXTURE_SIZE: i32 = 4096;
const MAX_TEXTURE_EXPORTS: usize = 512;
const ARRAY_PREVIEW_LIMIT: usize = 64;

const TEXTURE_PROPERTIES: &[&str] = &[
    "_MainTex",
    "_BaseMap",
    "_BaseColorMap",
    "_MaskTex",
    "_AlphaTex",
    "_GradientTex",
    "_RampTex",
    "_NoiseTex",
    "_Tex",
    "_Texture",
    "_UnlitColorMap",
];
const COLOR_PROPERTIES: &[&str] = &[
    "_Color",
    "_BaseColor",
    "_Tint",
    "_TintColor",
    "_EmissionColor",
    "_OutlineColor",
    "_BorderColor",
    "_InnerColor",
    "_InsideColor",
    "_OutsideColor",
    "_FillColor",
    "_MainColor",
    "_TeamColor",
    "_AllyColor",
    "_EnemyColor",
    "_SelectedColor",
    "_HighlightColor",
    "_ColorTop",
    "_ColorBottom",
];
const FLOAT_PROPERTIES: &[&str] = &[
    "_Alpha",
    "_Opacity",
    "_Intensity",
    "_Thickness",
    "_BorderWidth",
    "_OutlineWidth",
    "_Radius",
    "_Softness",
    "_Scale",
    "_Fill",
    "_ZWrite",
    "_SrcBlend",
    "_DstBlend",
    "_Stencil",
];
const VECTOR_PROPERTIES: &[&str] = &[
    "_MainTex_ST",
    "_Color",
    "_BaseColor",
    "_Tint",
    "_TintColor",
    "_EmissionColor",
    "_OutlineColor",
    "_BorderColor",
    "_InnerColor",
    "_InsideColor",
    "_OutsideColor",
    "_FillColor",
    "_MainColor",
    "_TeamColor",
    "_AllyColor",
    "_EnemyColor",
    "_SelectedColor",
    "_HighlightColor",
    "_ColorTop",
    "_ColorBottom",
];
const CANDIDATE_WORDS: &[&str] = &[
    "fight",
    "combat",
    "cell",
    "case",
    "slot",
    "team",
    "ally",
    "enemy",
    "blue",
    "red",
    "orange",
    "area",
    "unit",
    "moquette",
    "placement",
    "start",
    "selection",
    "target",
    "tactical",
    "floor",
    "grid",
];

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Vector2 {
    x: f32,
    y: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Vector3 {
    x: f32,
    y: f32,
    z: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Vector4 {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Color {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Color32 {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Bounds {
    center: Vector3,
    extents: Vector3,
}

#[derive(Clone, Copy)]
struct TextureBindings {
    render_texture_get_active: DnhHandle,
    render_texture_set_active: DnhHandle,
    render_texture_get_temporary: DnhHandle,
    render_texture_release_temporary: DnhHandle,
    graphics_blit: DnhHandle,
    texture_2d_ctor: DnhHandle,
    texture_2d_read_pixels: DnhHandle,
    texture_2d_apply: DnhHandle,
    image_conversion_encode_png: DnhHandle,
    object_destroy: DnhHandle,
}

#[derive(Clone, Copy)]
struct Bindings {
    renderer_class: DnhHandle,
    mesh_filter_type: DnhHandle,
    component_get_game_object: DnhHandle,
    component_get_transform: DnhHandle,
    game_object_get_transform: DnhHandle,
    game_object_get_active: DnhHandle,
    game_object_get_layer: DnhHandle,
    game_object_get_component: DnhHandle,
    transform_get_parent: DnhHandle,
    transform_get_position: DnhHandle,
    transform_get_local_position: DnhHandle,
    transform_get_local_scale: DnhHandle,
    transform_get_lossy_scale: DnhHandle,
    object_get_name: DnhHandle,
    object_get_instance_id: DnhHandle,
    renderer_get_visible: DnhHandle,
    renderer_get_enabled: DnhHandle,
    renderer_get_bounds: DnhHandle,
    renderer_get_shared_materials: DnhHandle,
    renderer_get_sorting_layer: DnhHandle,
    renderer_get_sorting_order: DnhHandle,
    sprite_renderer_get_sprite: DnhHandle,
    sprite_get_texture: DnhHandle,
    sprite_get_rect: DnhHandle,
    sprite_get_pixels_per_unit: DnhHandle,
    mesh_filter_get_shared_mesh: DnhHandle,
    skinned_get_shared_mesh: DnhHandle,
    mesh_get_vertex_count: DnhHandle,
    mesh_get_sub_mesh_count: DnhHandle,
    mesh_get_vertices: DnhHandle,
    mesh_get_uv: DnhHandle,
    mesh_get_colors: DnhHandle,
    mesh_get_colors_32: DnhHandle,
    material_get_shader: DnhHandle,
    material_get_render_queue: DnhHandle,
    material_has_property: DnhHandle,
    material_get_color: DnhHandle,
    material_get_float: DnhHandle,
    material_get_vector: DnhHandle,
    material_get_texture: DnhHandle,
    material_get_main_texture: DnhHandle,
    texture_get_width: DnhHandle,
    texture_get_height: DnhHandle,
    texture_export: Option<TextureBindings>,
}

struct TextureJob {
    gc_handle: DnhGcHandleV4,
    instance_id: i32,
    name: String,
    width: i32,
    height: i32,
}

struct DumpSession {
    root: PathBuf,
    renderers: VecDeque<DnhGcHandleV4>,
    textures: VecDeque<TextureJob>,
    texture_ids: BTreeSet<i32>,
    renderers_csv: BufWriter<File>,
    moquettes_csv: BufWriter<File>,
    errors: BufWriter<File>,
    discovered_renderers: usize,
    exported_rows: usize,
    moquette_rows: usize,
    exported_textures: usize,
    failed_textures: usize,
}

struct State {
    runtime: Runtime,
    bindings: Bindings,
    f9_was_down: bool,
    session: Option<DumpSession>,
}

unsafe impl Send for State {}

static STATE: Mutex<Option<State>> = Mutex::new(None);

static MOD_ID: &[u8] = b"bubble.dofus3.runtime-inspector\0";
static MOD_NAME: &[u8] = b"Dofus Runtime Inspector\0";
static MOD_VERSION: &[u8] = b"2.0.0\0";
static MOD_AUTHOR: &[u8] = b"Bubble\0";

struct SyncModInfo(DnhModInfoV1);
unsafe impl Sync for SyncModInfo {}

static MOD_INFO: SyncModInfo = SyncModInfo(DnhModInfoV1 {
    abi_version: DNH_ABI_VERSION_6,
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

fn state() -> MutexGuard<'static, Option<State>> {
    STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn required(value: Option<DnhHandle>) -> Option<DnhHandle> {
    value.filter(|handle| !handle.is_null())
}

fn argument<T>(value: &T) -> DnhHandle {
    std::ptr::from_ref(value).cast_mut().cast()
}

fn resolve_bindings(runtime: Runtime) -> Option<Bindings> {
    let core = c"UnityEngine.CoreModule.dll";
    let renderer_class = runtime.class(core, c"UnityEngine", c"Renderer");
    let component_class = runtime.class(core, c"UnityEngine", c"Component");
    let game_object_class = runtime.class(core, c"UnityEngine", c"GameObject");
    let transform_class = runtime.class(core, c"UnityEngine", c"Transform");
    let object_class = runtime.class(core, c"UnityEngine", c"Object");
    let sprite_renderer_class = runtime.class(core, c"UnityEngine", c"SpriteRenderer");
    let sprite_class = runtime.class(core, c"UnityEngine", c"Sprite");
    let mesh_filter_class = runtime.class(core, c"UnityEngine", c"MeshFilter");
    let skinned_class = runtime.class(core, c"UnityEngine", c"SkinnedMeshRenderer");
    let mesh_class = runtime.class(core, c"UnityEngine", c"Mesh");
    let material_class = runtime.class(core, c"UnityEngine", c"Material");
    let texture_class = runtime.class(core, c"UnityEngine", c"Texture");
    if [
        renderer_class,
        component_class,
        game_object_class,
        transform_class,
        object_class,
        sprite_renderer_class,
        sprite_class,
        mesh_filter_class,
        skinned_class,
        mesh_class,
        material_class,
        texture_class,
    ]
    .iter()
    .any(|class| class.is_null())
    {
        runtime.log(
            DnhLogLevel::Error,
            "Runtime Inspector: une classe Unity requise est introuvable.",
        );
        return None;
    }

    let mesh_filter_type = unsafe { (runtime.v2().class_type_object)(mesh_filter_class) };
    if mesh_filter_type.is_null() {
        return None;
    }

    let texture_export = resolve_texture_bindings(runtime, object_class);
    if texture_export.is_none() {
        runtime.log(
            DnhLogLevel::Warn,
            "Runtime Inspector: export PNG indisponible; les références de textures seront tout de même indexées.",
        );
    }

    Some(Bindings {
        renderer_class,
        mesh_filter_type,
        component_get_game_object: required(runtime.instance_method(
            component_class,
            c"get_gameObject",
            &[],
        ))?,
        component_get_transform: required(runtime.instance_method(
            component_class,
            c"get_transform",
            &[],
        ))?,
        game_object_get_transform: required(runtime.instance_method(
            game_object_class,
            c"get_transform",
            &[],
        ))?,
        game_object_get_active: required(runtime.instance_method(
            game_object_class,
            c"get_activeInHierarchy",
            &[],
        ))?,
        game_object_get_layer: required(runtime.instance_method(
            game_object_class,
            c"get_layer",
            &[],
        ))?,
        game_object_get_component: required(runtime.instance_method(
            game_object_class,
            c"GetComponent",
            &[c"System.Type"],
        ))?,
        transform_get_parent: required(runtime.instance_method(
            transform_class,
            c"get_parent",
            &[],
        ))?,
        transform_get_position: required(runtime.instance_method(
            transform_class,
            c"get_position",
            &[],
        ))?,
        transform_get_local_position: required(runtime.instance_method(
            transform_class,
            c"get_localPosition",
            &[],
        ))?,
        transform_get_local_scale: required(runtime.instance_method(
            transform_class,
            c"get_localScale",
            &[],
        ))?,
        transform_get_lossy_scale: required(runtime.instance_method(
            transform_class,
            c"get_lossyScale",
            &[],
        ))?,
        object_get_name: required(runtime.instance_method(object_class, c"get_name", &[]))?,
        object_get_instance_id: required(runtime.instance_method(
            object_class,
            c"GetInstanceID",
            &[],
        ))?,
        renderer_get_visible: required(runtime.instance_method(
            renderer_class,
            c"get_isVisible",
            &[],
        ))?,
        renderer_get_enabled: required(runtime.instance_method(
            renderer_class,
            c"get_enabled",
            &[],
        ))?,
        renderer_get_bounds: required(runtime.instance_method(renderer_class, c"get_bounds", &[]))?,
        renderer_get_shared_materials: required(runtime.instance_method(
            renderer_class,
            c"get_sharedMaterials",
            &[],
        ))?,
        renderer_get_sorting_layer: required(runtime.instance_method(
            renderer_class,
            c"get_sortingLayerName",
            &[],
        ))?,
        renderer_get_sorting_order: required(runtime.instance_method(
            renderer_class,
            c"get_sortingOrder",
            &[],
        ))?,
        sprite_renderer_get_sprite: required(runtime.instance_method(
            sprite_renderer_class,
            c"get_sprite",
            &[],
        ))?,
        sprite_get_texture: required(runtime.instance_method(sprite_class, c"get_texture", &[]))?,
        sprite_get_rect: required(runtime.instance_method(sprite_class, c"get_rect", &[]))?,
        sprite_get_pixels_per_unit: required(runtime.instance_method(
            sprite_class,
            c"get_pixelsPerUnit",
            &[],
        ))?,
        mesh_filter_get_shared_mesh: required(runtime.instance_method(
            mesh_filter_class,
            c"get_sharedMesh",
            &[],
        ))?,
        skinned_get_shared_mesh: required(runtime.instance_method(
            skinned_class,
            c"get_sharedMesh",
            &[],
        ))?,
        mesh_get_vertex_count: required(runtime.instance_method(
            mesh_class,
            c"get_vertexCount",
            &[],
        ))?,
        mesh_get_sub_mesh_count: required(runtime.instance_method(
            mesh_class,
            c"get_subMeshCount",
            &[],
        ))?,
        mesh_get_vertices: required(runtime.instance_method(mesh_class, c"get_vertices", &[]))?,
        mesh_get_uv: required(runtime.instance_method(mesh_class, c"get_uv", &[]))?,
        mesh_get_colors: required(runtime.instance_method(mesh_class, c"get_colors", &[]))?,
        mesh_get_colors_32: required(runtime.instance_method(mesh_class, c"get_colors32", &[]))?,
        material_get_shader: required(runtime.instance_method(material_class, c"get_shader", &[]))?,
        material_get_render_queue: required(runtime.instance_method(
            material_class,
            c"get_renderQueue",
            &[],
        ))?,
        material_has_property: required(runtime.instance_method(
            material_class,
            c"HasProperty",
            &[c"System.String"],
        ))?,
        material_get_color: required(runtime.instance_method(
            material_class,
            c"GetColor",
            &[c"System.String"],
        ))?,
        material_get_float: required(runtime.instance_method(
            material_class,
            c"GetFloat",
            &[c"System.String"],
        ))?,
        material_get_vector: required(runtime.instance_method(
            material_class,
            c"GetVector",
            &[c"System.String"],
        ))?,
        material_get_texture: required(runtime.instance_method(
            material_class,
            c"GetTexture",
            &[c"System.String"],
        ))?,
        material_get_main_texture: required(runtime.instance_method(
            material_class,
            c"get_mainTexture",
            &[],
        ))?,
        texture_get_width: required(runtime.instance_method(texture_class, c"get_width", &[]))?,
        texture_get_height: required(runtime.instance_method(texture_class, c"get_height", &[]))?,
        texture_export,
    })
}

fn resolve_texture_bindings(runtime: Runtime, object_class: DnhHandle) -> Option<TextureBindings> {
    let core = c"UnityEngine.CoreModule.dll";
    let render_texture = runtime.class(core, c"UnityEngine", c"RenderTexture");
    let texture_2d = runtime.class(core, c"UnityEngine", c"Texture2D");
    let graphics = runtime.class(core, c"UnityEngine", c"Graphics");
    let image_conversion = runtime.class(
        c"UnityEngine.ImageConversionModule.dll",
        c"UnityEngine",
        c"ImageConversion",
    );
    if [render_texture, texture_2d, graphics, image_conversion]
        .iter()
        .any(|class| class.is_null())
    {
        return None;
    }
    Some(TextureBindings {
        render_texture_get_active: required(runtime.method(
            render_texture,
            c"get_active",
            &[],
            DNH_MEMBER_STORAGE_STATIC,
        ))?,
        render_texture_set_active: required(runtime.method(
            render_texture,
            c"set_active",
            &[c"UnityEngine.RenderTexture"],
            DNH_MEMBER_STORAGE_STATIC,
        ))?,
        render_texture_get_temporary: required(runtime.method(
            render_texture,
            c"GetTemporary",
            &[
                c"System.Int32",
                c"System.Int32",
                c"System.Int32",
                c"UnityEngine.RenderTextureFormat",
            ],
            DNH_MEMBER_STORAGE_STATIC,
        ))?,
        render_texture_release_temporary: required(runtime.method(
            render_texture,
            c"ReleaseTemporary",
            &[c"UnityEngine.RenderTexture"],
            DNH_MEMBER_STORAGE_STATIC,
        ))?,
        graphics_blit: required(runtime.method(
            graphics,
            c"Blit",
            &[c"UnityEngine.Texture", c"UnityEngine.RenderTexture"],
            DNH_MEMBER_STORAGE_STATIC,
        ))?,
        texture_2d_ctor: required(runtime.instance_method(
            texture_2d,
            c".ctor",
            &[
                c"System.Int32",
                c"System.Int32",
                c"UnityEngine.TextureFormat",
                c"System.Boolean",
            ],
        ))?,
        texture_2d_read_pixels: required(runtime.instance_method(
            texture_2d,
            c"ReadPixels",
            &[
                c"UnityEngine.Rect",
                c"System.Int32",
                c"System.Int32",
                c"System.Boolean",
            ],
        ))?,
        texture_2d_apply: required(runtime.instance_method(texture_2d, c"Apply", &[]))?,
        image_conversion_encode_png: required(runtime.method(
            image_conversion,
            c"EncodeToPNG",
            &[c"UnityEngine.Texture2D"],
            DNH_MEMBER_STORAGE_STATIC,
        ))?,
        object_destroy: required(runtime.method(
            object_class,
            c"Destroy",
            &[c"UnityEngine.Object"],
            DNH_MEMBER_STORAGE_STATIC,
        ))?,
    })
}

fn invoke_value<T: Copy>(
    runtime: Runtime,
    method: DnhHandle,
    object: DnhHandle,
    arguments: &[DnhHandle],
) -> Option<T> {
    runtime
        .invoke(method, object, arguments)
        .and_then(|boxed| runtime.unbox::<T>(boxed))
}

fn object_name(runtime: Runtime, bindings: Bindings, object: DnhHandle) -> String {
    runtime
        .invoke(bindings.object_get_name, object, &[])
        .and_then(|value| runtime.managed_string(value))
        .unwrap_or_default()
}

fn object_path(runtime: Runtime, bindings: Bindings, game_object: DnhHandle) -> String {
    let Some(mut transform) = runtime.invoke(bindings.game_object_get_transform, game_object, &[])
    else {
        return object_name(runtime, bindings, game_object);
    };
    let mut names = Vec::new();
    for _ in 0..256 {
        if transform.is_null() {
            break;
        }
        names.push(object_name(runtime, bindings, transform));
        transform = runtime
            .invoke(bindings.transform_get_parent, transform, &[])
            .unwrap_or(null_mut());
    }
    names.reverse();
    names.join("/")
}

fn vector3_text(value: Vector3) -> String {
    format!("{:.3},{:.3},{:.3}", value.x, value.y, value.z)
}

fn vector2_text(value: Vector2) -> String {
    format!("{:.3},{:.3}", value.x, value.y)
}

fn vector4_text(value: Vector4) -> String {
    format!(
        "{:.3},{:.3},{:.3},{:.3}",
        value.x, value.y, value.z, value.w
    )
}

fn color_text(value: Color) -> String {
    format!(
        "{:.3},{:.3},{:.3},{:.3}",
        value.r, value.g, value.b, value.a
    )
}

fn bounds_text(value: Bounds) -> String {
    let size = Vector3 {
        x: value.extents.x * 2.0,
        y: value.extents.y * 2.0,
        z: value.extents.z * 2.0,
    };
    format!(
        "center={} size={}",
        vector3_text(value.center),
        vector3_text(size)
    )
}

fn rect_text(value: Rect) -> String {
    format!(
        "{:.3},{:.3},{:.3},{:.3}",
        value.x, value.y, value.width, value.height
    )
}

fn csv(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn is_candidate(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    CANDIDATE_WORDS.iter().any(|word| lower.contains(word))
}

fn property_exists(
    runtime: Runtime,
    bindings: Bindings,
    material: DnhHandle,
    property: &str,
) -> bool {
    let name = runtime.string(property);
    invoke_value::<bool>(runtime, bindings.material_has_property, material, &[name])
        .unwrap_or(false)
}

fn describe_scalar_properties(
    runtime: Runtime,
    bindings: Bindings,
    material: DnhHandle,
) -> (String, String, String) {
    let colors = COLOR_PROPERTIES
        .iter()
        .filter_map(|property| {
            if !property_exists(runtime, bindings, material, property) {
                return None;
            }
            let name = runtime.string(property);
            invoke_value::<Color>(runtime, bindings.material_get_color, material, &[name])
                .map(|value| format!("{property}={}", color_text(value)))
        })
        .collect::<Vec<_>>()
        .join(";");
    let floats = FLOAT_PROPERTIES
        .iter()
        .filter_map(|property| {
            if !property_exists(runtime, bindings, material, property) {
                return None;
            }
            let name = runtime.string(property);
            invoke_value::<f32>(runtime, bindings.material_get_float, material, &[name])
                .map(|value| format!("{property}={value:.3}"))
        })
        .collect::<Vec<_>>()
        .join(";");
    let vectors = VECTOR_PROPERTIES
        .iter()
        .filter_map(|property| {
            if !property_exists(runtime, bindings, material, property) {
                return None;
            }
            let name = runtime.string(property);
            invoke_value::<Vector4>(runtime, bindings.material_get_vector, material, &[name])
                .map(|value| format!("{property}={}", vector4_text(value)))
        })
        .collect::<Vec<_>>()
        .join(";");
    (colors, floats, vectors)
}

fn queue_texture(
    runtime: Runtime,
    bindings: Bindings,
    session: &mut DumpSession,
    texture: DnhHandle,
) -> String {
    if texture.is_null() {
        return "<null>".to_owned();
    }
    let name = object_name(runtime, bindings, texture);
    let width = invoke_value::<i32>(runtime, bindings.texture_get_width, texture, &[]).unwrap_or(0);
    let height =
        invoke_value::<i32>(runtime, bindings.texture_get_height, texture, &[]).unwrap_or(0);
    let instance_id =
        invoke_value::<i32>(runtime, bindings.object_get_instance_id, texture, &[]).unwrap_or(0);
    let class_name = runtime.object_class_name(texture);
    if bindings.texture_export.is_some()
        && class_name.ends_with("Texture2D")
        && width > 0
        && height > 0
        && width <= MAX_TEXTURE_SIZE
        && height <= MAX_TEXTURE_SIZE
        && session.texture_ids.len() < MAX_TEXTURE_EXPORTS
        && session.texture_ids.insert(instance_id)
    {
        let gc_handle = runtime.gc_new(texture, false);
        if gc_handle != 0 {
            session.textures.push_back(TextureJob {
                gc_handle,
                instance_id,
                name: name.clone(),
                width,
                height,
            });
        }
    }
    format!("{name} {width}x{height} type={class_name} id={instance_id}")
}

fn describe_textures(
    runtime: Runtime,
    bindings: Bindings,
    session: &mut DumpSession,
    material: DnhHandle,
) -> String {
    let mut values = Vec::new();
    for property in TEXTURE_PROPERTIES {
        if !property_exists(runtime, bindings, material, property) {
            continue;
        }
        let name = runtime.string(property);
        let texture = runtime
            .invoke(bindings.material_get_texture, material, &[name])
            .unwrap_or(null_mut());
        values.push(format!(
            "{property}={}",
            queue_texture(runtime, bindings, session, texture)
        ));
    }
    let main_texture = runtime
        .invoke(bindings.material_get_main_texture, material, &[])
        .unwrap_or(null_mut());
    if !main_texture.is_null() {
        values.push(format!(
            "mainTexture={}",
            queue_texture(runtime, bindings, session, main_texture)
        ));
    }
    values.join(";")
}

fn value_array_text<T: Copy>(
    runtime: Runtime,
    array: DnhHandle,
    formatter: impl Fn(T) -> String,
) -> String {
    if array.is_null() {
        return String::new();
    }
    let length = unsafe { (runtime.v2().array_length)(array) };
    let data = unsafe { (runtime.v2().array_data)(array) };
    if length == 0 || data.is_null() {
        return String::new();
    }
    let count = length.min(ARRAY_PREVIEW_LIMIT);
    let values = unsafe { std::slice::from_raw_parts(data.cast::<T>(), count) };
    let mut text = values
        .iter()
        .copied()
        .map(formatter)
        .collect::<Vec<_>>()
        .join("|");
    if length > count {
        text.push_str(&format!("|...{} more", length - count));
    }
    text
}

struct MeshDescription {
    summary: String,
    vertices: String,
    uv: String,
    colors: String,
    colors_32: String,
}

fn describe_mesh(
    runtime: Runtime,
    bindings: Bindings,
    renderer: DnhHandle,
    game_object: DnhHandle,
    component_type: &str,
) -> MeshDescription {
    let mesh = if component_type.ends_with("SkinnedMeshRenderer") {
        runtime
            .invoke(bindings.skinned_get_shared_mesh, renderer, &[])
            .unwrap_or(null_mut())
    } else {
        let mesh_filter = runtime
            .invoke(
                bindings.game_object_get_component,
                game_object,
                &[bindings.mesh_filter_type],
            )
            .unwrap_or(null_mut());
        if mesh_filter.is_null() {
            null_mut()
        } else {
            runtime
                .invoke(bindings.mesh_filter_get_shared_mesh, mesh_filter, &[])
                .unwrap_or(null_mut())
        }
    };
    if mesh.is_null() {
        return MeshDescription {
            summary: String::new(),
            vertices: String::new(),
            uv: String::new(),
            colors: String::new(),
            colors_32: String::new(),
        };
    }
    let vertex_count =
        invoke_value::<i32>(runtime, bindings.mesh_get_vertex_count, mesh, &[]).unwrap_or(0);
    let sub_mesh_count =
        invoke_value::<i32>(runtime, bindings.mesh_get_sub_mesh_count, mesh, &[]).unwrap_or(0);
    let vertices = runtime
        .invoke(bindings.mesh_get_vertices, mesh, &[])
        .map(|array| value_array_text::<Vector3>(runtime, array, vector3_text))
        .unwrap_or_default();
    let uv = runtime
        .invoke(bindings.mesh_get_uv, mesh, &[])
        .map(|array| value_array_text::<Vector2>(runtime, array, vector2_text))
        .unwrap_or_default();
    let colors = runtime
        .invoke(bindings.mesh_get_colors, mesh, &[])
        .map(|array| value_array_text::<Color>(runtime, array, color_text))
        .unwrap_or_default();
    let colors_32 = runtime
        .invoke(bindings.mesh_get_colors_32, mesh, &[])
        .map(|array| {
            value_array_text::<Color32>(runtime, array, |value| {
                format!("{},{},{},{}", value.r, value.g, value.b, value.a)
            })
        })
        .unwrap_or_default();
    MeshDescription {
        summary: format!(
            "{} vertices={vertex_count} subMeshes={sub_mesh_count}",
            object_name(runtime, bindings, mesh)
        ),
        vertices,
        uv,
        colors,
        colors_32,
    }
}

fn write_renderer(
    runtime: Runtime,
    bindings: Bindings,
    renderer: DnhHandle,
    session: &mut DumpSession,
) {
    let game_object = runtime
        .invoke(bindings.component_get_game_object, renderer, &[])
        .unwrap_or(null_mut());
    if game_object.is_null() {
        return;
    }
    let active = invoke_value::<bool>(runtime, bindings.game_object_get_active, game_object, &[])
        .unwrap_or(false);
    let visible = invoke_value::<bool>(runtime, bindings.renderer_get_visible, renderer, &[])
        .unwrap_or(false);
    let enabled = invoke_value::<bool>(runtime, bindings.renderer_get_enabled, renderer, &[])
        .unwrap_or(false);
    let object_path = object_path(runtime, bindings, game_object);
    let component_type = runtime.object_class_name(renderer);
    let materials_array = runtime
        .invoke(bindings.renderer_get_shared_materials, renderer, &[])
        .unwrap_or(null_mut());
    let materials = runtime.array_references(materials_array);
    let material_names = materials
        .iter()
        .map(|material| object_name(runtime, bindings, *material))
        .collect::<Vec<_>>()
        .join(" ");
    if !active
        || (!visible && !is_candidate(&format!("{object_path} {component_type} {material_names}")))
    {
        return;
    }

    let transform = runtime
        .invoke(bindings.component_get_transform, renderer, &[])
        .unwrap_or(null_mut());
    let position =
        invoke_value::<Vector3>(runtime, bindings.transform_get_position, transform, &[])
            .map(vector3_text)
            .unwrap_or_default();
    let local_position = invoke_value::<Vector3>(
        runtime,
        bindings.transform_get_local_position,
        transform,
        &[],
    )
    .map(vector3_text)
    .unwrap_or_default();
    let local_scale =
        invoke_value::<Vector3>(runtime, bindings.transform_get_local_scale, transform, &[])
            .map(vector3_text)
            .unwrap_or_default();
    let lossy_scale =
        invoke_value::<Vector3>(runtime, bindings.transform_get_lossy_scale, transform, &[])
            .map(vector3_text)
            .unwrap_or_default();
    let bounds = invoke_value::<Bounds>(runtime, bindings.renderer_get_bounds, renderer, &[])
        .map(bounds_text)
        .unwrap_or_default();
    let layer =
        invoke_value::<i32>(runtime, bindings.game_object_get_layer, game_object, &[]).unwrap_or(0);
    let sorting_layer = runtime
        .invoke(bindings.renderer_get_sorting_layer, renderer, &[])
        .and_then(|value| runtime.managed_string(value))
        .unwrap_or_default();
    let sorting_order =
        invoke_value::<i32>(runtime, bindings.renderer_get_sorting_order, renderer, &[])
            .unwrap_or(0);

    let sprite = if component_type.ends_with("SpriteRenderer") {
        let sprite = runtime
            .invoke(bindings.sprite_renderer_get_sprite, renderer, &[])
            .unwrap_or(null_mut());
        if sprite.is_null() {
            String::new()
        } else {
            let texture = runtime
                .invoke(bindings.sprite_get_texture, sprite, &[])
                .unwrap_or(null_mut());
            let rect = invoke_value::<Rect>(runtime, bindings.sprite_get_rect, sprite, &[])
                .map(rect_text)
                .unwrap_or_default();
            let ppu =
                invoke_value::<f32>(runtime, bindings.sprite_get_pixels_per_unit, sprite, &[])
                    .unwrap_or(0.0);
            format!(
                "{} texture={} rect={rect} ppu={ppu:.3}",
                object_name(runtime, bindings, sprite),
                queue_texture(runtime, bindings, session, texture)
            )
        }
    } else {
        String::new()
    };
    let mesh = describe_mesh(runtime, bindings, renderer, game_object, &component_type);

    let row_materials = if materials.is_empty() {
        vec![null_mut()]
    } else {
        materials
    };
    for (material_index, material) in row_materials.into_iter().enumerate() {
        let (material_name, shader_name, render_queue, colors, floats, vectors, textures) =
            if material.is_null() {
                (
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                )
            } else {
                let shader = runtime
                    .invoke(bindings.material_get_shader, material, &[])
                    .unwrap_or(null_mut());
                let (colors, floats, vectors) =
                    describe_scalar_properties(runtime, bindings, material);
                let textures = describe_textures(runtime, bindings, session, material);
                (
                    object_name(runtime, bindings, material),
                    object_name(runtime, bindings, shader),
                    invoke_value::<i32>(runtime, bindings.material_get_render_queue, material, &[])
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    colors,
                    floats,
                    vectors,
                    textures,
                )
            };
        let material_index = if material.is_null() {
            "-1".to_owned()
        } else {
            material_index.to_string()
        };
        let row = [
            csv(&component_type),
            csv(&visible.to_string()),
            csv(&enabled.to_string()),
            csv(&active.to_string()),
            csv(&layer.to_string()),
            csv(&sorting_layer),
            csv(&sorting_order.to_string()),
            csv(&object_path),
            csv(&position),
            csv(&local_position),
            csv(&local_scale),
            csv(&lossy_scale),
            csv(&bounds),
            csv(&material_index),
            csv(&material_name),
            csv(&shader_name),
            csv(&render_queue),
            csv(&colors),
            csv(&floats),
            csv(&vectors),
            csv(&textures),
            csv(&sprite),
            csv(&mesh.summary),
            csv(&mesh.vertices),
            csv(&mesh.uv),
            csv(&mesh.colors),
            csv(&mesh.colors_32),
        ]
        .join(",");
        let _ = writeln!(session.renderers_csv, "{row}");
        session.exported_rows += 1;
        if object_path.to_ascii_lowercase().contains("moquette")
            || material_name.to_ascii_lowercase().contains("moquette")
            || shader_name.contains("SpriteCustom")
        {
            let _ = writeln!(session.moquettes_csv, "{row}");
            session.moquette_rows += 1;
        }
    }
}

fn sanitize_file_name(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            ) || character.is_control()
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    if sanitized.trim().is_empty() {
        "texture".to_owned()
    } else {
        sanitized
    }
}

fn export_texture(
    runtime: Runtime,
    bindings: Bindings,
    session: &mut DumpSession,
    job: TextureJob,
) -> bool {
    let Some(export) = bindings.texture_export else {
        runtime.gc_free(job.gc_handle);
        return false;
    };
    let source = runtime.gc_target(job.gc_handle);
    if source.is_null() {
        runtime.gc_free(job.gc_handle);
        return false;
    }
    let previous = runtime
        .invoke(export.render_texture_get_active, null_mut(), &[])
        .unwrap_or(null_mut());
    let depth = 0_i32;
    let argb_32 = 0_i32;
    let temporary = runtime
        .invoke(
            export.render_texture_get_temporary,
            null_mut(),
            &[
                argument(&job.width),
                argument(&job.height),
                argument(&depth),
                argument(&argb_32),
            ],
        )
        .unwrap_or(null_mut());
    if temporary.is_null() {
        runtime.gc_free(job.gc_handle);
        return false;
    }
    runtime.invoke(export.graphics_blit, null_mut(), &[source, temporary]);
    runtime.invoke(export.render_texture_set_active, null_mut(), &[temporary]);

    let texture_2d_class =
        runtime.class(c"UnityEngine.CoreModule.dll", c"UnityEngine", c"Texture2D");
    let copy = unsafe { (runtime.v2().object_new)(texture_2d_class) };
    let rgba_32 = 4_i32;
    let mip_chain = false;
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: job.width as f32,
        height: job.height as f32,
    };
    let zero = 0_i32;
    let recalculate = true;
    let mut bytes = None;
    if !copy.is_null() {
        runtime.invoke(
            export.texture_2d_ctor,
            copy,
            &[
                argument(&job.width),
                argument(&job.height),
                argument(&rgba_32),
                argument(&mip_chain),
            ],
        );
        runtime.invoke(
            export.texture_2d_read_pixels,
            copy,
            &[
                argument(&rect),
                argument(&zero),
                argument(&zero),
                argument(&recalculate),
            ],
        );
        runtime.invoke(export.texture_2d_apply, copy, &[]);
        if let Some(array) = runtime.invoke(export.image_conversion_encode_png, null_mut(), &[copy])
        {
            let length = unsafe { (runtime.v2().array_length)(array) };
            let data = unsafe { (runtime.v2().array_data)(array) };
            if length != 0 && !data.is_null() {
                bytes = Some(unsafe { std::slice::from_raw_parts(data.cast::<u8>(), length) });
            }
        }
    }

    let path = session.root.join("textures").join(format!(
        "{}_{}_{}x{}.png",
        sanitize_file_name(&job.name),
        job.instance_id,
        job.width,
        job.height
    ));
    let written = bytes.is_some_and(|png| std::fs::write(&path, png).is_ok());
    runtime.invoke(export.render_texture_set_active, null_mut(), &[previous]);
    runtime.invoke(
        export.render_texture_release_temporary,
        null_mut(),
        &[temporary],
    );
    if !copy.is_null() {
        runtime.invoke(export.object_destroy, null_mut(), &[copy]);
    }
    runtime.gc_free(job.gc_handle);
    written
}

fn session_root() -> Result<PathBuf, String> {
    let session = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let root = std::env::current_dir()
        .map_err(|error| format!("dossier courant inaccessible : {error}"))?
        .join("NativeMods")
        .join("DofusRuntimeDump")
        .join(session.to_string());
    std::fs::create_dir_all(root.join("textures"))
        .map_err(|error| format!("création de {} impossible : {error}", root.display()))?;
    Ok(root)
}

fn open_writer(path: &Path) -> Result<BufWriter<File>, String> {
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map(BufWriter::new)
        .map_err(|error| format!("création de {} impossible : {error}", path.display()))
}

fn start_dump(state: &mut State) {
    if state.session.is_some() {
        state.runtime.log(
            DnhLogLevel::Warn,
            "Runtime Inspector: un dump F9 est déjà en cours.",
        );
        return;
    }
    let root = match session_root() {
        Ok(root) => root,
        Err(error) => {
            state.runtime.log(DnhLogLevel::Error, &error);
            return;
        }
    };
    let mut renderers_csv = match open_writer(&root.join("renderers.csv")) {
        Ok(writer) => writer,
        Err(error) => {
            state.runtime.log(DnhLogLevel::Error, &error);
            return;
        }
    };
    let mut moquettes_csv = match open_writer(&root.join("moquettes.csv")) {
        Ok(writer) => writer,
        Err(error) => {
            state.runtime.log(DnhLogLevel::Error, &error);
            return;
        }
    };
    let errors = match open_writer(&root.join("errors.log")) {
        Ok(writer) => writer,
        Err(error) => {
            state.runtime.log(DnhLogLevel::Error, &error);
            return;
        }
    };
    let header = "componentType,visible,enabled,active,layer,sortingLayer,sortingOrder,objectPath,position,localPosition,localScale,lossyScale,bounds,materialIndex,material,shader,renderQueue,materialColors,materialFloats,materialVectors,materialTextures,sprite,mesh,vertices,uv,colors,colors32";
    let _ = writeln!(renderers_csv, "{header}");
    let _ = writeln!(moquettes_csv, "{header}");
    let objects = state.runtime.find_objects(state.bindings.renderer_class);
    let discovered_renderers = objects.len();
    let renderers = objects
        .into_iter()
        .filter_map(|object| {
            let handle = state.runtime.gc_new(object, false);
            (handle != 0).then_some(handle)
        })
        .collect::<VecDeque<_>>();
    let _ = std::fs::write(
        root.join("README.txt"),
        "Dofus Runtime Inspector (Rust)\n\
         renderers.csv: renderers actifs visibles ou liés au combat.\n\
         moquettes.csv: lignes Moquette/SpriteCustom, avec aperçu limité des tableaux de mesh.\n\
         textures/: copies PNG des Texture2D référencées, quand Unity autorise la lecture.\n\
         Le traitement est réparti sur plusieurs frames; attendez le message de fin dans native-host.log.\n",
    );
    state.runtime.log(
        DnhLogLevel::Info,
        &format!(
            "Runtime Inspector: F9 a trouvé {discovered_renderers} renderer(s); dump progressif vers {}.",
            root.display()
        ),
    );
    state.session = Some(DumpSession {
        root,
        renderers,
        textures: VecDeque::new(),
        texture_ids: BTreeSet::new(),
        renderers_csv,
        moquettes_csv,
        errors,
        discovered_renderers,
        exported_rows: 0,
        moquette_rows: 0,
        exported_textures: 0,
        failed_textures: 0,
    });
}

fn finish_dump(runtime: Runtime, mut session: DumpSession) {
    let _ = session.renderers_csv.flush();
    let _ = session.moquettes_csv.flush();
    let _ = session.errors.flush();
    let manifest = json!({
        "schemaVersion": 1,
        "mod": "Dofus Runtime Inspector",
        "version": "2.0.0",
        "discoveredRenderers": session.discovered_renderers,
        "exportedRendererRows": session.exported_rows,
        "moquetteRows": session.moquette_rows,
        "uniqueTextures": session.texture_ids.len(),
        "exportedTextures": session.exported_textures,
        "failedTextures": session.failed_textures,
        "arrayPreviewLimit": ARRAY_PREVIEW_LIMIT,
        "textureExportLimit": MAX_TEXTURE_EXPORTS,
    });
    let _ = serde_json::to_vec_pretty(&manifest)
        .ok()
        .and_then(|contents| std::fs::write(session.root.join("manifest.json"), contents).ok());
    runtime.log(
        DnhLogLevel::Info,
        &format!(
            "Runtime Inspector terminé: {} ligne(s), {} moquette(s), {}/{} texture(s), {}.",
            session.exported_rows,
            session.moquette_rows,
            session.exported_textures,
            session.texture_ids.len(),
            session.root.display()
        ),
    );
}

fn process_dump(state: &mut State) {
    let Some(mut session) = state.session.take() else {
        return;
    };
    for _ in 0..RENDERERS_PER_TICK {
        let Some(renderer_handle) = session.renderers.pop_front() else {
            break;
        };
        let renderer = state.runtime.gc_target(renderer_handle);
        if !renderer.is_null() {
            write_renderer(state.runtime, state.bindings, renderer, &mut session);
        }
        state.runtime.gc_free(renderer_handle);
    }
    if session.renderers.is_empty()
        && let Some(job) = session.textures.pop_front()
    {
        if export_texture(state.runtime, state.bindings, &mut session, job) {
            session.exported_textures += 1;
        } else {
            session.failed_textures += 1;
        }
    }
    if session.renderers.is_empty() && session.textures.is_empty() {
        finish_dump(state.runtime, session);
    } else {
        state.session = Some(session);
    }
}

fn cancel_dump(runtime: Runtime, session: &mut DumpSession) {
    for handle in session.renderers.drain(..) {
        runtime.gc_free(handle);
    }
    for job in session.textures.drain(..) {
        runtime.gc_free(job.gc_handle);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn DNM_Query(host_abi_version: u32) -> *const DnhModInfoV1 {
    if host_abi_version == DNH_ABI_VERSION_6 {
        &MOD_INFO.0
    } else {
        null()
    }
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `host_api` must be the process-lifetime ABI v6 table supplied by the host.
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
        f9_was_down: false,
        session: None,
    });
    runtime.log(
        DnhLogLevel::Info,
        "Dofus Runtime Inspector prêt. F9 lance un dump progressif des renderers, sprites, meshes, matériaux et textures.",
    );
    DNH_OK
}

#[unsafe(no_mangle)]
pub extern "system" fn DNM_Tick() {
    let mut guard = state();
    let Some(state) = guard.as_mut() else {
        return;
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let f9_down = unsafe { GetAsyncKeyState(VK_F9) } < 0;
        if f9_down && !state.f9_was_down {
            start_dump(state);
        }
        state.f9_was_down = f9_down;
        process_dump(state);
    }));
    if result.is_err() {
        state.runtime.log(
            DnhLogLevel::Error,
            "Runtime Inspector: panique Rust interceptée pendant le dump; la session est annulée.",
        );
        if let Some(mut session) = state.session.take() {
            cancel_dump(state.runtime, &mut session);
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn DNM_Unload() {
    let mut guard = state();
    if let Some(mut state) = guard.take()
        && let Some(mut session) = state.session.take()
    {
        cancel_dump(state.runtime, &mut session);
    }
}
