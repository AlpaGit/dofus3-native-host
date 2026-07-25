use crate::marketplace::{MarketplaceCatalog, MarketplaceMod};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;
use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_WINDOW, CreateFontW, DEFAULT_CHARSET, DEFAULT_PITCH, FF_DONTCARE, FW_NORMAL, HFONT,
    OUT_DEFAULT_PRECIS,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, GetMessageW, HMENU, IDC_ARROW, IsWindowVisible, LB_ADDSTRING, LB_GETCURSEL,
    LB_RESETCONTENT, LBS_NOTIFY, LoadCursorW, MSG, PostMessageW, PostQuitMessage, RegisterClassW,
    SW_HIDE, SW_SHOW, SendMessageW, SetForegroundWindow, SetWindowTextW, ShowWindow,
    TranslateMessage, WM_APP, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_SETFONT, WNDCLASSW,
    WS_BORDER, WS_CAPTION, WS_CHILD, WS_CLIPCHILDREN, WS_EX_APPWINDOW, WS_MINIMIZEBOX,
    WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_THICKFRAME, WS_VISIBLE, WS_VSCROLL,
};

const WM_UI_EVENT: u32 = WM_APP + 0x41;
const ID_INSTALLED_LIST: usize = 1001;
const ID_ENABLE: usize = 1002;
const ID_DISABLE: usize = 1003;
const ID_RELOAD: usize = 1004;
const ID_MARKETPLACE_LIST: usize = 1101;
const ID_INSTALL: usize = 1102;
const ID_REFRESH: usize = 1103;
const ID_DESCRIPTION: usize = 1104;
const ID_STATUS: usize = 1201;
const LB_ERR: isize = -1;

static UI_HWND: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static UI_EVENTS: OnceLock<Mutex<Receiver<UiEvent>>> = OnceLock::new();
static UI_ACTIONS: OnceLock<Sender<UiAction>> = OnceLock::new();
static WINDOW_STATE: OnceLock<Mutex<WindowState>> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct InstalledModView {
    pub id: String,
    pub name: String,
    pub version: String,
    pub abi_version: u32,
    pub path: String,
    pub loaded: bool,
    pub disabled: bool,
}

#[derive(Clone, Debug, Default)]
pub struct UiSnapshot {
    pub installed: Vec<InstalledModView>,
}

#[derive(Clone, Debug)]
pub enum UiAction {
    Enable(InstalledModView),
    Disable(InstalledModView),
    Reload(InstalledModView),
    RefreshMarketplace,
    Install(MarketplaceMod),
}

enum UiEvent {
    Toggle(UiSnapshot),
    Snapshot(UiSnapshot),
    Catalog(Result<MarketplaceCatalog, String>),
    Status(String),
    Shutdown,
}

#[derive(Default)]
struct WindowState {
    installed: UiSnapshot,
    catalog: Option<MarketplaceCatalog>,
    installed_list: usize,
    marketplace_list: usize,
    description: usize,
    status: usize,
    font: usize,
}

pub struct UiController {
    events: Sender<UiEvent>,
    actions: Receiver<UiAction>,
    thread: Option<JoinHandle<()>>,
}

impl UiController {
    pub fn start() -> Result<Self, String> {
        let (event_sender, event_receiver) = channel();
        let (action_sender, action_receiver) = channel();
        UI_EVENTS
            .set(Mutex::new(event_receiver))
            .map_err(|_| "le gestionnaire natif est déjà initialisé".to_owned())?;
        UI_ACTIONS
            .set(action_sender)
            .map_err(|_| "les actions du gestionnaire sont déjà initialisées".to_owned())?;
        WINDOW_STATE
            .set(Mutex::new(WindowState::default()))
            .map_err(|_| "l'état du gestionnaire est déjà initialisé".to_owned())?;

        let thread = std::thread::Builder::new()
            .name("dofus-native-mod-manager".to_owned())
            .spawn(ui_thread)
            .map_err(|error| format!("création du thread UI impossible : {error}"))?;
        Ok(Self {
            events: event_sender,
            actions: action_receiver,
            thread: Some(thread),
        })
    }

    pub fn toggle(&self, snapshot: UiSnapshot) {
        self.send(UiEvent::Toggle(snapshot));
    }

