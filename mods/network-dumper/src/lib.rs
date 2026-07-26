use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use dofus_native_mod_api::{
    DNH_ABI_VERSION_5, DNH_ERROR, DNH_MEMBER_STORAGE_INSTANCE, DNH_MEMBER_STORAGE_STATIC, DNH_OK,
    DnhClassInfoV2, DnhClassSignatureV3, DnhHandle, DnhHostApiV1, DnhLogLevel,
    DnhMethodSignatureV3, DnhModInfoV1,
};
use dofus_native_mod_sdk::Runtime;
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::{CStr, c_char};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::mem::size_of;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_QUEUED_PACKETS: usize = 10_000;
const MAX_FLUSH_PER_TICK: usize = 500;

static CAPTURE: Mutex<Option<CaptureRuntime>> = Mutex::new(None);
static OUTPUT: Mutex<Option<OutputState>> = Mutex::new(None);
static QUEUE: Mutex<VecDeque<PacketRecord>> = Mutex::new(VecDeque::new());
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
static DROPPED: AtomicU64 = AtomicU64::new(0);
static ACTIVE_CALLBACKS: AtomicUsize = AtomicUsize::new(0);
static UNLOADING: AtomicBool = AtomicBool::new(false);
static INCOMING_TARGET: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(null_mut());
static INCOMING_ORIGINAL: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(null_mut());
static OUTGOING_TARGET: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(null_mut());
static OUTGOING_ORIGINAL: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(null_mut());

static MOD_ID: &[u8] = b"bubble.dofus3.network-dumper\0";
static MOD_NAME: &[u8] = b"Dofus Network Dumper\0";
static MOD_VERSION: &[u8] = b"1.1.0\0";
static MOD_AUTHOR: &[u8] = b"Bubble\0";

struct SyncModInfo(DnhModInfoV1);
unsafe impl Sync for SyncModInfo {}

static MOD_INFO: SyncModInfo = SyncModInfo(DnhModInfoV1 {
    abi_version: DNH_ABI_VERSION_5,
    struct_size: size_of::<DnhModInfoV1>() as u32,
    id: MOD_ID.as_ptr().cast::<c_char>(),
    name: MOD_NAME.as_ptr().cast::<c_char>(),
    version: MOD_VERSION.as_ptr().cast::<c_char>(),
    author: MOD_AUTHOR.as_ptr().cast::<c_char>(),
});

#[derive(Clone, Copy)]
struct CaptureRuntime {
    runtime: Runtime,
    incoming_id: DnhHandle,
    incoming_message: DnhHandle,
    to_byte_array: DnhHandle,
}

unsafe impl Send for CaptureRuntime {}

