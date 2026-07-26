use dofus_native_mod_api::{
    DNH_ABI_VERSION_7, DNH_ERROR, DNH_MEMBER_STORAGE_STATIC, DNH_OK, DnhGcHandleV4, DnhHandle,
    DnhHostApiV1, DnhLogLevel, DnhModInfoV1,
};
use dofus_native_mod_sdk::Runtime;
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::{BTreeSet, VecDeque};
use std::ffi::{CStr, c_char};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

const VK_F10: i32 = 0x79;
const VK_F11: i32 = 0x7a;
const VK_F12: i32 = 0x7b;
const NODES_PER_TICK: usize = 32;
const MAX_NODES: usize = 50_000;
const MAX_DEPTH: usize = 64;
const MAX_RESOLUTIONS: usize = 10_000;

#[repr(C)]
#[derive(Clone, Copy, Default, Serialize)]
struct Color {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

#[derive(Clone, Copy)]
struct StyleBindings {
    get_width: DnhHandle,
    get_height: DnhHandle,
    get_display: DnhHandle,
    get_visibility: DnhHandle,
    get_opacity: DnhHandle,
    get_color: DnhHandle,
    get_background_color: DnhHandle,
    get_font_size: DnhHandle,
    get_font_weight: DnhHandle,
    get_letter_spacing: DnhHandle,
    get_paragraph_spacing: DnhHandle,
    get_outline_width: DnhHandle,
    get_position: DnhHandle,
    get_top: DnhHandle,
    get_right: DnhHandle,
    get_bottom: DnhHandle,
    get_left: DnhHandle,
    get_padding_top: DnhHandle,
    get_padding_right: DnhHandle,
    get_padding_bottom: DnhHandle,
    get_padding_left: DnhHandle,
    get_margin_top: DnhHandle,
    get_margin_right: DnhHandle,
    get_margin_bottom: DnhHandle,
    get_margin_left: DnhHandle,
    get_flex_direction: DnhHandle,
    get_flex_wrap: DnhHandle,
    get_flex_grow: DnhHandle,
    get_flex_shrink: DnhHandle,
    get_align_items: DnhHandle,
    get_align_self: DnhHandle,
    get_justify_content: DnhHandle,
    get_border_top_width: DnhHandle,
    get_border_top_left_radius: DnhHandle,
}

#[derive(Clone, Copy)]
struct Bindings {
    ui_document_class: DnhHandle,
    text_element_class: DnhHandle,
    font_asset_class: DnhHandle,
    object_get_name: DnhHandle,
    uidocument_get_root: DnhHandle,
    uidocument_get_panel_settings: DnhHandle,
    visual_get_name: DnhHandle,
    visual_get_tooltip: DnhHandle,
    visual_get_hierarchy: DnhHandle,
    visual_get_resolved_style: DnhHandle,
    text_get_text: DnhHandle,
    text_utilities_get_font_asset: DnhHandle,
    font_asset_get_face_info: DnhHandle,
    font_asset_get_population_mode: DnhHandle,
    font_asset_get_source_font: Option<DnhHandle>,
    font_asset_get_atlas_textures: Option<DnhHandle>,
    texture_get_width: Option<DnhHandle>,
    texture_get_height: Option<DnhHandle>,
    style: Option<StyleBindings>,
}

#[derive(Clone, Copy)]
struct FontCapture {
    runtime: Runtime,
    bindings: Bindings,
}

unsafe impl Send for FontCapture {}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResolutionRecord {
    captured_at_unix_ms: u128,
    source_font: String,
    font_asset: FontAssetSummary,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FontAssetSummary {
    object_name: String,
    family_name: String,
    style_name: String,
    point_size: Option<f32>,
    line_height: Option<f32>,
    atlas_population_mode: Option<i32>,
    source_font: String,
    atlas_textures: Vec<TextureSummary>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TextureSummary {
    name: String,
    width: Option<i32>,
    height: Option<i32>,
}

struct NodeJob {
    gc_handle: DnhGcHandleV4,
    depth: usize,
    parent_path: String,
    sibling_index: i32,
}

struct UiSession {
    sequence: u64,
    verbose: bool,
    queue: VecDeque<NodeJob>,
    visited: BTreeSet<usize>,
    tree: BufWriter<File>,
    texts: BufWriter<File>,
    total_nodes: usize,
    text_nodes: usize,
    truncated: bool,
}

struct State {
    runtime: Runtime,
    bindings: Bindings,
    root: PathBuf,
    resolution_writer: BufWriter<File>,
    ui_session: Option<UiSession>,
    sequence: u64,
    f10_was_down: bool,
    f11_was_down: bool,
    f12_was_down: bool,
}

unsafe impl Send for State {}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static FONT_CAPTURE: Mutex<Option<FontCapture>> = Mutex::new(None);
static RESOLUTION_QUEUE: Mutex<VecDeque<ResolutionRecord>> = Mutex::new(VecDeque::new());
static RESOLUTION_KEYS: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
static FONT_HOOK_TARGET: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(null_mut());
static FONT_HOOK_ORIGINAL: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(null_mut());

static MOD_ID: &[u8] = b"bubble.dofus3.interface-analyzer\0";
static MOD_NAME: &[u8] = b"Dofus Interface Analyzer\0";
static MOD_VERSION: &[u8] = b"2.0.0\0";
static MOD_AUTHOR: &[u8] = b"Bubble\0";

struct SyncModInfo(DnhModInfoV1);
unsafe impl Sync for SyncModInfo {}

static MOD_INFO: SyncModInfo = SyncModInfo(DnhModInfoV1 {
    abi_version: DNH_ABI_VERSION_7,
    struct_size: size_of::<DnhModInfoV1>() as u32,
    id: MOD_ID.as_ptr().cast::<c_char>(),
    name: MOD_NAME.as_ptr().cast::<c_char>(),
    version: MOD_VERSION.as_ptr().cast::<c_char>(),
    author: MOD_AUTHOR.as_ptr().cast::<c_char>(),
});

type FontResolverFn = unsafe extern "system" fn(DnhHandle, DnhHandle, DnhHandle) -> DnhHandle;

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

fn raw_method(runtime: Runtime, class: DnhHandle, name: &CStr, count: i32) -> Option<DnhHandle> {
    let method = unsafe { (runtime.v2().get_method)(class, name.as_ptr(), count) };
    (!method.is_null()).then_some(method)
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

fn invoke_virtual_value<T: Copy>(
    runtime: Runtime,
    method: DnhHandle,
    object: DnhHandle,
) -> Option<T> {
    runtime
        .invoke_virtual(method, object, &[])
        .and_then(|boxed| runtime.unbox::<T>(boxed))
}

fn managed_string(runtime: Runtime, value: DnhHandle) -> String {
    runtime.managed_string(value).unwrap_or_default()
}

fn object_name(runtime: Runtime, bindings: Bindings, object: DnhHandle) -> String {
    if object.is_null() {
        return String::new();
    }
    runtime
        .invoke(bindings.object_get_name, object, &[])
        .map(|value| managed_string(runtime, value))
        .unwrap_or_default()
}

fn resolve_style_bindings(runtime: Runtime, style_class: DnhHandle) -> Option<StyleBindings> {
    if style_class.is_null() {
        return None;
    }
    let method = |name: &CStr| raw_method(runtime, style_class, name, 0).unwrap_or(null_mut());
    let bindings = StyleBindings {
        get_width: method(c"get_width"),
        get_height: method(c"get_height"),
        get_display: method(c"get_display"),
        get_visibility: method(c"get_visibility"),
        get_opacity: method(c"get_opacity"),
        get_color: method(c"get_color"),
        get_background_color: method(c"get_backgroundColor"),
        get_font_size: method(c"get_fontSize"),
        get_font_weight: method(c"get_unityFontStyleAndWeight"),
        get_letter_spacing: method(c"get_letterSpacing"),
        get_paragraph_spacing: method(c"get_unityParagraphSpacing"),
        get_outline_width: method(c"get_unityTextOutlineWidth"),
        get_position: method(c"get_position"),
        get_top: method(c"get_top"),
        get_right: method(c"get_right"),
        get_bottom: method(c"get_bottom"),
        get_left: method(c"get_left"),
        get_padding_top: method(c"get_paddingTop"),
        get_padding_right: method(c"get_paddingRight"),
        get_padding_bottom: method(c"get_paddingBottom"),
        get_padding_left: method(c"get_paddingLeft"),
        get_margin_top: method(c"get_marginTop"),
        get_margin_right: method(c"get_marginRight"),
        get_margin_bottom: method(c"get_marginBottom"),
        get_margin_left: method(c"get_marginLeft"),
        get_flex_direction: method(c"get_flexDirection"),
        get_flex_wrap: method(c"get_flexWrap"),
        get_flex_grow: method(c"get_flexGrow"),
        get_flex_shrink: method(c"get_flexShrink"),
        get_align_items: method(c"get_alignItems"),
        get_align_self: method(c"get_alignSelf"),
        get_justify_content: method(c"get_justifyContent"),
        get_border_top_width: method(c"get_borderTopWidth"),
        get_border_top_left_radius: method(c"get_borderTopLeftRadius"),
    };
    (!bindings.get_width.is_null() && !bindings.get_height.is_null()).then_some(bindings)
}

fn resolve_bindings(runtime: Runtime) -> Option<Bindings> {
    let ui = c"UnityEngine.UIElementsModule.dll";
    let core = c"UnityEngine.CoreModule.dll";
    let text = c"UnityEngine.TextCoreTextEngineModule.dll";
    let ui_document_class = runtime.class(ui, c"UnityEngine.UIElements", c"UIDocument");
    let visual_element_class = runtime.class(ui, c"UnityEngine.UIElements", c"VisualElement");
    let text_element_class = runtime.class(ui, c"UnityEngine.UIElements", c"TextElement");
    let text_utilities_class = runtime.class(ui, c"UnityEngine.UIElements", c"TextUtilities");
    let style_class = runtime.class(ui, c"UnityEngine.UIElements", c"IResolvedStyle");
    let font_asset_class = runtime.class(text, c"UnityEngine.TextCore.Text", c"FontAsset");
    let object_class = runtime.class(core, c"UnityEngine", c"Object");
    if [
        ui_document_class,
        text_element_class,
        text_utilities_class,
        font_asset_class,
        object_class,
    ]
    .iter()
    .any(|class| class.is_null())
    {
        runtime.log(
            DnhLogLevel::Error,
            "Interface Analyzer: une classe UI Toolkit/TextCore requise est introuvable.",
        );
        return None;
    }
    let texture_class = runtime.class(core, c"UnityEngine", c"Texture");
    let font_asset_get_source_font =
        raw_method(runtime, font_asset_class, c"get_sourceFontFile", 0);
    let font_asset_get_atlas_textures =
        raw_method(runtime, font_asset_class, c"get_atlasTextures", 0);
    Some(Bindings {
        ui_document_class,
        text_element_class,
        font_asset_class,
        object_get_name: required(runtime.instance_method(object_class, c"get_name", &[]))?,
        uidocument_get_root: required(runtime.instance_method(
            ui_document_class,
            c"get_rootVisualElement",
            &[],
        ))?,
        uidocument_get_panel_settings: required(runtime.instance_method(
            ui_document_class,
            c"get_panelSettings",
            &[],
        ))?,
        visual_get_name: required(runtime.instance_method(visual_element_class, c"get_name", &[]))?,
        visual_get_tooltip: required(runtime.instance_method(
            visual_element_class,
            c"get_tooltip",
            &[],
        ))?,
        visual_get_hierarchy: required(runtime.instance_method(
            visual_element_class,
            c"get_hierarchy",
            &[],
        ))?,
        visual_get_resolved_style: required(runtime.instance_method(
            visual_element_class,
            c"get_resolvedStyle",
            &[],
        ))?,
        text_get_text: required(runtime.instance_method(text_element_class, c"get_text", &[]))?,
        text_utilities_get_font_asset: required(runtime.method(
            text_utilities_class,
            c"GetFontAsset",
            &[c"UnityEngine.UIElements.VisualElement"],
            DNH_MEMBER_STORAGE_STATIC,
        ))?,
        font_asset_get_face_info: required(runtime.instance_method(
            font_asset_class,
            c"get_faceInfo",
            &[],
        ))?,
        font_asset_get_population_mode: required(runtime.instance_method(
            font_asset_class,
            c"get_atlasPopulationMode",
            &[],
        ))?,
        font_asset_get_source_font,
        font_asset_get_atlas_textures,
        texture_get_width: (!texture_class.is_null())
            .then(|| raw_method(runtime, texture_class, c"get_width", 0))
            .flatten(),
        texture_get_height: (!texture_class.is_null())
            .then(|| raw_method(runtime, texture_class, c"get_height", 0))
            .flatten(),
        style: resolve_style_bindings(runtime, style_class),
    })
}

fn quiet_getter(runtime: Runtime, object: DnhHandle, property: &str) -> Option<DnhHandle> {
    if object.is_null() {
        return None;
    }
    let class = unsafe { (runtime.v2().get_object_class)(object) };
    let getter = std::ffi::CString::new(format!("get_{property}")).ok()?;
    let method = raw_method(runtime, class, &getter, 0)?;
    runtime.invoke(method, object, &[])
}

fn quiet_string_property(runtime: Runtime, object: DnhHandle, property: &str) -> Option<String> {
    let value = quiet_getter(runtime, object, property)?;
    let text = runtime.managed_string(value)?;
    (!text.is_empty()).then_some(text)
}

fn quiet_value_property<T: Copy>(runtime: Runtime, object: DnhHandle, property: &str) -> Option<T> {
    runtime.unbox(quiet_getter(runtime, object, property)?)
}

fn face_field_reference(runtime: Runtime, face: DnhHandle, field_name: &str) -> Option<DnhHandle> {
    if face.is_null() {
        return None;
    }
    let class = unsafe { (runtime.v2().get_object_class)(face) };
    let name = std::ffi::CString::new(field_name).ok()?;
    let field = unsafe { (runtime.v2().get_field)(class, name.as_ptr()) };
    if field.is_null() {
        return None;
    }
    let mut value = null_mut();
    unsafe { (runtime.v2().field_get_value)(face, field, (&mut value as *mut DnhHandle).cast()) }
        .then_some(value)
}

fn face_field_value<T: Copy>(runtime: Runtime, face: DnhHandle, field_name: &str) -> Option<T> {
    if face.is_null() {
        return None;
    }
    let class = unsafe { (runtime.v2().get_object_class)(face) };
    let name = std::ffi::CString::new(field_name).ok()?;
    let field = unsafe { (runtime.v2().get_field)(class, name.as_ptr()) };
    if field.is_null() {
        return None;
    }
    let mut value = std::mem::MaybeUninit::<T>::uninit();
    unsafe { (runtime.v2().field_get_value)(face, field, value.as_mut_ptr().cast()) }
        .then(|| unsafe { value.assume_init() })
}

fn face_string(runtime: Runtime, face: DnhHandle, field_name: &str) -> String {
    face_field_reference(runtime, face, field_name)
        .and_then(|value| runtime.managed_string(value))
        .unwrap_or_default()
}

fn font_asset_summary(
    runtime: Runtime,
    bindings: Bindings,
    font_asset: DnhHandle,
) -> FontAssetSummary {
    if font_asset.is_null() {
        return FontAssetSummary {
            object_name: String::new(),
            family_name: String::new(),
            style_name: String::new(),
            point_size: None,
            line_height: None,
            atlas_population_mode: None,
            source_font: String::new(),
            atlas_textures: Vec::new(),
        };
    }
    let face = runtime
        .invoke(bindings.font_asset_get_face_info, font_asset, &[])
        .unwrap_or(null_mut());
    let source_font = bindings
        .font_asset_get_source_font
        .and_then(|method| runtime.invoke(method, font_asset, &[]))
        .map(|font| object_name(runtime, bindings, font))
        .unwrap_or_default();
    let atlas_textures = bindings
        .font_asset_get_atlas_textures
        .and_then(|method| runtime.invoke(method, font_asset, &[]))
        .map(|array| {
            runtime
                .array_references(array)
                .into_iter()
                .filter(|texture| !texture.is_null())
                .take(32)
                .map(|texture| TextureSummary {
                    name: object_name(runtime, bindings, texture),
                    width: bindings
                        .texture_get_width
                        .and_then(|method| invoke_value(runtime, method, texture, &[])),
                    height: bindings
                        .texture_get_height
                        .and_then(|method| invoke_value(runtime, method, texture, &[])),
                })
                .collect()
        })
        .unwrap_or_default();
    FontAssetSummary {
        object_name: object_name(runtime, bindings, font_asset),
        family_name: face_string(runtime, face, "m_FamilyName"),
        style_name: face_string(runtime, face, "m_StyleName"),
        point_size: face_field_value(runtime, face, "m_PointSize"),
        line_height: face_field_value(runtime, face, "m_LineHeight"),
        atlas_population_mode: invoke_value(
            runtime,
            bindings.font_asset_get_population_mode,
            font_asset,
            &[],
        ),
        source_font,
        atlas_textures,
    }
}

fn capture_font_resolution(capture: FontCapture, source_font: DnhHandle, asset: DnhHandle) {
    if source_font.is_null() || asset.is_null() {
        return;
    }
    let source_font_name = object_name(capture.runtime, capture.bindings, source_font);
    let asset_name = object_name(capture.runtime, capture.bindings, asset);
    let key = format!("{source_font_name}\u{0}{asset_name}");
    if RESOLUTION_KEYS
        .lock()
        .map(|mut keys| !keys.insert(key))
        .unwrap_or(true)
    {
        return;
    }
    let record = ResolutionRecord {
        captured_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0),
        source_font: source_font_name,
        font_asset: font_asset_summary(capture.runtime, capture.bindings, asset),
    };
    if let Ok(mut queue) = RESOLUTION_QUEUE.lock() {
        if queue.len() >= MAX_RESOLUTIONS {
            queue.pop_front();
        }
        queue.push_back(record);
    }
}

unsafe extern "system" fn font_resolver_detour(
    this: DnhHandle,
    source_font: DnhHandle,
    method_info: DnhHandle,
) -> DnhHandle {
    let original = FONT_HOOK_ORIGINAL.load(Ordering::Acquire);
    let result = if original.is_null() {
        null_mut()
    } else {
        let original: FontResolverFn = unsafe { std::mem::transmute(original) };
        unsafe { original(this, source_font, method_info) }
    };
    let _ = std::panic::catch_unwind(|| {
        let capture = FONT_CAPTURE.lock().ok().and_then(|state| *state);
        if let Some(capture) = capture {
            capture_font_resolution(capture, source_font, result);
        }
    });
    result
}

fn install_font_hook(runtime: Runtime, bindings: Bindings) {
    let text_settings = runtime.class(
        c"UnityEngine.TextCoreTextEngineModule.dll",
        c"UnityEngine.TextCore.Text",
        c"TextSettings",
    );
    if text_settings.is_null() {
        runtime.log(
            DnhLogLevel::Warn,
            "Interface Analyzer: TextSettings introuvable; inventaire F10/F11 disponible sans capture dynamique.",
        );
        return;
    }
    let Some(method) =
        runtime.instance_method(text_settings, c"GetCachedFontAsset", &[c"UnityEngine.Font"])
    else {
        runtime.log(
            DnhLogLevel::Warn,
            "Interface Analyzer: GetCachedFontAsset(Font) introuvable.",
        );
        return;
    };
    let target = unsafe { (runtime.v2().method_address)(method) };
    if target.is_null() {
        return;
    }
    let Some(original) =
        runtime.create_hook(target, font_resolver_detour as *const () as DnhHandle)
    else {
        runtime.log(
            DnhLogLevel::Warn,
            "Interface Analyzer: création du hook de résolution de police impossible.",
        );
        return;
    };
    FONT_HOOK_ORIGINAL.store(original, Ordering::Release);
    FONT_HOOK_TARGET.store(target, Ordering::Release);
    if !runtime.enable_hook(target) {
        runtime.remove_hook(target);
        FONT_HOOK_ORIGINAL.store(null_mut(), Ordering::Release);
        FONT_HOOK_TARGET.store(null_mut(), Ordering::Release);
        return;
    }
    if let Ok(mut state) = FONT_CAPTURE.lock() {
        *state = Some(FontCapture { runtime, bindings });
    }
    runtime.log(
        DnhLogLevel::Info,
        "Interface Analyzer: capture Font -> FontAsset active.",
    );
}

fn remove_font_hook(runtime: Runtime) {
    let target = FONT_HOOK_TARGET.swap(null_mut(), Ordering::AcqRel);
    if !target.is_null() {
        runtime.disable_hook(target);
        runtime.remove_hook(target);
    }
    FONT_HOOK_ORIGINAL.store(null_mut(), Ordering::Release);
    if let Ok(mut capture) = FONT_CAPTURE.lock() {
        *capture = None;
    }
}

fn session_root() -> Result<PathBuf, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let root = std::env::current_dir()
        .map_err(|error| format!("dossier courant inaccessible : {error}"))?
        .join("NativeMods")
        .join("DofusInterfaceAnalyzer")
        .join(format!("session-{timestamp}"));
    std::fs::create_dir_all(&root)
        .map_err(|error| format!("création de {} impossible : {error}", root.display()))?;
    Ok(root)
}

fn writer(path: &Path) -> Result<BufWriter<File>, String> {
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map(BufWriter::new)
        .map_err(|error| format!("création de {} impossible : {error}", path.display()))
}

fn dump_font_inventory(
    runtime: Runtime,
    bindings: Bindings,
    root: &Path,
    sequence: u64,
) -> Result<PathBuf, String> {
    let fonts = runtime
        .find_objects(bindings.font_asset_class)
        .into_iter()
        .filter(|font| !font.is_null())
        .map(|font| font_asset_summary(runtime, bindings, font))
        .collect::<Vec<_>>();
    let path = root.join(format!("font-assets-{sequence}.json"));
    let document = json!({
        "schemaVersion": 1,
        "count": fonts.len(),
        "fontAssets": fonts,
    });
    let bytes = serde_json::to_vec_pretty(&document)
        .map_err(|error| format!("sérialisation inventaire impossible : {error}"))?;
    std::fs::write(&path, bytes)
        .map_err(|error| format!("écriture de {} impossible : {error}", path.display()))?;
    Ok(path)
}

fn csv(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn style_float(runtime: Runtime, style: DnhHandle, method: DnhHandle) -> Option<f32> {
    (!method.is_null())
        .then(|| invoke_virtual_value(runtime, method, style))
        .flatten()
}

fn style_int(runtime: Runtime, style: DnhHandle, method: DnhHandle) -> Option<i32> {
    (!method.is_null())
        .then(|| invoke_virtual_value(runtime, method, style))
        .flatten()
}

fn insert_float(map: &mut Map<String, Value>, key: &str, value: Option<f32>) {
    if let Some(value) = value.filter(|value| value.is_finite()) {
        map.insert(key.to_owned(), Value::from(value));
    }
}

fn insert_int(map: &mut Map<String, Value>, key: &str, value: Option<i32>) {
    if let Some(value) = value {
        map.insert(key.to_owned(), Value::from(value));
    }
}

fn style_summary(runtime: Runtime, bindings: Bindings, element: DnhHandle, verbose: bool) -> Value {
    let Some(methods) = bindings.style else {
        return Value::Null;
    };
    let style = runtime
        .invoke(bindings.visual_get_resolved_style, element, &[])
        .unwrap_or(null_mut());
    if style.is_null() {
        return Value::Null;
    }
    let mut values = Map::new();
    insert_float(
        &mut values,
        "width",
        style_float(runtime, style, methods.get_width),
    );
    insert_float(
        &mut values,
        "height",
        style_float(runtime, style, methods.get_height),
    );
    insert_int(
        &mut values,
        "display",
        style_int(runtime, style, methods.get_display),
    );
    insert_int(
        &mut values,
        "visibility",
        style_int(runtime, style, methods.get_visibility),
    );
    insert_float(
        &mut values,
        "opacity",
        style_float(runtime, style, methods.get_opacity),
    );
    for (name, method) in [
        ("color", methods.get_color),
        ("backgroundColor", methods.get_background_color),
    ] {
        if !method.is_null()
            && let Some(color) = invoke_virtual_value::<Color>(runtime, method, style)
        {
            values.insert(name.to_owned(), json!(color));
        }
    }
    insert_float(
        &mut values,
        "fontSize",
        style_float(runtime, style, methods.get_font_size),
    );
    insert_int(
        &mut values,
        "fontStyleAndWeight",
        style_int(runtime, style, methods.get_font_weight),
    );
    insert_float(
        &mut values,
        "letterSpacing",
        style_float(runtime, style, methods.get_letter_spacing),
    );
    insert_float(
        &mut values,
        "paragraphSpacing",
        style_float(runtime, style, methods.get_paragraph_spacing),
    );
    insert_float(
        &mut values,
        "outlineWidth",
        style_float(runtime, style, methods.get_outline_width),
    );
    if verbose {
        for (name, method) in [
            ("top", methods.get_top),
            ("right", methods.get_right),
            ("bottom", methods.get_bottom),
            ("left", methods.get_left),
            ("paddingTop", methods.get_padding_top),
            ("paddingRight", methods.get_padding_right),
            ("paddingBottom", methods.get_padding_bottom),
            ("paddingLeft", methods.get_padding_left),
            ("marginTop", methods.get_margin_top),
            ("marginRight", methods.get_margin_right),
            ("marginBottom", methods.get_margin_bottom),
            ("marginLeft", methods.get_margin_left),
            ("flexGrow", methods.get_flex_grow),
            ("flexShrink", methods.get_flex_shrink),
            ("borderTopWidth", methods.get_border_top_width),
            ("borderTopLeftRadius", methods.get_border_top_left_radius),
        ] {
            insert_float(&mut values, name, style_float(runtime, style, method));
        }
        for (name, method) in [
            ("position", methods.get_position),
            ("flexDirection", methods.get_flex_direction),
            ("flexWrap", methods.get_flex_wrap),
            ("alignItems", methods.get_align_items),
            ("alignSelf", methods.get_align_self),
            ("justifyContent", methods.get_justify_content),
        ] {
            insert_int(&mut values, name, style_int(runtime, style, method));
        }
    }
    Value::Object(values)
}

fn custom_properties(runtime: Runtime, element: DnhHandle, verbose: bool) -> Value {
    let mut values = Map::new();
    for property in [
        "title",
        "subtitle",
        "imgUrl",
        "text",
        "captionText",
        "choiceText",
        "buttonText",
        "buttonImageUrl",
        "imageUrl",
    ] {
        if let Some(value) = quiet_string_property(runtime, element, property) {
            values.insert(property.to_owned(), Value::String(value));
        }
    }
    if verbose {
        for property in ["isModal", "showCloseButton", "hideTitleBar", "isMovable"] {
            if let Some(value) = quiet_value_property::<bool>(runtime, element, property) {
                values.insert(property.to_owned(), Value::Bool(value));
            }
        }
        for property in [
            "selectedIndex",
            "mainStyle",
            "status",
            "size",
            "headerStyle",
            "dropdownType",
        ] {
            if let Some(value) = quiet_value_property::<i32>(runtime, element, property) {
                values.insert(property.to_owned(), Value::from(value));
            }
        }
    }
    Value::Object(values)
}

fn node_label(runtime: Runtime, bindings: Bindings, element: DnhHandle) -> (String, String) {
    let type_name = runtime.object_class_name(element);
    let name = runtime
        .invoke(bindings.visual_get_name, element, &[])
        .map(|value| managed_string(runtime, value))
        .unwrap_or_default();
    (type_name, name)
}

fn enqueue_node(
    runtime: Runtime,
    session: &mut UiSession,
    element: DnhHandle,
    depth: usize,
    parent_path: String,
    sibling_index: i32,
) {
    if element.is_null()
        || depth > MAX_DEPTH
        || session.total_nodes + session.queue.len() >= MAX_NODES
        || !session.visited.insert(element as usize)
    {
        if session.total_nodes + session.queue.len() >= MAX_NODES {
            session.truncated = true;
        }
        return;
    }
    let gc_handle = runtime.gc_new(element, false);
    if gc_handle != 0 {
        session.queue.push_back(NodeJob {
            gc_handle,
            depth,
            parent_path,
            sibling_index,
        });
    }
}

fn inspect_node(runtime: Runtime, bindings: Bindings, session: &mut UiSession, job: NodeJob) {
    let element = runtime.gc_target(job.gc_handle);
    if element.is_null() {
        runtime.gc_free(job.gc_handle);
        return;
    }
    let (type_name, name) = node_label(runtime, bindings, element);
    let segment = if name.is_empty() {
        format!("{type_name}[{}]", job.sibling_index)
    } else {
        format!("{type_name}#{name}[{}]", job.sibling_index)
    };
    let path = if job.parent_path.is_empty() {
        segment
    } else {
        format!("{}/{}", job.parent_path, segment)
    };
    let tooltip = runtime
        .invoke(bindings.visual_get_tooltip, element, &[])
        .map(|value| managed_string(runtime, value))
        .unwrap_or_default();
    let is_text = runtime.object_is(element, bindings.text_element_class);
    let text = if is_text {
        runtime
            .invoke_virtual(bindings.text_get_text, element, &[])
            .map(|value| managed_string(runtime, value))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let font_asset = is_text
        .then(|| {
            runtime
                .invoke(
                    bindings.text_utilities_get_font_asset,
                    null_mut(),
                    &[element],
                )
                .unwrap_or(null_mut())
        })
        .filter(|asset| !asset.is_null())
        .map(|asset| font_asset_summary(runtime, bindings, asset));
    let style = style_summary(runtime, bindings, element, session.verbose);
    let custom = custom_properties(runtime, element, session.verbose);
    let row = json!({
        "depth": job.depth,
        "path": path,
        "type": type_name,
        "name": name,
        "tooltip": tooltip,
        "text": is_text.then_some(&text),
        "fontAsset": font_asset,
        "resolvedStyle": style,
        "customProperties": custom,
    });
    if serde_json::to_writer(&mut session.tree, &row).is_ok() {
        let _ = session.tree.write_all(b"\n");
    }
    if is_text {
        let font_name = row["fontAsset"]["objectName"].as_str().unwrap_or_default();
        let family = row["fontAsset"]["familyName"].as_str().unwrap_or_default();
        let style_name = row["fontAsset"]["styleName"].as_str().unwrap_or_default();
        let _ = writeln!(
            session.texts,
            "{},{},{},{},{},{},{}",
            csv(&path),
            csv(&type_name),
            csv(&name),
            csv(&text),
            csv(font_name),
            csv(family),
            csv(style_name)
        );
        session.text_nodes += 1;
    }
    session.total_nodes += 1;

    let hierarchy = runtime
        .invoke(bindings.visual_get_hierarchy, element, &[])
        .unwrap_or(null_mut());
    let hierarchy_class = if hierarchy.is_null() {
        null_mut()
    } else {
        unsafe { (runtime.v2().get_object_class)(hierarchy) }
    };
    let hierarchy_value = runtime.unboxed_handle(hierarchy).unwrap_or(null_mut());
    let hierarchy_get_count =
        raw_method(runtime, hierarchy_class, c"get_childCount", 0).unwrap_or(null_mut());
    let hierarchy_get_item =
        raw_method(runtime, hierarchy_class, c"get_Item", 1).unwrap_or(null_mut());
    let child_count = if hierarchy_value.is_null() || hierarchy_get_count.is_null() {
        0
    } else {
        invoke_value::<i32>(runtime, hierarchy_get_count, hierarchy_value, &[])
            .unwrap_or(0)
            .clamp(0, 100_000)
    };
    for index in 0..child_count {
        let child = if hierarchy_get_item.is_null() {
            null_mut()
        } else {
            runtime
                .invoke(
                    hierarchy_get_item,
                    hierarchy_value,
                    &[std::ptr::from_ref(&index).cast_mut().cast()],
                )
                .unwrap_or(null_mut())
        };
        enqueue_node(runtime, session, child, job.depth + 1, path.clone(), index);
    }
    runtime.gc_free(job.gc_handle);
}

fn start_ui_dump(state: &mut State, verbose: bool) {
    if state.ui_session.is_some() {
        state.runtime.log(
            DnhLogLevel::Warn,
            "Interface Analyzer: une analyse UI est déjà en cours.",
        );
        return;
    }
    state.sequence = state.sequence.wrapping_add(1);
    let sequence = state.sequence;
    let mode = if verbose { "verbose" } else { "light" };
    let tree_path = state.root.join(format!("ui-tree-{sequence}-{mode}.jsonl"));
    let texts_path = state.root.join(format!("texts-{sequence}-{mode}.csv"));
    let Ok(tree) = writer(&tree_path) else {
        state
            .runtime
            .log(DnhLogLevel::Error, "Création du dump UI impossible.");
        return;
    };
    let Ok(mut texts) = writer(&texts_path) else {
        state
            .runtime
            .log(DnhLogLevel::Error, "Création du CSV de textes impossible.");
        return;
    };
    let _ = writeln!(texts, "path,type,name,text,fontAsset,familyName,styleName");
    let documents = state.runtime.find_objects(state.bindings.ui_document_class);
    let mut panels = Vec::new();
    let mut session = UiSession {
        sequence,
        verbose,
        queue: VecDeque::new(),
        visited: BTreeSet::new(),
        tree,
        texts,
        total_nodes: 0,
        text_nodes: 0,
        truncated: false,
    };
    for (index, document) in documents.into_iter().enumerate() {
        if document.is_null() {
            continue;
        }
        let root = state
            .runtime
            .invoke(state.bindings.uidocument_get_root, document, &[])
            .unwrap_or(null_mut());
        let panel = state
            .runtime
            .invoke(state.bindings.uidocument_get_panel_settings, document, &[])
            .unwrap_or(null_mut());
        let document_name = object_name(state.runtime, state.bindings, document);
        let panel_name = object_name(state.runtime, state.bindings, panel);
        panels.push(json!({
            "document": document_name,
            "panel": panel_name,
            "rootType": state.runtime.object_class_name(root),
        }));
        enqueue_node(
            state.runtime,
            &mut session,
            root,
            0,
            format!("UIDocument#{document_name}"),
            index as i32,
        );
    }
    let _ = serde_json::to_vec_pretty(&json!({
        "schemaVersion": 1,
        "sequence": sequence,
        "mode": mode,
        "panels": panels,
    }))
    .ok()
    .and_then(|contents| {
        std::fs::write(
            state.root.join(format!("panels-{sequence}-{mode}.json")),
            contents,
        )
        .ok()
    });
    if let Ok(path) = dump_font_inventory(state.runtime, state.bindings, &state.root, sequence) {
        state.runtime.log(
            DnhLogLevel::Info,
            &format!(
                "Interface Analyzer: inventaire de polices écrit dans {}.",
                path.display()
            ),
        );
    }
    state.runtime.log(
        DnhLogLevel::Info,
        &format!(
            "Interface Analyzer: analyse {mode} démarrée, {} racine(s), sortie {}.",
            session.queue.len(),
            tree_path.display()
        ),
    );
    state.ui_session = Some(session);
}

fn process_ui_dump(state: &mut State) {
    let Some(mut session) = state.ui_session.take() else {
        return;
    };
    for _ in 0..NODES_PER_TICK {
        let Some(job) = session.queue.pop_front() else {
            break;
        };
        inspect_node(state.runtime, state.bindings, &mut session, job);
    }
    if session.queue.is_empty() {
        let _ = session.tree.flush();
        let _ = session.texts.flush();
        let mode = if session.verbose { "verbose" } else { "light" };
        let manifest_path = state
            .root
            .join(format!("ui-manifest-{}-{mode}.json", session.sequence));
        let _ = serde_json::to_vec_pretty(&json!({
            "schemaVersion": 1,
            "sequence": session.sequence,
            "mode": mode,
            "nodes": session.total_nodes,
            "textNodes": session.text_nodes,
            "truncated": session.truncated,
            "maxNodes": MAX_NODES,
            "maxDepth": MAX_DEPTH,
        }))
        .ok()
        .and_then(|contents| std::fs::write(&manifest_path, contents).ok());
        state.runtime.log(
            DnhLogLevel::Info,
            &format!(
                "Interface Analyzer terminé: {} nœud(s), {} texte(s), manifeste {}.",
                session.total_nodes,
                session.text_nodes,
                manifest_path.display()
            ),
        );
    } else {
        state.ui_session = Some(session);
    }
}

fn drain_resolutions(state: &mut State) {
    let records = RESOLUTION_QUEUE
        .lock()
        .ok()
        .map(|mut queue| {
            let count = queue.len().min(256);
            queue.drain(..count).collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for record in records {
        if serde_json::to_writer(&mut state.resolution_writer, &record).is_ok() {
            let _ = state.resolution_writer.write_all(b"\n");
        }
    }
    let _ = state.resolution_writer.flush();
}

fn cancel_ui_dump(runtime: Runtime, session: &mut UiSession) {
    for job in session.queue.drain(..) {
        runtime.gc_free(job.gc_handle);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn DNM_Query(host_abi_version: u32) -> *const DnhModInfoV1 {
    if host_abi_version == DNH_ABI_VERSION_7 {
        &MOD_INFO.0
    } else {
        null()
    }
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `host_api` must be the process-lifetime ABI v7 table supplied by the host.
pub unsafe extern "system" fn DNM_Load(host_api: *const DnhHostApiV1) -> i32 {
    let Some(runtime) = (unsafe { Runtime::bind(host_api) }) else {
        return DNH_ERROR;
    };
    if runtime.v7().is_none() || runtime.v5_hooks().is_none() {
        runtime.log(
            DnhLogLevel::Error,
            "Dofus Interface Analyzer requiert l'ABI v7.",
        );
        return DNH_ERROR;
    }
    let Some(bindings) = resolve_bindings(runtime) else {
        return DNH_ERROR;
    };
    let root = match session_root() {
        Ok(root) => root,
        Err(error) => {
            runtime.log(DnhLogLevel::Error, &error);
            return DNH_ERROR;
        }
    };
    let resolution_writer = match writer(&root.join("font-resolutions.jsonl")) {
        Ok(writer) => writer,
        Err(error) => {
            runtime.log(DnhLogLevel::Error, &error);
            return DNH_ERROR;
        }
    };
    let _ = std::fs::write(
        root.join("README.txt"),
        "Dofus Interface Analyzer (Rust)\n\
         F10: inventaire live des FontAsset.\n\
         F11: arbre UI Toolkit progressif léger.\n\
         F12: arbre UI Toolkit progressif avec styles/layout étendus.\n\
         font-resolutions.jsonl: résolutions dynamiques Font -> FontAsset.\n\
         texts-*.csv: textes, chemins UI et polices effectivement résolues.\n",
    );
    let mut guard = state();
    if guard.is_some() {
        return DNH_ERROR;
    }
    *guard = Some(State {
        runtime,
        bindings,
        root: root.clone(),
        resolution_writer,
        ui_session: None,
        sequence: 0,
        f10_was_down: false,
        f11_was_down: false,
        f12_was_down: false,
    });
    install_font_hook(runtime, bindings);
    runtime.log(
        DnhLogLevel::Info,
        &format!(
            "Dofus Interface Analyzer prêt: F10 polices, F11 UI, F12 UI verbose. Sortie {}.",
            root.display()
        ),
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
        drain_resolutions(state);
        let f10 = unsafe { GetAsyncKeyState(VK_F10) } < 0;
        let f11 = unsafe { GetAsyncKeyState(VK_F11) } < 0;
        let f12 = unsafe { GetAsyncKeyState(VK_F12) } < 0;
        if f10 && !state.f10_was_down {
            state.sequence = state.sequence.wrapping_add(1);
            match dump_font_inventory(state.runtime, state.bindings, &state.root, state.sequence) {
                Ok(path) => state.runtime.log(
                    DnhLogLevel::Info,
                    &format!(
                        "Interface Analyzer: inventaire F10 écrit dans {}.",
                        path.display()
                    ),
                ),
                Err(error) => state.runtime.log(DnhLogLevel::Error, &error),
            }
        }
        if f11 && !state.f11_was_down {
            start_ui_dump(state, false);
        }
        if f12 && !state.f12_was_down {
            start_ui_dump(state, true);
        }
        state.f10_was_down = f10;
        state.f11_was_down = f11;
        state.f12_was_down = f12;
        process_ui_dump(state);
    }));
    if result.is_err() {
        state.runtime.log(
            DnhLogLevel::Error,
            "Interface Analyzer: panique Rust interceptée; analyse courante annulée.",
        );
        if let Some(mut session) = state.ui_session.take() {
            cancel_ui_dump(state.runtime, &mut session);
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn DNM_Unload() {
    let mut guard = state();
    if let Some(mut state) = guard.take() {
        remove_font_hook(state.runtime);
        drain_resolutions(&mut state);
        if let Some(mut session) = state.ui_session.take() {
            cancel_ui_dump(state.runtime, &mut session);
        }
        let _ = state.resolution_writer.flush();
    }
    if let Ok(mut queue) = RESOLUTION_QUEUE.lock() {
        queue.clear();
    }
    if let Ok(mut keys) = RESOLUTION_KEYS.lock() {
        keys.clear();
    }
}