    pub fn update_snapshot(&self, snapshot: UiSnapshot) {
        self.send(UiEvent::Snapshot(snapshot));
    }

    pub fn update_catalog(&self, result: Result<MarketplaceCatalog, String>) {
        self.send(UiEvent::Catalog(result));
    }

    pub fn set_status(&self, message: impl Into<String>) {
        self.send(UiEvent::Status(message.into()));
    }

    pub fn try_action(&self) -> Result<UiAction, TryRecvError> {
        self.actions.try_recv()
    }

    pub fn shutdown(mut self) {
        self.send(UiEvent::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    fn send(&self, event: UiEvent) {
        if self.events.send(event).is_ok() {
            let hwnd: HWND = UI_HWND.load(Ordering::Acquire).cast();
            if !hwnd.is_null() {
                unsafe {
                    PostMessageW(hwnd, WM_UI_EVENT, 0, 0);
                }
            }
        }
    }
}

fn ui_thread() {
    let instance = unsafe { GetModuleHandleW(null()) } as HINSTANCE;
    let class_name = wide("DofusNativeModManagerWindow");
    let title = wide("Dofus 3 — Gestionnaire de mods");
    let window_class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: null_mut(),
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        hbrBackground: (COLOR_WINDOW as usize + 1) as *mut c_void,
        lpszMenuName: null(),
        lpszClassName: class_name.as_ptr(),
    };
    if unsafe { RegisterClassW(&window_class) } == 0 {
        return;
    }

    let style =
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW,
            class_name.as_ptr(),
            title.as_ptr(),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            980,
            660,
            null_mut(),
            null_mut(),
            instance,
            null(),
        )
    };
    if hwnd.is_null() {
        return;
    }
    UI_HWND.store(hwnd.cast(), Ordering::Release);
    unsafe {
        PostMessageW(hwnd, WM_UI_EVENT, 0, 0);
    }

    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    UI_HWND.store(null_mut(), Ordering::Release);
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => {
            unsafe {
                create_controls(hwnd);
            }
            0
        }
        WM_COMMAND => {
            unsafe {
                handle_command(wparam & 0xffff);
            }
            0
        }
        WM_UI_EVENT => {
            unsafe {
                drain_events(hwnd);
            }
            0
        }
        WM_CLOSE => {
            unsafe {
                ShowWindow(hwnd, SW_HIDE);
            }
            0
        }
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