struct OutputState {
    packets: BufWriter<File>,
    infinite_dream_json: BufWriter<File>,
    infinite_dream_csv: BufWriter<File>,
    packet_path: PathBuf,
    class_path: PathBuf,
    infinite_dream_path: PathBuf,
    classes: BTreeSet<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PacketRecord {
    sequence: u64,
    captured_at_unix_ms: u128,
    direction: &'static str,
    message_id: Option<i32>,
    class_name: String,
    payload_length: usize,
    raw_protobuf_base64: String,
    payload: Value,
    #[serde(skip)]
    infinite_dream_monsters: Vec<InfiniteDreamMonster>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InfiniteDreamMonster {
    sequence: u64,
    captured_at_unix_ms: u128,
    source_class: String,
    monster_id: u64,
    level: u64,
    entry_field_1: u64,
    entry_field_5: u64,
    stats: BTreeMap<u64, u64>,
}

type IncomingFn = unsafe extern "system" fn(DnhHandle, DnhHandle, DnhHandle, DnhHandle);
type OutgoingFn = unsafe extern "system" fn(DnhHandle, DnhHandle, DnhHandle);

struct CallbackGuard;

impl CallbackGuard {
    fn enter() -> Self {
        ACTIVE_CALLBACKS.fetch_add(1, Ordering::AcqRel);
        Self
    }
}

impl Drop for CallbackGuard {
    fn drop(&mut self) {
        ACTIVE_CALLBACKS.fetch_sub(1, Ordering::AcqRel);
    }
}

fn method_signature(
    name: *const c_char,
    return_type: *const c_char,
    parameters: &[*const c_char],
    storage: u8,
) -> DnhMethodSignatureV3 {
    DnhMethodSignatureV3 {
        struct_size: size_of::<DnhMethodSignatureV3>() as u32,
        name,
        return_type_name: return_type,
        parameter_count: parameters.len() as i32,
        parameter_type_names: if parameters.is_empty() {
            null()
        } else {
            parameters.as_ptr()
        },
        parameter_type_count: parameters.len() as u32,
        storage,
        reserved: [0; 3],
    }
}

fn unique_method(
    runtime: Runtime,
    class: DnhHandle,
    signature: &DnhMethodSignatureV3,
    label: &str,
) -> Option<DnhHandle> {
    let count =
        unsafe { (runtime.v3().find_methods_by_signature)(class, signature, null_mut(), 0) };
    if count == 0 {
        runtime.log(
            DnhLogLevel::Error,
            &format!("{label}: aucune méthode ne correspond à la signature."),
        );
        return None;
    }
    let mut matches = vec![null_mut(); count];
    unsafe {
        (runtime.v3().find_methods_by_signature)(
            class,
            signature,
            matches.as_mut_ptr(),
            matches.len(),
        );
    }
    if count == 1 {
        return matches.first().copied();
    }

    let all_count = unsafe { (runtime.v2().get_class_methods)(class, null_mut(), 0) };
    let mut all_methods = vec![null_mut(); all_count];
    if all_count != 0 {
        unsafe {
            (runtime.v2().get_class_methods)(class, all_methods.as_mut_ptr(), all_methods.len());
        }
    }
    let mut ranked = matches
        .into_iter()
        .filter_map(|method| {
            let address = unsafe { (runtime.v2().method_address)(method) };
            (!address.is_null()).then(|| {
                let reuse = all_methods
                    .iter()
                    .filter(|candidate| unsafe {
                        (runtime.v2().method_address)(**candidate) == address
                    })
                    .count();
                (reuse, method, address)
            })
        })
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(reuse, _, _)| *reuse);
    let selected = ranked.first().copied();
    if let Some((reuse, method, address)) = selected {
        runtime.log(
            DnhLogLevel::Warn,
            &format!(
                "{label}: {count} méthodes candidates, sélection structurelle de {address:p} (adresse réutilisée {reuse} fois dans la classe)."
            ),
        );
        Some(method)
    } else {
        runtime.log(
            DnhLogLevel::Error,
            &format!("{label}: toutes les méthodes candidates ont une adresse nulle."),
        );
        None
    }
}

fn class_name(runtime: Runtime, class: DnhHandle) -> String {
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
    if !unsafe { (runtime.v2().copy_class_info)(class, &mut info) } || info.name.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(info.name) }
        .to_string_lossy()
        .into_owned()
}

