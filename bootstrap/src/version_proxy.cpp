#include <Windows.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <cstdint>
#include <cstdio>
#include <cwchar>
#include <filesystem>
#include <string>
#include <system_error>

#pragma comment(lib, "User32.lib")

namespace {

constexpr wchar_t kDofusProcessName[] = L"Dofus.exe";
constexpr wchar_t kDofusDataName[] = L"Dofus_Data";
constexpr wchar_t kHarnessProcessName[] = L"DofusNativeHarness.exe";
constexpr wchar_t kHarnessDataName[] = L"DofusNativeHarness_Data";
constexpr wchar_t kHarnessSentinelName[] = L".bootstrap-enabled";
constexpr wchar_t kHostName[] = L"DofusNativeHost.dll";
constexpr wchar_t kGameAssemblyName[] = L"GameAssembly.dll";
constexpr wchar_t kModDirectoryName[] = L"NativeMods";
constexpr wchar_t kBootstrapLogName[] = L"native-bootstrap.log";
constexpr wchar_t kUnityWindowClass[] = L"UnityWndClass";
constexpr DWORD kTickIntervalMilliseconds = 16;

HMODULE g_our_module = nullptr;
HMODULE g_system_version = nullptr;
HMODULE g_host_module = nullptr;
INIT_ONCE g_version_once = INIT_ONCE_STATIC_INIT;
SRWLOCK g_error_lock = SRWLOCK_INIT;
std::array<char, 512> g_last_error{};
std::atomic_long g_state{0};
std::atomic_bool g_worker_started{false};
std::atomic_bool g_stop_requested{false};
DWORD g_unity_thread_id = 0;
HWND g_unity_window = nullptr;
HHOOK g_message_hook = nullptr;
LARGE_INTEGER g_tick_frequency{};
LARGE_INTEGER g_last_tick{};

using HostInitializeFn = std::int32_t(WINAPI*)(HMODULE, const wchar_t*);
using HostTickFn = void(WINAPI*)();
using HostShutdownFn = void(WINAPI*)();
using HostCopyLastErrorFn = std::size_t(WINAPI*)(char*, std::size_t);
using Il2CppDomainGetFn = void* (*)();

HostInitializeFn g_host_initialize = nullptr;
HostTickFn g_host_tick = nullptr;
HostShutdownFn g_host_shutdown = nullptr;

std::filesystem::path ModulePath(const HMODULE module) {
    std::array<wchar_t, 32768> buffer{};
    const DWORD length =
        GetModuleFileNameW(module, buffer.data(), static_cast<DWORD>(buffer.size()));
    if (length == 0 || length >= buffer.size()) {
        return {};
    }
    return std::filesystem::path(std::wstring_view(buffer.data(), length));
}

std::filesystem::path BootstrapDirectory() {
    const std::filesystem::path path = ModulePath(g_our_module);
    return path.empty() ? std::filesystem::path{} : path.parent_path();
}

void AppendBootstrapLog(const char* level, const char* message) {
    OutputDebugStringA("[DofusNativeBootstrap] ");
    OutputDebugStringA(message);
    OutputDebugStringA("\n");

    const std::filesystem::path directory = BootstrapDirectory() / kModDirectoryName;
    std::error_code error;
    std::filesystem::create_directories(directory, error);
    if (error) {
        return;
    }

    FILE* file = nullptr;
    if (_wfopen_s(&file, (directory / kBootstrapLogName).c_str(), L"ab") != 0 ||
        file == nullptr) {
        return;
    }

    SYSTEMTIME time{};
    GetLocalTime(&time);
    std::fprintf(
        file,
        "[%04u-%02u-%02u %02u:%02u:%02u.%03u] [%s] %s\r\n",
        static_cast<unsigned>(time.wYear),
        static_cast<unsigned>(time.wMonth),
        static_cast<unsigned>(time.wDay),
        static_cast<unsigned>(time.wHour),
        static_cast<unsigned>(time.wMinute),
        static_cast<unsigned>(time.wSecond),
        static_cast<unsigned>(time.wMilliseconds),
        level,
        message);
    std::fclose(file);
}

void SetBootstrapError(const char* message) {
    AcquireSRWLockExclusive(&g_error_lock);
    strncpy_s(g_last_error.data(), g_last_error.size(), message, _TRUNCATE);
    ReleaseSRWLockExclusive(&g_error_lock);
    AppendBootstrapLog("ERROR", message);
}

void SetBootstrapError(const char* operation, const DWORD error) {
    std::array<char, 512> message{};
    sprintf_s(
        message.data(),
        message.size(),
        "%s failed with Win32 error %lu",
        operation,
        static_cast<unsigned long>(error));
    SetBootstrapError(message.data());
}

bool IsDofusRuntime(const std::filesystem::path& directory) {
    const std::filesystem::path process_path = ModulePath(nullptr);
    return !process_path.empty() &&
           _wcsicmp(process_path.filename().c_str(), kDofusProcessName) == 0 &&
           std::filesystem::is_directory(directory / kDofusDataName);
}

bool IsHarnessRuntime(const std::filesystem::path& directory) {
    const std::filesystem::path process_path = ModulePath(nullptr);
    return !process_path.empty() &&
           _wcsicmp(process_path.filename().c_str(), kHarnessProcessName) == 0 &&
           std::filesystem::is_directory(directory / kHarnessDataName) &&
           std::filesystem::is_regular_file(directory / kHarnessSentinelName);
}

BOOL CALLBACK LoadSystemVersion(PINIT_ONCE, PVOID, PVOID*) {
    std::array<wchar_t, 32768> system_directory{};
    const UINT length =
        GetSystemDirectoryW(system_directory.data(), static_cast<UINT>(system_directory.size()));
    if (length == 0 || length >= system_directory.size()) {
        SetBootstrapError("GetSystemDirectoryW", GetLastError());
        return FALSE;
    }

    const std::filesystem::path version_path =
        std::filesystem::path(system_directory.data()) / L"version.dll";
    g_system_version = LoadLibraryExW(
        version_path.c_str(),
        nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (g_system_version == nullptr) {
        SetBootstrapError("LoadLibraryExW(system version.dll)", GetLastError());
        return FALSE;
    }
    return TRUE;
}

template <typename Function>
Function ResolveVersionExport(const char* name) {
    if (!InitOnceExecuteOnce(&g_version_once, LoadSystemVersion, nullptr, nullptr) ||
        g_system_version == nullptr) {
        SetLastError(ERROR_DLL_INIT_FAILED);
        return nullptr;
    }

    const auto address = GetProcAddress(g_system_version, name);
    if (address == nullptr) {
        SetBootstrapError(name, GetLastError());
        SetLastError(ERROR_PROC_NOT_FOUND);
        return nullptr;
    }
    return reinterpret_cast<Function>(address);
}

template <typename Function>
Function ResolveHostExport(const char* name) {
    const auto address = GetProcAddress(g_host_module, name);
    if (address == nullptr) {
        SetBootstrapError(name, GetLastError());
        return nullptr;
    }
    return reinterpret_cast<Function>(address);
}

bool IsIl2CppReady(HMODULE* game_assembly_output) {
    const HMODULE game_assembly = GetModuleHandleW(kGameAssemblyName);
    if (game_assembly == nullptr) {
        return false;
    }

    const auto domain_get = reinterpret_cast<Il2CppDomainGetFn>(
        GetProcAddress(game_assembly, "il2cpp_domain_get"));
    if (domain_get == nullptr || domain_get() == nullptr) {
        return false;
    }

    if (game_assembly_output != nullptr) {
        *game_assembly_output = game_assembly;
    }
    return true;
}

LONG InitializeHostOnUnityThread() {
    LONG expected = 0;
    if (!g_state.compare_exchange_strong(expected, 1)) {
        return expected == 2 ? 1 : expected;
    }

    const std::filesystem::path bootstrap_directory = BootstrapDirectory();
    if (bootstrap_directory.empty()) {
        SetBootstrapError("bootstrap directory could not be resolved");
        g_state.store(-2);
        return -2;
    }
    if (!IsDofusRuntime(bootstrap_directory) &&
        !IsHarnessRuntime(bootstrap_directory)) {
        SetBootstrapError("bootstrap refused an unsupported process or data directory");
        g_state.store(-2);
        return -2;
    }

    HMODULE game_assembly = nullptr;
    if (!IsIl2CppReady(&game_assembly)) {
        g_state.store(0);
        return -3;
    }

    const std::filesystem::path host_path = bootstrap_directory / kHostName;
    g_host_module = LoadLibraryExW(
        host_path.c_str(),
        nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (g_host_module == nullptr) {
        SetBootstrapError("LoadLibraryExW(DofusNativeHost.dll)", GetLastError());
        g_state.store(-4);
        return -4;
    }

    g_host_initialize = ResolveHostExport<HostInitializeFn>("DNH_Initialize");
    g_host_tick = ResolveHostExport<HostTickFn>("DNH_Tick");
    g_host_shutdown = ResolveHostExport<HostShutdownFn>("DNH_Shutdown");
    if (g_host_initialize == nullptr || g_host_tick == nullptr || g_host_shutdown == nullptr) {
        FreeLibrary(g_host_module);
        g_host_module = nullptr;
        g_state.store(-5);
        return -5;
    }

    const std::filesystem::path mod_directory =
        bootstrap_directory / kModDirectoryName;
    const std::int32_t result = g_host_initialize(game_assembly, mod_directory.c_str());
    if (result != 0 && result != 1) {
        std::array<char, 512> host_error{};
        const auto copy_error =
            ResolveHostExport<HostCopyLastErrorFn>("DNH_CopyLastError");
        if (copy_error != nullptr) {
            copy_error(host_error.data(), host_error.size());
        }
        if (host_error[0] != '\0') {
            SetBootstrapError(host_error.data());
        } else {
            SetBootstrapError("DNH_Initialize rejected bootstrap initialization");
        }
        FreeLibrary(g_host_module);
        g_host_module = nullptr;
        g_host_initialize = nullptr;
        g_host_tick = nullptr;
        g_host_shutdown = nullptr;
        g_state.store(-6);
        return -6;
    }

    g_unity_thread_id = GetCurrentThreadId();
    QueryPerformanceFrequency(&g_tick_frequency);
    QueryPerformanceCounter(&g_last_tick);
    g_state.store(2);
    AppendBootstrapLog("INFO", "standalone native host initialized on the Unity thread");
    return 0;
}

void TickHostOnUnityThread() {
    if (g_state.load() != 2 || g_host_tick == nullptr) {
        return;
    }
    if (GetCurrentThreadId() != g_unity_thread_id) {
        SetBootstrapError("native host tick refused a non-Unity thread");
        return;
    }

    LARGE_INTEGER now{};
    QueryPerformanceCounter(&now);
    if (g_tick_frequency.QuadPart > 0) {
        const LONGLONG elapsed = now.QuadPart - g_last_tick.QuadPart;
        const LONGLONG threshold =
            g_tick_frequency.QuadPart * kTickIntervalMilliseconds / 1000;
        if (elapsed < threshold) {
            return;
        }
    }
    g_last_tick = now;
    g_host_tick();
}

LRESULT CALLBACK UnityMessageHook(
    const int code,
    const WPARAM word_parameter,
    const LPARAM long_parameter) {
    if (code >= 0) {
        if (g_state.load() == 0) {
            InitializeHostOnUnityThread();
        }
        TickHostOnUnityThread();
    }
    return CallNextHookEx(g_message_hook, code, word_parameter, long_parameter);
}

struct WindowSearch {
    DWORD process_id;
    HWND exact;
    HWND fallback;
};

BOOL CALLBACK FindUnityWindowCallback(const HWND window, const LPARAM parameter) {
    auto* search = reinterpret_cast<WindowSearch*>(parameter);
    DWORD process_id = 0;
    GetWindowThreadProcessId(window, &process_id);
    if (process_id != search->process_id) {
        return TRUE;
    }

    std::array<wchar_t, 128> class_name{};
    GetClassNameW(window, class_name.data(), static_cast<int>(class_name.size()));
    if (_wcsicmp(class_name.data(), kUnityWindowClass) == 0) {
        search->exact = window;
        return FALSE;
    }

    if (search->fallback == nullptr &&
        IsWindowVisible(window) != FALSE &&
        GetWindow(window, GW_OWNER) == nullptr) {
        search->fallback = window;
    }
    return TRUE;
}

HWND FindUnityWindow() {
    WindowSearch search{GetCurrentProcessId(), nullptr, nullptr};
    EnumWindows(FindUnityWindowCallback, reinterpret_cast<LPARAM>(&search));
    return search.exact != nullptr ? search.exact : search.fallback;
}

DWORD WINAPI BootstrapWorker(LPVOID) {
    const std::filesystem::path directory = BootstrapDirectory();
    if (!IsDofusRuntime(directory)) {
        return 0;
    }

    AppendBootstrapLog("INFO", "standalone bootstrap worker started");

    while (!g_stop_requested.load()) {
        if (!IsIl2CppReady(nullptr)) {
            Sleep(100);
            continue;
        }

        g_unity_window = FindUnityWindow();
        if (g_unity_window == nullptr) {
            Sleep(100);
            continue;
        }

        g_unity_thread_id = GetWindowThreadProcessId(g_unity_window, nullptr);
        if (g_unity_thread_id == 0) {
            Sleep(100);
            continue;
        }

        g_message_hook = SetWindowsHookExW(
            WH_GETMESSAGE,
            UnityMessageHook,
            g_our_module,
            g_unity_thread_id);
        if (g_message_hook == nullptr) {
            SetBootstrapError("SetWindowsHookExW(WH_GETMESSAGE)", GetLastError());
            g_state.store(-7);
            return 0;
        }

        std::array<char, 160> message{};
        sprintf_s(
            message.data(),
            message.size(),
            "Unity message hook installed on thread %lu",
            static_cast<unsigned long>(g_unity_thread_id));
        AppendBootstrapLog("INFO", message.data());
        break;
    }

    while (!g_stop_requested.load() && g_message_hook != nullptr) {
        if (g_unity_window != nullptr && IsWindow(g_unity_window) != FALSE) {
            PostMessageW(g_unity_window, WM_NULL, 0, 0);
        } else {
            PostThreadMessageW(g_unity_thread_id, WM_NULL, 0, 0);
        }
        Sleep(kTickIntervalMilliseconds);
    }

    if (g_message_hook != nullptr) {
        UnhookWindowsHookEx(g_message_hook);
        g_message_hook = nullptr;
    }
    return 0;
}

void StartStandaloneWorker() {
    bool expected = false;
    if (!g_worker_started.compare_exchange_strong(expected, true)) {
        return;
    }

    const HANDLE thread = CreateThread(nullptr, 0, BootstrapWorker, nullptr, 0, nullptr);
    if (thread == nullptr) {
        g_worker_started.store(false);
        SetBootstrapError("CreateThread(BootstrapWorker)", GetLastError());
        return;
    }
    CloseHandle(thread);
}

}  // namespace

extern "C" __declspec(dllexport) LONG WINAPI DNB_NotifyUnityReady() {
    return InitializeHostOnUnityThread();
}

extern "C" __declspec(dllexport) void WINAPI DNB_Tick() {
    TickHostOnUnityThread();
}

extern "C" __declspec(dllexport) void WINAPI DNB_Shutdown() {
    LONG expected = 2;
    if (!g_state.compare_exchange_strong(expected, 3)) {
        return;
    }
    g_stop_requested.store(true);
    if (GetCurrentThreadId() == g_unity_thread_id && g_host_shutdown != nullptr) {
        g_host_shutdown();
    }
    if (g_host_module != nullptr) {
        FreeLibrary(g_host_module);
        g_host_module = nullptr;
    }
    g_host_initialize = nullptr;
    g_host_tick = nullptr;
    g_host_shutdown = nullptr;
    AppendBootstrapLog("INFO", "native host stopped");
}

extern "C" __declspec(dllexport) LONG WINAPI DNB_GetState() {
    return g_state.load();
}

extern "C" __declspec(dllexport) SIZE_T WINAPI DNB_CopyLastError(
    char* output,
    const SIZE_T capacity) {
    AcquireSRWLockShared(&g_error_lock);
    const SIZE_T length = strnlen_s(g_last_error.data(), g_last_error.size());
    if (output != nullptr && capacity != 0) {
        const SIZE_T count = (std::min)(length, capacity - 1);
        memcpy(output, g_last_error.data(), count);
        output[count] = '\0';
    }
    ReleaseSRWLockShared(&g_error_lock);
    return length;
}

#define VERSION_FORWARD(name, type, failure, parameters, arguments) \
    extern "C" type WINAPI Proxy_##name parameters {                 \
        using Function = type(WINAPI*) parameters;                   \
        const auto function = ResolveVersionExport<Function>(#name); \
        return function == nullptr ? failure : function arguments;   \
    }

VERSION_FORWARD(
    GetFileVersionInfoA,
    BOOL,
    FALSE,
    (LPCSTR filename, DWORD handle, DWORD length, LPVOID data),
    (filename, handle, length, data))
VERSION_FORWARD(
    GetFileVersionInfoW,
    BOOL,
    FALSE,
    (LPCWSTR filename, DWORD handle, DWORD length, LPVOID data),
    (filename, handle, length, data))
VERSION_FORWARD(
    GetFileVersionInfoExA,
    BOOL,
    FALSE,
    (DWORD flags, LPCSTR filename, DWORD handle, DWORD length, LPVOID data),
    (flags, filename, handle, length, data))
VERSION_FORWARD(
    GetFileVersionInfoExW,
    BOOL,
    FALSE,
    (DWORD flags, LPCWSTR filename, DWORD handle, DWORD length, LPVOID data),
    (flags, filename, handle, length, data))
VERSION_FORWARD(
    GetFileVersionInfoSizeA,
    DWORD,
    0,
    (LPCSTR filename, LPDWORD handle),
    (filename, handle))
VERSION_FORWARD(
    GetFileVersionInfoSizeW,
    DWORD,
    0,
    (LPCWSTR filename, LPDWORD handle),
    (filename, handle))
VERSION_FORWARD(
    GetFileVersionInfoSizeExA,
    DWORD,
    0,
    (DWORD flags, LPCSTR filename, LPDWORD handle),
    (flags, filename, handle))
VERSION_FORWARD(
    GetFileVersionInfoSizeExW,
    DWORD,
    0,
    (DWORD flags, LPCWSTR filename, LPDWORD handle),
    (flags, filename, handle))
VERSION_FORWARD(
    VerLanguageNameA,
    DWORD,
    0,
    (DWORD language, LPSTR buffer, DWORD capacity),
    (language, buffer, capacity))
VERSION_FORWARD(
    VerLanguageNameW,
    DWORD,
    0,
    (DWORD language, LPWSTR buffer, DWORD capacity),
    (language, buffer, capacity))
VERSION_FORWARD(
    VerQueryValueA,
    BOOL,
    FALSE,
    (LPCVOID block, LPCSTR sub_block, LPVOID* buffer, PUINT length),
    (block, sub_block, buffer, length))
VERSION_FORWARD(
    VerQueryValueW,
    BOOL,
    FALSE,
    (LPCVOID block, LPCWSTR sub_block, LPVOID* buffer, PUINT length),
    (block, sub_block, buffer, length))
VERSION_FORWARD(
    VerFindFileA,
    DWORD,
    0,
    (DWORD flags, LPCSTR filename, LPCSTR windows_directory, LPCSTR app_directory,
     LPSTR current_directory, PUINT current_length, LPSTR destination_directory,
     PUINT destination_length),
    (flags, filename, windows_directory, app_directory, current_directory, current_length,
     destination_directory, destination_length))
VERSION_FORWARD(
    VerFindFileW,
    DWORD,
    0,
    (DWORD flags, LPCWSTR filename, LPCWSTR windows_directory, LPCWSTR app_directory,
     LPWSTR current_directory, PUINT current_length, LPWSTR destination_directory,
     PUINT destination_length),
    (flags, filename, windows_directory, app_directory, current_directory, current_length,
     destination_directory, destination_length))
VERSION_FORWARD(
    VerInstallFileA,
    DWORD,
    0,
    (DWORD flags, LPCSTR source_filename, LPCSTR destination_filename,
     LPCSTR source_directory, LPCSTR destination_directory, LPCSTR current_directory,
     LPSTR temporary_file, PUINT temporary_length),
    (flags, source_filename, destination_filename, source_directory, destination_directory,
     current_directory, temporary_file, temporary_length))
VERSION_FORWARD(
    VerInstallFileW,
    DWORD,
    0,
    (DWORD flags, LPCWSTR source_filename, LPCWSTR destination_filename,
     LPCWSTR source_directory, LPCWSTR destination_directory, LPCWSTR current_directory,
     LPWSTR temporary_file, PUINT temporary_length),
    (flags, source_filename, destination_filename, source_directory, destination_directory,
     current_directory,
     temporary_file, temporary_length))

#undef VERSION_FORWARD

BOOL WINAPI DllMain(const HMODULE module, const DWORD reason, LPVOID) {
    if (reason == DLL_PROCESS_ATTACH) {
        g_our_module = module;
        DisableThreadLibraryCalls(module);
        StartStandaloneWorker();
    } else if (reason == DLL_PROCESS_DETACH) {
        g_stop_requested.store(true);
    }
    return TRUE;
}