unsafe fn create_controls(hwnd: HWND) {
    let font_name = wide("Segoe UI");
    let font = unsafe {
        CreateFontW(
            -17,
            0,
            0,
            0,
            FW_NORMAL as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET.into(),
            OUT_DEFAULT_PRECIS.into(),
            0,
            5,
            (DEFAULT_PITCH | FF_DONTCARE).into(),
            font_name.as_ptr(),
        )
    };

    unsafe {
        create_control(
            hwnd,
            "STATIC",
            "GESTIONNAIRE NATIF",
            WS_CHILD | WS_VISIBLE,
            24,
            18,
            300,
            28,
            0,
            font,
        );
        create_control(
            hwnd,
            "STATIC",
            "F1 pour afficher ou masquer — les opérations sont exécutées sur le thread Unity",
            WS_CHILD | WS_VISIBLE,
            24,
            48,
            900,
            24,
            0,
            font,
        );
        create_control(
            hwnd,
            "STATIC",
            "Mods installés",
            WS_CHILD | WS_VISIBLE,
            24,
            90,
            430,
            24,
            0,
            font,
        );
        create_control(
            hwnd,
            "STATIC",
            "Marketplace GitHub",
            WS_CHILD | WS_VISIBLE,
            500,
            90,
            430,
            24,
            0,
            font,
        );
    }

    let installed_list = unsafe {
        create_control(
            hwnd,
            "LISTBOX",
            "",
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_VSCROLL | WS_TABSTOP | LBS_NOTIFY as u32,
            24,
            120,
            430,
            360,
            ID_INSTALLED_LIST,
            font,
        )
    };
    let marketplace_list = unsafe {
        create_control(
            hwnd,
            "LISTBOX",
            "",
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_VSCROLL | WS_TABSTOP | LBS_NOTIFY as u32,
            500,
            120,
            430,
            250,
            ID_MARKETPLACE_LIST,
            font,
        )
    };
    let description = unsafe {
        create_control(
            hwnd,
            "STATIC",
            "Charge le catalogue pour découvrir les mods disponibles.",
            WS_CHILD | WS_VISIBLE | WS_BORDER,
            500,
            382,
            430,
            98,
            ID_DESCRIPTION,
            font,
        )
    };

    unsafe {
        create_control(
            hwnd,
            "BUTTON",
            "Activer",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            24,
            498,
            130,
            36,
            ID_ENABLE,
            font,
        );
        create_control(
            hwnd,
            "BUTTON",
            "Désactiver",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            164,
            498,
            140,
            36,
            ID_DISABLE,
            font,
        );
        create_control(
            hwnd,
            "BUTTON",
            "Recharger",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            314,
            498,
            140,
            36,
            ID_RELOAD,
            font,
        );
        create_control(
            hwnd,
            "BUTTON",
            "Installer / Mettre à jour",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            500,
            498,
            260,
            36,
            ID_INSTALL,
            font,
        );
        create_control(
            hwnd,
            "BUTTON",
            "Actualiser",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            770,
            498,
            160,
            36,
            ID_REFRESH,
            font,
        );
    }
    let status = unsafe {
        create_control(
            hwnd,
            "STATIC",
            "Prêt.",
            WS_CHILD | WS_VISIBLE | WS_BORDER,
            24,
            558,
            906,
            34,
            ID_STATUS,
            font,
        )
    };

    if let Some(state) = WINDOW_STATE.get()
        && let Ok(mut state) = state.lock()
    {
        state.installed_list = installed_list as usize;
        state.marketplace_list = marketplace_list as usize;
        state.description = description as usize;
        state.status = status as usize;
        state.font = font as usize;
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn create_control(
    parent: HWND,
    class_name: &str,
    text: &str,
    style: u32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    id: usize,
    font: HFONT,
) -> HWND {
    let class_name = wide(class_name);
    let text = wide(text);
    let control = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            text.as_ptr(),
            style,
            x,
            y,
            width,
            height,
            parent,
            id as HMENU,
            GetModuleHandleW(null()) as HINSTANCE,
            null(),
        )
    };
    if !control.is_null() && !font.is_null() {
        unsafe {
            SendMessageW(control, WM_SETFONT, font as WPARAM, 1);
        }
    }
    control
}

unsafe fn drain_events(hwnd: HWND) {
    let Some(receiver) = UI_EVENTS.get() else {
        return;
    };
    let Ok(receiver) = receiver.lock() else {
        return;
    };
    while let Ok(event) = receiver.try_recv() {
        match event {
            UiEvent::Toggle(snapshot) => {
                unsafe {
                    update_snapshot(snapshot);
                }
                if unsafe { IsWindowVisible(hwnd) } != 0 {
                    unsafe {
                        ShowWindow(hwnd, SW_HIDE);
                    }
                } else {
                    unsafe {
                        ShowWindow(hwnd, SW_SHOW);
                        SetForegroundWindow(hwnd);
                    }
                    send_action(UiAction::RefreshMarketplace);
                }
            }
            UiEvent::Snapshot(snapshot) => unsafe {
                update_snapshot(snapshot);
            },
            UiEvent::Catalog(result) => unsafe {
                update_catalog(result);
            },
            UiEvent::Status(message) => unsafe {
                set_status(&message);
            },
            UiEvent::Shutdown => {
                unsafe {
                    DestroyWindow(hwnd);
                }
                break;
            }
        }
    }
}

unsafe fn handle_command(command: usize) {
    match command {
        ID_ENABLE => {
            if let Some(mod_view) = selected_installed() {
                send_action(UiAction::Enable(mod_view));
            }
        }
        ID_DISABLE => {
            if let Some(mod_view) = selected_installed() {
                send_action(UiAction::Disable(mod_view));
            }
        }
        ID_RELOAD => {
            if let Some(mod_view) = selected_installed() {
                send_action(UiAction::Reload(mod_view));
            }
        }
        ID_REFRESH => send_action(UiAction::RefreshMarketplace),
        ID_INSTALL => {
            if let Some(entry) = selected_marketplace() {
                send_action(UiAction::Install(entry));
            }
        }
        ID_MARKETPLACE_LIST => unsafe {
            update_description();
        },
        _ => {}
    }
}