fn unique_class(
    runtime: Runtime,
    required_methods: &[DnhMethodSignatureV3],
    label: &str,
) -> Option<DnhHandle> {
    let signature = DnhClassSignatureV3 {
        struct_size: size_of::<DnhClassSignatureV3>() as u32,
        name: null(),
        namespace_name: null(),
        parent_name: null(),
        required_fields: null(),
        required_field_count: 0,
        required_methods: required_methods.as_ptr(),
        required_method_count: required_methods.len() as u32,
        minimum_field_count: 0,
        minimum_method_count: required_methods.len() as u32,
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
    let mut ranked = classes
        .into_iter()
        .filter(|class| !class_name(runtime, *class).contains('`'))
        .map(|class| {
            let score = required_methods
                .iter()
                .map(|method| unsafe {
                    (runtime.v3().find_methods_by_signature)(class, method, null_mut(), 0)
                })
                .sum::<usize>();
            (score, class)
        })
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    let Some((score, class)) = ranked.first().copied() else {
        runtime.log(
            DnhLogLevel::Error,
            &format!("{label}: aucune classe concrète ne correspond à la signature."),
        );
        return None;
    };
    if ranked
        .get(1)
        .is_some_and(|(next_score, _)| *next_score == score)
    {
        runtime.log(
            DnhLogLevel::Error,
            &format!("{label}: résolution ambiguë entre plusieurs classes (score {score})."),
        );
        return None;
    }
    if count > 1 {
        runtime.log(
            DnhLogLevel::Warn,
            &format!(
                "{label}: {count} classes candidates, sélection structurelle de {} (score {score}).",
                class_name(runtime, class)
            ),
        );
    }
    Some(class)
}

fn resolve_bindings(runtime: Runtime) -> Option<(CaptureRuntime, DnhHandle, DnhHandle)> {
    let incoming_parameters = [
        c"DotNetty.Transport.Channels.IChannelHandlerContext".as_ptr(),
        c"Core.Engine.Networking.Handlers.GameServerHandlers.GameMessage".as_ptr(),
    ];
    let incoming_signature = method_signature(
        c"ChannelRead0".as_ptr(),
        c"System.Void".as_ptr(),
        &incoming_parameters,
        DNH_MEMBER_STORAGE_INSTANCE,
    );
    let incoming_class = unique_class(runtime, &[incoming_signature], "handler entrant")?;
    let incoming_method = unique_method(
        runtime,
        incoming_class,
        &incoming_signature,
        "ChannelRead0 entrant",
    )?;

    let game_message = runtime.class(
        c"Core.dll",
        c"Core.Engine.Networking.Handlers.GameServerHandlers",
        c"GameMessage",
    );
    if game_message.is_null() {
        runtime.log(DnhLogLevel::Error, "GameMessage est introuvable.");
        return None;
    }
    let incoming_id_signature = method_signature(
        null(),
        c"System.Int32".as_ptr(),
        &[],
        DNH_MEMBER_STORAGE_INSTANCE,
    );
    let incoming_message_signature = method_signature(
        null(),
        c"Google.Protobuf.IMessage".as_ptr(),
        &[],
        DNH_MEMBER_STORAGE_INSTANCE,
    );
    let incoming_id = unique_method(
        runtime,
        game_message,
        &incoming_id_signature,
        "getter d'identifiant entrant",
    )?;
    let incoming_message = unique_method(
        runtime,
        game_message,
        &incoming_message_signature,
        "getter de message entrant",
    )?;

    let outgoing_message_parameters = [c"Google.Protobuf.IMessage".as_ptr()];
    let outgoing_wrap_signature = method_signature(
        null(),
        c"Core.Engine.Networking.Handlers.GameServerHandlers.GameRequest".as_ptr(),
        &outgoing_message_parameters,
        DNH_MEMBER_STORAGE_INSTANCE,
    );
    let outgoing_send_signature = method_signature(
        null(),
        c"System.Void".as_ptr(),
        &outgoing_message_parameters,
        DNH_MEMBER_STORAGE_INSTANCE,
    );
    let outgoing_class = unique_class(
        runtime,
        &[outgoing_wrap_signature, outgoing_send_signature],
        "service sortant",
    )?;
    let outgoing_method = unique_method(
        runtime,
        outgoing_class,
        &outgoing_send_signature,
        "envoi IMessage",
    )?;

    let extensions = runtime.class(
        c"Google.Protobuf.dll",
        c"Google.Protobuf",
        c"MessageExtensions",
    );
    if extensions.is_null() {
        runtime.log(
            DnhLogLevel::Error,
            "Google.Protobuf.MessageExtensions est introuvable.",
        );
        return None;
    }
    let serialize_parameters = [c"Google.Protobuf.IMessage".as_ptr()];
    let serialize_signature = method_signature(
        c"ToByteArray".as_ptr(),
        c"System.Byte[]".as_ptr(),
        &serialize_parameters,
        DNH_MEMBER_STORAGE_STATIC,
    );
    let to_byte_array = unique_method(
        runtime,
        extensions,
        &serialize_signature,
        "MessageExtensions.ToByteArray",
    )?;

    let incoming_target = unsafe { (runtime.v2().method_address)(incoming_method) };
    let outgoing_target = unsafe { (runtime.v2().method_address)(outgoing_method) };
    if incoming_target.is_null() || outgoing_target.is_null() {
        runtime.log(DnhLogLevel::Error, "Une adresse de hook réseau est nulle.");
        return None;
    }
    Some((
        CaptureRuntime {
            runtime,
            incoming_id,
            incoming_message,
            to_byte_array,
        },
        incoming_target,
        outgoing_target,
    ))
}

fn native_class_name(runtime: Runtime, object: DnhHandle) -> String {
    if object.is_null() {
        return "<null>".to_owned();
    }
    let class = unsafe { (runtime.v2().get_object_class)(object) };
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
    if !unsafe { (runtime.v2().copy_class_info)(class, &mut info) } || info.name.is_null() {
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

fn protobuf_bytes(capture: CaptureRuntime, message: DnhHandle) -> Option<Vec<u8>> {
    let array = capture
        .runtime
        .invoke(capture.to_byte_array, null_mut(), &[message])?;
    if array.is_null() {
        return None;
    }
    let length = unsafe { (capture.runtime.v2().array_length)(array) };
    let data = unsafe { (capture.runtime.v2().array_data)(array) };
    if length != 0 && data.is_null() {
        return None;
    }
    Some(if length == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(data.cast::<u8>(), length) }.to_vec()
    })
}

fn capture_packet(direction: &'static str, wrapper: DnhHandle, direct_message: DnhHandle) {
    if UNLOADING.load(Ordering::Acquire) {
        return;
    }
    let capture = CAPTURE.lock().ok().and_then(|state| *state);
    let Some(capture) = capture else {
        return;
    };
    let (message_id, message) = if direct_message.is_null() {
        let id = capture
            .runtime
            .invoke(capture.incoming_id, wrapper, &[])
            .and_then(|boxed| capture.runtime.unbox::<i32>(boxed));
        let message = capture
            .runtime
            .invoke(capture.incoming_message, wrapper, &[])
            .unwrap_or(null_mut());
        (id, message)
    } else {
        (None, direct_message)
    };
    if message.is_null() {
        return;
    }
    let class_name = native_class_name(capture.runtime, message);
    let Some(bytes) = protobuf_bytes(capture, message) else {
        capture.runtime.log(
            DnhLogLevel::Warn,
            &format!("Impossible de sérialiser le paquet {direction} {class_name}."),
        );
        return;
    };
    let payload = parse_wire_payload(&bytes).unwrap_or_else(|| {
        json!({
            "parseError": "protobuf wire payload could not be decoded"
        })
    });
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    let captured_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let infinite_dream_monsters = if direction == "incoming" {
        parse_infinite_dream_monsters(&bytes, sequence, captured_at_unix_ms, &class_name)
    } else {
        Vec::new()
    };
    let record = PacketRecord {
        sequence,
        captured_at_unix_ms,
        direction,
        message_id,
        class_name,
        payload_length: bytes.len(),
        raw_protobuf_base64: BASE64.encode(&bytes),
        payload,
        infinite_dream_monsters,
    };
    if let Ok(mut queue) = QUEUE.lock() {
        if queue.len() >= MAX_QUEUED_PACKETS {
            queue.pop_front();
            DROPPED.fetch_add(1, Ordering::Relaxed);
        }
        queue.push_back(record);
    }
}

unsafe extern "system" fn incoming_detour(
    this: DnhHandle,
    context: DnhHandle,
    message: DnhHandle,
    method_info: DnhHandle,
) {
    let _guard = CallbackGuard::enter();
    let _ = std::panic::catch_unwind(|| capture_packet("incoming", message, null_mut()));
    let original = INCOMING_ORIGINAL.load(Ordering::Acquire);
    if !original.is_null() {
        let original: IncomingFn = unsafe { std::mem::transmute(original) };
        unsafe { original(this, context, message, method_info) };
    }
}

unsafe extern "system" fn outgoing_detour(
    this: DnhHandle,
    message: DnhHandle,
    method_info: DnhHandle,
) {
    let _guard = CallbackGuard::enter();
    let _ = std::panic::catch_unwind(|| capture_packet("outgoing", null_mut(), message));
    let original = OUTGOING_ORIGINAL.load(Ordering::Acquire);
    if !original.is_null() {
        let original: OutgoingFn = unsafe { std::mem::transmute(original) };
        unsafe { original(this, message, method_info) };
    }
}

fn install_hook(
    runtime: Runtime,
    target: DnhHandle,
    detour: DnhHandle,
    target_slot: &AtomicPtr<std::ffi::c_void>,
    original_slot: &AtomicPtr<std::ffi::c_void>,
    label: &str,
) -> bool {
    let Some(original) = runtime.create_hook(target, detour) else {
        runtime.log(
            DnhLogLevel::Error,
            &format!("Création du hook {label} impossible."),
        );
        return false;
    };
    original_slot.store(original, Ordering::Release);
    target_slot.store(target, Ordering::Release);
    if !runtime.enable_hook(target) {
        runtime.remove_hook(target);
        original_slot.store(null_mut(), Ordering::Release);
        target_slot.store(null_mut(), Ordering::Release);
        runtime.log(
            DnhLogLevel::Error,
            &format!("Activation du hook {label} impossible."),
        );
        return false;
    }
    runtime.log(
        DnhLogLevel::Info,
        &format!("Hook {label} actif à {target:p}."),
    );
    true
}

fn remove_hooks(runtime: Runtime) {
    for (target_slot, original_slot) in [
        (&INCOMING_TARGET, &INCOMING_ORIGINAL),
        (&OUTGOING_TARGET, &OUTGOING_ORIGINAL),
    ] {
        let target = target_slot.swap(null_mut(), Ordering::AcqRel);
        if !target.is_null() {
            runtime.disable_hook(target);
            runtime.remove_hook(target);
        }
        original_slot.store(null_mut(), Ordering::Release);
    }
}

fn create_output() -> Result<OutputState, String> {
    let root = std::env::current_dir()
        .map_err(|error| format!("dossier courant inaccessible : {error}"))?
        .join("NativeMods")
        .join("DofusNetworkDump");
    std::fs::create_dir_all(&root)
        .map_err(|error| format!("création de {} impossible : {error}", root.display()))?;
    let session = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let packet_path = root.join(format!("packets-{session}.jsonl"));
    let class_path = root.join(format!("classes-{session}.json"));
    let infinite_dream_path = root.join(format!("infinite-dream-monsters-{session}.jsonl"));
    let infinite_dream_csv_path = root.join(format!("infinite-dream-monsters-{session}.csv"));
    let packets = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&packet_path)
        .map_err(|error| format!("création de {} impossible : {error}", packet_path.display()))?;
    let infinite_dream_json = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&infinite_dream_path)
        .map_err(|error| {
            format!(
                "création de {} impossible : {error}",
                infinite_dream_path.display()
            )
        })?;
    let infinite_dream_csv_file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&infinite_dream_csv_path)
        .map_err(|error| {
            format!(
                "création de {} impossible : {error}",
                infinite_dream_csv_path.display()
            )
        })?;
    let mut infinite_dream_csv = BufWriter::new(infinite_dream_csv_file);
    infinite_dream_csv
        .write_all(
            b"sequence,capturedAtUnixMs,sourceClass,monsterId,level,entryField1,entryField5,statsJson\n",
        )
        .map_err(|error| format!("initialisation CSV impossible : {error}"))?;
    Ok(OutputState {
        packets: BufWriter::new(packets),
        infinite_dream_json: BufWriter::new(infinite_dream_json),
        infinite_dream_csv,
        packet_path,
        class_path,
        infinite_dream_path,
        classes: BTreeSet::new(),
    })
}

