use dofus_native_mod_api::{
    DNH_ABI_VERSION_8, DNH_ERROR, DNH_MEMBER_STORAGE_STATIC, DNH_OK, DnhGcHandleV4, DnhHandle,
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
    visual_get_child_count: DnhHandle,
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
    inside_popup: bool,
    legacy_header: Option<String>,
}

struct PendingNode {
    element: DnhHandle,
    depth: usize,
    parent_path: String,
    sibling_index: i32,
    inside_popup: bool,
    legacy_header: Option<String>,
}

struct UiSession {
    sequence: u64,
    verbose: bool,
    queue: Vec<NodeJob>,
    visited: BTreeSet<usize>,
    legacy: BufWriter<File>,
    tree: BufWriter<File>,
    texts: BufWriter<File>,
    total_nodes: usize,
    text_nodes: usize,
    skipped_hidden: usize,
    truncated: bool,
}

struct State {
    runtime: Runtime,
    bindings: Bindings,
    root: PathBuf,
    legacy_path: PathBuf,
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
static MOD_VERSION: &[u8] = b"2.0.2\0";
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

type FontResolverFn = unsafe extern "system" fn(DnhHandle, DnhHandle, DnhHandle) -> DnhHandle;

#[link(name = "user32")]
unsafe extern "system" {
    fn GetAsyncKeyState(virtual_key: i32) -> i16;
}

#[repr(C)]
#[derive(Default)]
struct LocalSystemTime {
    year: u16,
    month: u16,
    day_of_week: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    milliseconds: u16,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetLocalTime(system_time: *mut LocalSystemTime);
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
        visual_get_child_count: required(runtime.instance_method(
            visual_element_class,
            c"get_childCount",
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

fn append_writer(path: &Path) -> Result<BufWriter<File>, String> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map(BufWriter::new)
        .map_err(|error| format!("ouverture de {} impossible : {error}", path.display()))
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

fn legacy_timestamp() -> String {
    let mut value = LocalSystemTime::default();
    unsafe { GetLocalTime(&mut value) };
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        value.hour, value.minute, value.second, value.milliseconds
    )
}

fn legacy_log(writer: &mut BufWriter<File>, line: &str) {
    let _ = writeln!(writer, "[{}] {line}", legacy_timestamp());
}

fn legacy_text(value: &str, maximum_characters: usize) -> String {
    let escaped = value.replace('\r', "").replace('\n', "\\n");
    if escaped.chars().count() <= maximum_characters {
        return escaped;
    }
    let mut truncated = escaped.chars().take(maximum_characters).collect::<String>();
    truncated.push('…');
    truncated
}

fn short_type_name(type_name: &str) -> &str {
    type_name.rsplit('.').next().unwrap_or(type_name)
}

fn style_float_value(style: &Value, name: &str) -> Option<f64> {
    style.get(name).and_then(Value::as_f64)
}

fn style_int_value(style: &Value, name: &str) -> Option<i64> {
    style.get(name).and_then(Value::as_i64)
}

fn compact_float(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value.is_sign_positive() {
            "+Inf".to_owned()
        } else {
            "-Inf".to_owned()
        };
    }
    let rounded = value.round();
    if (value - rounded).abs() < 0.0005 {
        return format!("{}", rounded as i64);
    }
    let formatted = format!("{value:.2}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

fn color_hex(value: &Value) -> Option<String> {
    let channel = |name: &str| {
        value
            .get(name)
            .and_then(Value::as_f64)
            .map(|channel| (channel * 255.0).round().clamp(0.0, 255.0) as u8)
    };
    let (r, g, b, a) = (channel("r")?, channel("g")?, channel("b")?, channel("a")?);
    if a == 255 {
        Some(format!("#{r:02X}{g:02X}{b:02X}"))
    } else {
        Some(format!("#{r:02X}{g:02X}{b:02X}{a:02X}"))
    }
}

fn display_name(value: i64) -> String {
    match value {
        0 => "Flex".to_owned(),
        1 => "None".to_owned(),
        _ => value.to_string(),
    }
}

fn visibility_name(value: i64) -> String {
    match value {
        0 => "Visible".to_owned(),
        1 => "Hidden".to_owned(),
        _ => value.to_string(),
    }
}

fn position_name(value: i64) -> String {
    match value {
        0 => "Relative".to_owned(),
        1 => "Absolute".to_owned(),
        _ => value.to_string(),
    }
}

fn flex_direction_name(value: i64) -> String {
    match value {
        0 => "Column".to_owned(),
        1 => "ColumnReverse".to_owned(),
        2 => "Row".to_owned(),
        3 => "RowReverse".to_owned(),
        _ => value.to_string(),
    }
}

fn flex_wrap_name(value: i64) -> String {
    match value {
        0 => "NoWrap".to_owned(),
        1 => "Wrap".to_owned(),
        2 => "WrapReverse".to_owned(),
        _ => value.to_string(),
    }
}

fn align_name(value: i64) -> String {
    match value {
        0 => "Auto".to_owned(),
        1 => "FlexStart".to_owned(),
        2 => "Center".to_owned(),
        3 => "FlexEnd".to_owned(),
        4 => "Stretch".to_owned(),
        _ => value.to_string(),
    }
}

fn justify_name(value: i64) -> String {
    match value {
        0 => "FlexStart".to_owned(),
        1 => "Center".to_owned(),
        2 => "FlexEnd".to_owned(),
        3 => "SpaceBetween".to_owned(),
        4 => "SpaceAround".to_owned(),
        _ => value.to_string(),
    }
}

fn font_weight_name(value: i64) -> String {
    match value {
        0 => "Normal".to_owned(),
        1 => "Bold".to_owned(),
        2 => "Italic".to_owned(),
        3 => "BoldAndItalic".to_owned(),
        _ => value.to_string(),
    }
}

fn legacy_layout(style: &Value) -> Vec<String> {
    let mut parts = Vec::new();
    if let Some(value) = style_int_value(style, "display").filter(|value| *value != 0) {
        parts.push(format!("display={}", display_name(value)));
    }
    if let Some(value) = style_int_value(style, "visibility").filter(|value| *value != 0) {
        parts.push(format!("vis={}", visibility_name(value)));
    }
    if let Some(value) = style_float_value(style, "opacity").filter(|value| *value < 0.999) {
        parts.push(format!("op={}", compact_float(value)));
    }
    if let Some(value) = style_int_value(style, "position").filter(|value| *value != 0) {
        parts.push(format!("pos={}", position_name(value)));
    }
    for (key, label) in [
        ("top", "top"),
        ("right", "right"),
        ("bottom", "bottom"),
        ("left", "left"),
    ] {
        if let Some(value) =
            style_float_value(style, key).filter(|value| value.is_finite() && *value != 0.0)
        {
            parts.push(format!("{label}={}", compact_float(value)));
        }
    }
    for (label, keys) in [
        (
            "pad",
            ["paddingTop", "paddingRight", "paddingBottom", "paddingLeft"],
        ),
        (
            "mar",
            ["marginTop", "marginRight", "marginBottom", "marginLeft"],
        ),
    ] {
        let values = keys.map(|key| style_float_value(style, key).unwrap_or(0.0));
        if values.iter().any(|value| *value != 0.0) {
            parts.push(format!(
                "{label}={},{},{},{}",
                compact_float(values[0]),
                compact_float(values[1]),
                compact_float(values[2]),
                compact_float(values[3])
            ));
        }
    }
    if let Some(value) = style_int_value(style, "flexDirection").filter(|value| *value != 0) {
        parts.push(format!("flexDir={}", flex_direction_name(value)));
    }
    if let Some(value) = style_int_value(style, "flexWrap").filter(|value| *value != 0) {
        parts.push(format!("flexWrap={}", flex_wrap_name(value)));
    }
    if let Some(value) = style_float_value(style, "flexGrow").filter(|value| *value > 0.0) {
        parts.push(format!("grow={}", compact_float(value)));
    }
    if let Some(value) =
        style_float_value(style, "flexShrink").filter(|value| (*value - 1.0).abs() > 0.0001)
    {
        parts.push(format!("shrink={}", compact_float(value)));
    }
    if let Some(value) = style_int_value(style, "alignItems").filter(|value| *value != 4) {
        parts.push(format!("alignItems={}", align_name(value)));
    }
    if let Some(value) = style_int_value(style, "alignSelf").filter(|value| *value != 0) {
        parts.push(format!("alignSelf={}", align_name(value)));
    }
    if let Some(value) = style_int_value(style, "justifyContent").filter(|value| *value != 0) {
        parts.push(format!("justify={}", justify_name(value)));
    }
    parts
}

fn custom_string<'a>(custom: &'a Value, name: &str) -> Option<&'a str> {
    custom.get(name).and_then(Value::as_str)
}

fn push_quoted_detail(parts: &mut Vec<String>, custom: &Value, property: &str, label: &str) {
    if let Some(value) = custom_string(custom, property).filter(|value| !value.is_empty()) {
        parts.push(format!("{label}=\"{}\"", legacy_text(value, 500)));
    }
}

fn legacy_custom_details(type_name: &str, custom: &Value) -> Vec<String> {
    let mut parts = Vec::new();
    match type_name {
        "WindowFigma" => {
            push_quoted_detail(&mut parts, custom, "title", "title");
            push_quoted_detail(&mut parts, custom, "subtitle", "subtitle");
            push_quoted_detail(&mut parts, custom, "imgUrl", "imgUrl");
            for (property, label) in [
                ("isModal", "isModal"),
                ("showCloseButton", "hasClose"),
                ("hideTitleBar", "hideTitleBar"),
                ("isMovable", "isMovable"),
            ] {
                if custom.get(property).and_then(Value::as_bool) == Some(true) {
                    parts.push(label.to_owned());
                }
            }
            if let Some(value) = custom.get("headerStyle").and_then(Value::as_i64) {
                parts.push(format!("headerStyle={value}"));
            }
        }
        "DofusButtonCustom" => {
            push_quoted_detail(&mut parts, custom, "text", "text");
            for (property, label) in [
                ("mainStyle", "main"),
                ("status", "status"),
                ("size", "size"),
            ] {
                if let Some(value) = custom.get(property).and_then(Value::as_i64) {
                    parts.push(format!("{label}={value}"));
                }
            }
        }
        "Dropdown" => {
            push_quoted_detail(&mut parts, custom, "captionText", "caption");
            push_quoted_detail(&mut parts, custom, "choiceText", "selected");
            if let Some(value) = custom.get("dropdownType").and_then(Value::as_i64) {
                parts.push(format!("type={value}"));
            }
        }
        "DropdownButton" => {
            push_quoted_detail(&mut parts, custom, "buttonText", "buttonText");
            push_quoted_detail(&mut parts, custom, "buttonImageUrl", "buttonImageUrl");
        }
        "DofusToggleButtonGroupCustom" => {
            if let Some(value) = custom.get("selectedIndex").and_then(Value::as_i64) {
                parts.push(format!("selectedIdx={value}"));
            }
        }
        "DofusToggleButtonCustom" => {
            push_quoted_detail(&mut parts, custom, "text", "text");
            push_quoted_detail(&mut parts, custom, "imageUrl", "imageUrl");
        }
        _ => {}
    }
    parts
}

struct LegacyNode<'a> {
    depth: usize,
    type_name: &'a str,
    name: &'a str,
    tooltip: &'a str,
    text: Option<&'a str>,
    font_asset: Option<&'a FontAssetSummary>,
    style: &'a Value,
    custom: &'a Value,
}

fn legacy_node_line(node: LegacyNode<'_>) -> String {
    let short_type = short_type_name(node.type_name);
    let mut line = format!("  {}{short_type}", " ".repeat(node.depth * 2));
    if !node.name.is_empty() {
        line.push('#');
        line.push_str(node.name);
    }
    if let (Some(width), Some(height)) = (
        style_float_value(node.style, "width"),
        style_float_value(node.style, "height"),
    ) {
        line.push_str(&format!(" {}x{}", width as i32, height as i32));
    }
    if !node.tooltip.is_empty() {
        line.push_str(&format!(" tooltip=\"{}\"", legacy_text(node.tooltip, 200)));
    }
    if let Some(text) = node.text {
        line.push_str(&format!(" text=\"{}\"", legacy_text(text, 500)));
        if let Some(font) = node.font_asset {
            line.push_str(&format!(" font={}/{}", font.family_name, font.style_name));
        } else {
            line.push_str(" font=<no font>");
        }
        if let Some(value) = style_float_value(node.style, "fontSize") {
            line.push_str(&format!(" size={}px", compact_float(value)));
        }
        if let Some(value) = style_int_value(node.style, "fontStyleAndWeight") {
            line.push_str(&format!(" weight={}", font_weight_name(value)));
        }
        if let Some(value) = node.style.get("color").and_then(color_hex) {
            line.push_str(&format!(" color={value}"));
        }
        if let Some(value) = style_float_value(node.style, "letterSpacing") {
            line.push_str(&format!(" letterSpacing={}px", compact_float(value)));
        }
        if let Some(value) = style_float_value(node.style, "paragraphSpacing") {
            line.push_str(&format!(" paragraph={}px", compact_float(value)));
        }
        if let Some(value) =
            style_float_value(node.style, "outlineWidth").filter(|value| *value > 0.0)
        {
            line.push_str(&format!(" outline={}px", compact_float(value)));
        }
    } else {
        for detail in legacy_custom_details(short_type, node.custom) {
            line.push(' ');
            line.push_str(&detail);
        }
        if let Some(value) = node.style.get("backgroundColor").and_then(color_hex)
            && node.style["backgroundColor"]["a"].as_f64().unwrap_or(0.0) > 0.0
        {
            line.push_str(&format!(" bg={value}"));
        }
        if let Some(value) =
            style_float_value(node.style, "borderTopLeftRadius").filter(|value| *value > 0.0)
        {
            line.push_str(&format!(" radius={}px", compact_float(value)));
        }
        if let Some(value) =
            style_float_value(node.style, "borderTopWidth").filter(|value| *value > 0.0)
        {
            line.push_str(&format!(" border={}px", compact_float(value)));
        }
    }
    for detail in legacy_layout(node.style) {
        line.push(' ');
        line.push_str(&detail);
    }
    line
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

fn enqueue_node(runtime: Runtime, session: &mut UiSession, node: PendingNode) {
    if node.element.is_null()
        || node.depth > MAX_DEPTH
        || session.total_nodes + session.queue.len() >= MAX_NODES
        || !session.visited.insert(node.element as usize)
    {
        if session.total_nodes + session.queue.len() >= MAX_NODES {
            session.truncated = true;
        }
        return;
    }
    let gc_handle = runtime.gc_new(node.element, false);
    if gc_handle != 0 {
        session.queue.push(NodeJob {
            gc_handle,
            depth: node.depth,
            parent_path: node.parent_path,
            sibling_index: node.sibling_index,
            inside_popup: node.inside_popup,
            legacy_header: node.legacy_header,
        });
    }
}

fn inspect_node(runtime: Runtime, bindings: Bindings, session: &mut UiSession, job: NodeJob) {
    let element = runtime.gc_target(job.gc_handle);
    if element.is_null() {
        runtime.gc_free(job.gc_handle);
        return;
    }
    if let Some(header) = job.legacy_header.as_deref() {
        legacy_log(&mut session.legacy, header);
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
    let style = style_summary(runtime, bindings, element, true);
    let hidden = style_int_value(&style, "display") == Some(1)
        || (!job.inside_popup
            && style_float_value(&style, "width") == Some(0.0)
            && style_float_value(&style, "height") == Some(0.0));
    if hidden {
        session.skipped_hidden += 1;
        runtime.gc_free(job.gc_handle);
        return;
    }
    let now_inside_popup =
        job.inside_popup || short_type_name(&type_name).eq_ignore_ascii_case("WindowFigma");
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
    let custom = custom_properties(runtime, element, true);
    let legacy_line = legacy_node_line(LegacyNode {
        depth: job.depth,
        type_name: &type_name,
        name: &name,
        tooltip: &tooltip,
        text: is_text.then_some(text.as_str()),
        font_asset: font_asset.as_ref(),
        style: &style,
        custom: &custom,
    });
    legacy_log(&mut session.legacy, &legacy_line);
    let _ = session.legacy.flush();
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
    for index in (0..child_count).rev() {
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
        enqueue_node(
            runtime,
            session,
            PendingNode {
                element: child,
                depth: job.depth + 1,
                parent_path: path.clone(),
                sibling_index: index,
                inside_popup: now_inside_popup,
                legacy_header: None,
            },
        );
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
    let hotkey = if verbose { "F12" } else { "F11" };
    let legacy_path = state.legacy_path.clone();
    let tree_path = state.root.join(format!("ui-tree-{sequence}-{mode}.jsonl"));
    let texts_path = state.root.join(format!("texts-{sequence}-{mode}.csv"));
    let Ok(mut legacy) = append_writer(&legacy_path) else {
        state.runtime.log(
            DnhLogLevel::Error,
            "Création du log compatible DofusFontDumper impossible.",
        );
        return;
    };
    legacy_log(
        &mut legacy,
        if verbose {
            "--- F12 pressed: scheduling background dump (verbose: all properties) ---"
        } else {
            "--- F11 pressed: scheduling background dump (lightweight) ---"
        },
    );
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
    legacy_log(
        &mut legacy,
        &format!("  Found {} UIDocument instances.", documents.len()),
    );
    let mut panels = Vec::new();
    let mut session = UiSession {
        sequence,
        verbose,
        queue: Vec::new(),
        visited: BTreeSet::new(),
        legacy,
        tree,
        texts,
        total_nodes: 0,
        text_nodes: 0,
        skipped_hidden: 0,
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
        if root.is_null() {
            legacy_log(
                &mut session.legacy,
                &format!("    UIDocument \"{document_name}\" has no rootVisualElement."),
            );
            continue;
        }
        let root_child_count = invoke_value::<i32>(
            state.runtime,
            state.bindings.visual_get_child_count,
            root,
            &[],
        )
        .unwrap_or(0);
        let legacy_header = format!(
            "  --- UIDocument \"{document_name}\" (panel=\"{panel_name}\") childCount={root_child_count} ---"
        );
        panels.push(json!({
            "document": document_name,
            "panel": panel_name,
            "rootType": state.runtime.object_class_name(root),
        }));
        enqueue_node(
            state.runtime,
            &mut session,
            PendingNode {
                element: root,
                depth: 0,
                parent_path: format!("UIDocument#{document_name}"),
                sibling_index: index as i32,
                inside_popup: false,
                legacy_header: Some(legacy_header),
            },
        );
    }
    session.queue.reverse();
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
            legacy_path.display()
        ),
    );
    state.runtime.log(
        DnhLogLevel::Info,
        &format!("{hotkey}: format texte historique + JSON/CSV complémentaires."),
    );
    state.ui_session = Some(session);
}

fn process_ui_dump(state: &mut State) {
    let Some(mut session) = state.ui_session.take() else {
        return;
    };
    for _ in 0..NODES_PER_TICK {
        let Some(job) = session.queue.pop() else {
            break;
        };
        inspect_node(state.runtime, state.bindings, &mut session, job);
    }
    if session.queue.is_empty() {
        legacy_log(
            &mut session.legacy,
            &format!(
                "  --- dump complete (logged {} visible nodes, skipped {} hidden/0×0 subtrees) ---",
                session.total_nodes, session.skipped_hidden
            ),
        );
        let _ = session.legacy.flush();
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
            "skippedHidden": session.skipped_hidden,
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
    if runtime.v7().is_none() || runtime.v5_hooks().is_none() {
        runtime.log(
            DnhLogLevel::Error,
            "Dofus Interface Analyzer requiert l'ABI v8.",
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
    let legacy_path = match std::env::current_dir()
        .map(|game_root| game_root.join("UserData"))
        .map_err(|error| format!("dossier du jeu inaccessible : {error}"))
        .and_then(|user_data| {
            std::fs::create_dir_all(&user_data)
                .map_err(|error| {
                    format!("création de {} impossible : {error}", user_data.display())
                })
                .map(|()| user_data.join("dofus-font-dumper.log"))
        }) {
        Ok(path) => path,
        Err(error) => {
            runtime.log(DnhLogLevel::Error, &error);
            return DNH_ERROR;
        }
    };
    let mut initial_legacy_log = match File::create(&legacy_path).map(BufWriter::new) {
        Ok(writer) => writer,
        Err(error) => {
            runtime.log(
                DnhLogLevel::Error,
                &format!("création de {} impossible : {error}", legacy_path.display()),
            );
            return DNH_ERROR;
        }
    };
    legacy_log(
        &mut initial_legacy_log,
        &format!(
            "DofusInterfaceAnalyzer online — log path: {}",
            legacy_path.display()
        ),
    );
    let _ = initial_legacy_log.flush();
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
         UserData/dofus-font-dumper.log: format texte historique principal.\n\
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
        legacy_path: legacy_path.clone(),
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
            "Dofus Interface Analyzer prêt: F10 polices, F11 UI, F12 UI verbose. Log historique {}.",
            legacy_path.display()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_text_nodes_like_the_historical_dumper() {
        let style = json!({
            "width": 320.0,
            "height": 40.0,
            "display": 0,
            "visibility": 0,
            "opacity": 1.0,
            "color": {"r": 1.0, "g": 0.5, "b": 0.0, "a": 1.0},
            "fontSize": 16.0,
            "fontStyleAndWeight": 1,
            "letterSpacing": 0.5,
            "paragraphSpacing": 2.0,
            "outlineWidth": 1.0,
            "paddingTop": 4.0,
            "paddingRight": 8.0,
            "paddingBottom": 4.0,
            "paddingLeft": 8.0,
            "flexShrink": 1.0,
            "alignItems": 4,
            "alignSelf": 0,
            "justifyContent": 0
        });
        let font = FontAssetSummary {
            object_name: "Dofus-Regular SDF".to_owned(),
            family_name: "Dofus".to_owned(),
            style_name: "Regular".to_owned(),
            point_size: Some(90.0),
            line_height: Some(108.0),
            atlas_population_mode: Some(1),
            source_font: "Dofus-Regular".to_owned(),
            atlas_textures: Vec::new(),
        };
        let line = legacy_node_line(LegacyNode {
            depth: 1,
            type_name: "UnityEngine.UIElements.TextElement",
            name: "title",
            tooltip: "",
            text: Some("Bonjour\nDofus"),
            font_asset: Some(&font),
            style: &style,
            custom: &json!({}),
        });
        assert_eq!(
            line,
            "    TextElement#title 320x40 text=\"Bonjour\\nDofus\" font=Dofus/Regular size=16px weight=Bold color=#FF8000 letterSpacing=0.5px paragraph=2px outline=1px pad=4,8,4,8"
        );
    }

    #[test]
    fn formats_ankama_nodes_like_the_historical_dumper() {
        let style = json!({
            "width": 640.0,
            "height": 360.0,
            "display": 0,
            "visibility": 0,
            "opacity": 1.0,
            "backgroundColor": {"r": 0.0, "g": 0.0, "b": 0.0, "a": 0.5},
            "borderTopLeftRadius": 8.0,
            "borderTopWidth": 1.0,
            "position": 1,
            "top": 12.0,
            "flexShrink": 1.0,
            "alignItems": 2,
            "alignSelf": 0,
            "justifyContent": 1
        });
        let custom = json!({
            "title": "Confirmation",
            "isModal": true,
            "showCloseButton": true,
            "headerStyle": 2
        });
        let line = legacy_node_line(LegacyNode {
            depth: 0,
            type_name: "Core.UILogic.Components.Figma.WindowFigma",
            name: "popup",
            tooltip: "Fermer",
            text: None,
            font_asset: None,
            style: &style,
            custom: &custom,
        });
        assert_eq!(
            line,
            "  WindowFigma#popup 640x360 tooltip=\"Fermer\" title=\"Confirmation\" isModal hasClose headerStyle=2 bg=#00000080 radius=8px border=1px pos=Absolute top=12 alignItems=Center justify=Center"
        );
    }
}