fn send_action(action: UiAction) {
    if let Some(sender) = UI_ACTIONS.get() {
        let _ = sender.send(action);
    }
}

fn selected_installed() -> Option<InstalledModView> {
    let state = WINDOW_STATE.get()?.lock().ok()?;
    let index = unsafe { SendMessageW(state.installed_list as HWND, LB_GETCURSEL, 0, 0) } as isize;
    if index == LB_ERR {
        return None;
    }
    state.installed.installed.get(index as usize).cloned()
}

fn selected_marketplace() -> Option<MarketplaceMod> {
    let state = WINDOW_STATE.get()?.lock().ok()?;
    let index =
        unsafe { SendMessageW(state.marketplace_list as HWND, LB_GETCURSEL, 0, 0) } as isize;
    if index == LB_ERR {
        return None;
    }
    state.catalog.as_ref()?.mods.get(index as usize).cloned()
}

unsafe fn update_snapshot(snapshot: UiSnapshot) {
    let Some(state) = WINDOW_STATE.get() else {
        return;
    };
    let Ok(mut state) = state.lock() else {
        return;
    };
    state.installed = snapshot;
    let list = state.installed_list as HWND;
    unsafe {
        SendMessageW(list, LB_RESETCONTENT, 0, 0);
    }
    for entry in &state.installed.installed {
        let status = if entry.loaded {
            "ACTIF"
        } else if entry.disabled {
            "DÉSACTIVÉ"
        } else {
            "ARRÊTÉ"
        };
        let text = wide(&format!(
            "[{status}] {} {} · ABI v{}",
            entry.name, entry.version, entry.abi_version
        ));
        unsafe {
            SendMessageW(list, LB_ADDSTRING, 0, text.as_ptr() as LPARAM);
        }
    }
}

unsafe fn update_catalog(result: Result<MarketplaceCatalog, String>) {
    let Some(state) = WINDOW_STATE.get() else {
        return;
    };
    let Ok(mut state) = state.lock() else {
        return;
    };
    let list = state.marketplace_list as HWND;
    unsafe {
        SendMessageW(list, LB_RESETCONTENT, 0, 0);
    }
    match result {
        Ok(catalog) => {
            for entry in &catalog.mods {
                let text = wide(&format!(
                    "{} {} · {}",
                    entry.name, entry.version, entry.author
                ));
                unsafe {
                    SendMessageW(list, LB_ADDSTRING, 0, text.as_ptr() as LPARAM);
                }
            }
            let count = catalog.mods.len();
            state.catalog = Some(catalog);
            let status = state.status as HWND;
            let text = wide(&format!("{count} mod(s) disponible(s) sur la marketplace."));
            unsafe {
                SetWindowTextW(status, text.as_ptr());
            }
        }
        Err(error) => {
            state.catalog = None;
            let status = state.status as HWND;
            let text = wide(&error);
            unsafe {
                SetWindowTextW(status, text.as_ptr());
            }
        }
    }
}

unsafe fn update_description() {
    let Some(state) = WINDOW_STATE.get() else {
        return;
    };
    let Ok(state) = state.lock() else {
        return;
    };
    let Some(catalog) = state.catalog.as_ref() else {
        return;
    };
    let index =
        unsafe { SendMessageW(state.marketplace_list as HWND, LB_GETCURSEL, 0, 0) } as isize;
    let Some(entry) = (index != LB_ERR)
        .then_some(index as usize)
        .and_then(|index| catalog.mods.get(index))
    else {
        return;
    };
    let text = wide(&format!(
        "{}\r\n\r\nVersion {} · ABI v{}\r\n{}",
        entry.description, entry.version, entry.abi_version, entry.download_url
    ));
    unsafe {
        SetWindowTextW(state.description as HWND, text.as_ptr());
    }
}

unsafe fn set_status(message: &str) {
    let Some(state) = WINDOW_STATE.get() else {
        return;
    };
    let Ok(state) = state.lock() else {
        return;
    };
    let text = wide(message);
    unsafe {
        SetWindowTextW(state.status as HWND, text.as_ptr());
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