fn drain_queue() {
    let mut records = Vec::new();
    if let Ok(mut queue) = QUEUE.lock() {
        for _ in 0..MAX_FLUSH_PER_TICK {
            let Some(record) = queue.pop_front() else {
                break;
            };
            records.push(record);
        }
    }
    if records.is_empty() {
        return;
    }
    let Ok(mut output) = OUTPUT.lock() else {
        return;
    };
    let Some(output) = output.as_mut() else {
        return;
    };
    let mut classes_changed = false;
    for record in records {
        classes_changed |= output.classes.insert(record.class_name.clone());
        for monster in &record.infinite_dream_monsters {
            if serde_json::to_writer(&mut output.infinite_dream_json, monster).is_ok() {
                let _ = output.infinite_dream_json.write_all(b"\n");
            }
            let stats = serde_json::to_string(&monster.stats).unwrap_or_else(|_| "{}".to_owned());
            let row = [
                monster.sequence.to_string(),
                monster.captured_at_unix_ms.to_string(),
                csv_cell(&monster.source_class),
                monster.monster_id.to_string(),
                monster.level.to_string(),
                monster.entry_field_1.to_string(),
                monster.entry_field_5.to_string(),
                csv_cell(&stats),
            ]
            .join(",");
            let _ = writeln!(output.infinite_dream_csv, "{row}");
        }
        if serde_json::to_writer(&mut output.packets, &record).is_ok() {
            let _ = output.packets.write_all(b"\n");
        }
    }
    let _ = output.packets.flush();
    let _ = output.infinite_dream_json.flush();
    let _ = output.infinite_dream_csv.flush();
    if classes_changed {
        let classes = output.classes.iter().collect::<Vec<_>>();
        if let Ok(json) = serde_json::to_vec_pretty(&classes) {
            let _ = std::fs::write(&output.class_path, json);
        }
    }
}

fn csv_cell(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn parse_infinite_dream_monsters(
    bytes: &[u8],
    sequence: u64,
    captured_at_unix_ms: u128,
    source_class: &str,
) -> Vec<InfiniteDreamMonster> {
    let mut position = 0;
    let mut monsters = Vec::new();
    while position < bytes.len() {
        let Some(key) = read_varint(bytes, &mut position, bytes.len()) else {
            return Vec::new();
        };
        let field = key >> 3;
        let wire_type = (key & 7) as u8;
        if field == 17 && wire_type == 2 {
            let Some(range) = read_length_delimited(bytes, &mut position, bytes.len()) else {
                return Vec::new();
            };
            if let Some(monster) = parse_infinite_dream_entry(
                &bytes[range],
                sequence,
                captured_at_unix_ms,
                source_class,
            ) {
                monsters.push(monster);
            }
        } else if !skip_wire_value(bytes, &mut position, bytes.len(), wire_type) {
            return Vec::new();
        }
    }
    monsters
}

fn parse_infinite_dream_entry(
    bytes: &[u8],
    sequence: u64,
    captured_at_unix_ms: u128,
    source_class: &str,
) -> Option<InfiniteDreamMonster> {
    let mut monster = InfiniteDreamMonster {
        sequence,
        captured_at_unix_ms,
        source_class: source_class.to_owned(),
        monster_id: 0,
        level: 0,
        entry_field_1: 0,
        entry_field_5: 0,
        stats: BTreeMap::new(),
    };
    let mut position = 0;
    while position < bytes.len() {
        let key = read_varint(bytes, &mut position, bytes.len())?;
        let field = key >> 3;
        let wire_type = (key & 7) as u8;
        if wire_type == 0 {
            let value = read_varint(bytes, &mut position, bytes.len())?;
            match field {
                1 => monster.entry_field_1 = value,
                2 => monster.monster_id = value,
                4 => monster.level = value,
                5 => monster.entry_field_5 = value,
                _ => {}
            }
        } else if field == 3 && wire_type == 2 {
            let range = read_length_delimited(bytes, &mut position, bytes.len())?;
            let entry = &bytes[range];
            let mut map_position = 0;
            let mut stat_id = 0;
            let mut stat_value = 0;
            while map_position < entry.len() {
                let map_key = read_varint(entry, &mut map_position, entry.len())?;
                let map_field = map_key >> 3;
                let map_wire_type = (map_key & 7) as u8;
                if map_wire_type == 0 {
                    let value = read_varint(entry, &mut map_position, entry.len())?;
                    if map_field == 1 {
                        stat_id = value;
                    } else if map_field == 2 {
                        stat_value = value;
                    }
                } else if !skip_wire_value(entry, &mut map_position, entry.len(), map_wire_type) {
                    return None;
                }
            }
            monster.stats.insert(stat_id, stat_value);
        } else if !skip_wire_value(bytes, &mut position, bytes.len(), wire_type) {
            return None;
        }
    }
    (monster.monster_id != 0 && !monster.stats.is_empty()).then_some(monster)
}

fn read_length_delimited(
    bytes: &[u8],
    position: &mut usize,
    end: usize,
) -> Option<std::ops::Range<usize>> {
    let length = usize::try_from(read_varint(bytes, position, end)?).ok()?;
    let value_end = position.checked_add(length)?;
    if value_end > end {
        return None;
    }
    let range = *position..value_end;
    *position = value_end;
    Some(range)
}

fn skip_wire_value(bytes: &[u8], position: &mut usize, end: usize, wire_type: u8) -> bool {
    match wire_type {
        0 => read_varint(bytes, position, end).is_some(),
        1 => {
            *position = (*position).saturating_add(8);
            *position <= end
        }
        2 => read_length_delimited(bytes, position, end).is_some(),
        5 => {
            *position = (*position).saturating_add(4);
            *position <= end
        }
        _ => false,
    }
}

fn parse_wire_payload(bytes: &[u8]) -> Option<Value> {
    let mut position = 0;
    let fields = parse_wire_fields(bytes, &mut position, bytes.len(), 0)?;
    (position == bytes.len()).then_some(json!({ "fields": fields }))
}

fn parse_wire_fields(
    bytes: &[u8],
    position: &mut usize,
    end: usize,
    depth: usize,
) -> Option<Vec<Value>> {
    if depth > 12 {
        return None;
    }
    let mut fields = Vec::new();
    while *position < end {
        let key = read_varint(bytes, position, end)?;
        let number = key >> 3;
        let wire_type = (key & 7) as u8;
        if number == 0 || number > 100_000 {
            return None;
        }
        let mut field = Map::new();
        field.insert("number".to_owned(), Value::from(number));
        field.insert("wireType".to_owned(), Value::from(wire_type));
        match wire_type {
            0 => {
                field.insert(
                    "value".to_owned(),
                    Value::from(read_varint(bytes, position, end)?),
                );
            }
            1 => {
                let value_end = position.checked_add(8)?;
                if value_end > end {
                    return None;
                }
                field.insert(
                    "fixed64Hex".to_owned(),
                    Value::String(hex(&bytes[*position..value_end])),
                );
                *position = value_end;
            }
            2 => {
                let length = read_varint(bytes, position, end)? as usize;
                let value_end = position.checked_add(length)?;
                if value_end > end {
                    return None;
                }
                let value = &bytes[*position..value_end];
                let mut child_position = *position;
                if !value.is_empty()
                    && let Some(child) =
                        parse_wire_fields(bytes, &mut child_position, value_end, depth + 1)
                    && child_position == value_end
                {
                    field.insert("message".to_owned(), json!({ "fields": child }));
                } else if let Ok(text) = std::str::from_utf8(value)
                    && text.chars().all(|character| {
                        !character.is_control() || matches!(character, '\r' | '\n' | '\t')
                    })
                {
                    field.insert("text".to_owned(), Value::String(text.to_owned()));
                } else {
                    field.insert("base64".to_owned(), Value::String(BASE64.encode(value)));
                }
                *position = value_end;
            }
            5 => {
                let value_end = position.checked_add(4)?;
                if value_end > end {
                    return None;
                }
                field.insert(
                    "fixed32Hex".to_owned(),
                    Value::String(hex(&bytes[*position..value_end])),
                );
                *position = value_end;
            }
            _ => return None,
        }
        fields.push(Value::Object(field));
    }
    Some(fields)
}

fn read_varint(bytes: &[u8], position: &mut usize, end: usize) -> Option<u64> {
    let mut value = 0_u64;
    let mut shift = 0_u32;
    while *position < end && shift < 70 {
        let current = bytes[*position];
        *position += 1;
        value |= u64::from(current & 0x7f) << shift;
        if current & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
    }
    None
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn runtime_guard() -> Option<MutexGuard<'static, Option<CaptureRuntime>>> {
    CAPTURE.lock().ok()
}

#[unsafe(no_mangle)]
pub extern "system" fn DNM_Query(host_abi_version: u32) -> *const DnhModInfoV1 {
    if host_abi_version == DNH_ABI_VERSION_5 {
        &MOD_INFO.0
    } else {
        null()
    }
}

#[unsafe(no_mangle)]
/// Initialise le dumper avec la table ABI fournie par le host.
///
/// # Safety
///
/// `host_api` doit pointer vers une table ABI v5 valide pendant toute la durée
/// de chargement du mod.
pub unsafe extern "system" fn DNM_Load(host_api: *const DnhHostApiV1) -> i32 {
    let Some(runtime) = (unsafe { Runtime::bind(host_api) }) else {
        return DNH_ERROR;
    };
    if runtime.v5_hooks().is_none() {
        runtime.log(
            DnhLogLevel::Error,
            "Dofus Network Dumper requiert l'ABI v5.",
        );
        return DNH_ERROR;
    }
    let Some((capture, incoming_target, outgoing_target)) = resolve_bindings(runtime) else {
        return DNH_ERROR;
    };
    let output = match create_output() {
        Ok(output) => output,
        Err(error) => {
            runtime.log(DnhLogLevel::Error, &error);
            return DNH_ERROR;
        }
    };
    runtime.log(
        DnhLogLevel::Info,
        &format!(
            "Dofus Network Dumper écrit les paquets dans {} et les monstres de songes dans {}",
            output.packet_path.display(),
            output.infinite_dream_path.display()
        ),
    );
    UNLOADING.store(false, Ordering::Release);
    if let Some(mut state) = runtime_guard() {
        *state = Some(capture);
    }
    if let Ok(mut state) = OUTPUT.lock() {
        *state = Some(output);
    }
    let incoming_installed = install_hook(
        runtime,
        incoming_target,
        incoming_detour as *const () as DnhHandle,
        &INCOMING_TARGET,
        &INCOMING_ORIGINAL,
        "entrant",
    );
    let outgoing_installed = install_hook(
        runtime,
        outgoing_target,
        outgoing_detour as *const () as DnhHandle,
        &OUTGOING_TARGET,
        &OUTGOING_ORIGINAL,
        "sortant",
    );
    if !incoming_installed || !outgoing_installed {
        remove_hooks(runtime);
        if let Some(mut state) = runtime_guard() {
            *state = None;
        }
        if let Ok(mut state) = OUTPUT.lock() {
            *state = None;
        }
        return DNH_ERROR;
    }
    runtime.log(
        DnhLogLevel::Warn,
        "La capture réseau est active. Les dumps peuvent contenir des données privées.",
    );
    DNH_OK
}

#[unsafe(no_mangle)]
pub extern "system" fn DNM_Tick() {
    drain_queue();
    let dropped = DROPPED.swap(0, Ordering::AcqRel);
    if dropped != 0
        && let Some(runtime) = CAPTURE
            .lock()
            .ok()
            .and_then(|state| state.map(|capture| capture.runtime))
    {
        runtime.log(
            DnhLogLevel::Warn,
            &format!("{dropped} paquet(s) réseau abandonné(s), file pleine."),
        );
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn DNM_Unload() {
    UNLOADING.store(true, Ordering::Release);
    let runtime = CAPTURE
        .lock()
        .ok()
        .and_then(|state| state.map(|capture| capture.runtime));
    if let Some(runtime) = runtime {
        remove_hooks(runtime);
    }
    let started = std::time::Instant::now();
    while ACTIVE_CALLBACKS.load(Ordering::Acquire) != 0
        && started.elapsed() < std::time::Duration::from_secs(2)
    {
        std::thread::yield_now();
    }
    drain_queue();
    if let Ok(mut state) = OUTPUT.lock() {
        if let Some(output) = state.as_mut() {
            let _ = output.packets.flush();
            let _ = output.infinite_dream_json.flush();
            let _ = output.infinite_dream_csv.flush();
        }
        *state = None;
    }
    if let Some(mut state) = runtime_guard() {
        *state = None;
    }
    if let Ok(mut queue) = QUEUE.lock() {
        queue.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_wire_payload() {
        let payload = [0x08, 0x96, 0x01, 0x12, 0x03, b'a', b'b', b'c'];
        let parsed = parse_wire_payload(&payload).expect("wire payload");
        assert_eq!(parsed["fields"][0]["number"], 1);
        assert_eq!(parsed["fields"][0]["value"], 150);
        assert_eq!(parsed["fields"][1]["text"], "abc");
    }

    #[test]
    fn rejects_invalid_wire_payload() {
        assert!(parse_wire_payload(&[0x0f]).is_none());
    }

    #[test]
    fn structurally_extracts_infinite_dream_monster() {
        // packet field 17 -> entry { field 2 monsterId, field 3 map, field 4 level }
        let payload = [
            0x8a, 0x01, 0x0b, 0x10, 0x2a, 0x1a, 0x04, 0x08, 0x05, 0x10, 0x63, 0x20, 0xc8, 0x01,
        ];
        let monsters = parse_infinite_dream_monsters(&payload, 7, 8, "obfuscated.Class");
        assert_eq!(monsters.len(), 1);
        assert_eq!(monsters[0].monster_id, 42);
        assert_eq!(monsters[0].level, 200);
        assert_eq!(monsters[0].stats.get(&5), Some(&99));
    }
}
