#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <shobjidl.h>
#include <shlobj.h>
#include <shlwapi.h>
#include <shellapi.h>
#include <strsafe.h>
#include <new>
#include <string>

namespace {
constexpr CLSID CLSID_OpenLocalServerCommands = {
    0x9f841d83, 0xa9cc, 0x4c75, {0x98, 0x0f, 0x3b, 0x4f, 0x6f, 0x5a, 0x11, 0x42}};
constexpr wchar_t kInstallKey[] = L"Software\\OpenLocalServer";
constexpr wchar_t kInstallValue[] = L"InstallLocation";

enum class Action : int { Root = 0, Add, Setup, IgnoreProject, IgnoreSite };

const wchar_t* title(Action action) {
    switch (action) {
    case Action::Root: return L"Open Local Server";
    case Action::Add: return L"Add folder to OLS projects";
    case Action::Setup: return L"Set up with OLS";
    case Action::IgnoreProject: return L"Ignore this project";
    case Action::IgnoreSite: return L"Ignore this site";
    }
    return L"Open Local Server";
}

std::wstring install_directory() {
    wchar_t buffer[32768] = {};
    DWORD size = sizeof(buffer);
    const LSTATUS status = RegGetValueW(
        HKEY_CURRENT_USER, kInstallKey, kInstallValue, RRF_RT_REG_SZ,
        nullptr, buffer, &size);
    if (status != ERROR_SUCCESS || buffer[0] == L'\0') return {};
    return buffer;
}

std::wstring quote_path(const wchar_t* path) {
    std::wstring quoted = L"\"";
    quoted += path;
    size_t trailing_slashes = 0;
    for (size_t i = quoted.size() - 1; i > 0 && quoted[i] == L'\\'; --i) {
        ++trailing_slashes;
    }
    quoted.append(trailing_slashes, L'\\');
    quoted += L'\"';
    return quoted;
}

HRESULT selected_path(IShellItemArray* items, IUnknown* site, std::wstring& path) {
    if (items) {
        DWORD count = 0;
        if (SUCCEEDED(items->GetCount(&count)) && count > 0) {
            IShellItem* item = nullptr;
            const HRESULT got = items->GetItemAt(0, &item);
            if (SUCCEEDED(got)) {
                PWSTR value = nullptr;
                const HRESULT named = item->GetDisplayName(SIGDN_FILESYSPATH, &value);
                item->Release();
                if (SUCCEEDED(named) && value) {
                    path.assign(value);
                    CoTaskMemFree(value);
                    return S_OK;
                }
                if (value) CoTaskMemFree(value);
            }
        }
    }

    if (!site) return HRESULT_FROM_WIN32(ERROR_PATH_NOT_FOUND);
    IServiceProvider* services = nullptr;
    HRESULT hr = site->QueryInterface(IID_PPV_ARGS(&services));
    if (FAILED(hr)) return hr;
    IShellBrowser* browser = nullptr;
    hr = services->QueryService(SID_STopLevelBrowser, IID_PPV_ARGS(&browser));
    services->Release();
    if (FAILED(hr)) return hr;
    IShellView* view = nullptr;
    hr = browser->QueryActiveShellView(&view);
    browser->Release();
    if (FAILED(hr)) return hr;
    IFolderView* folder_view = nullptr;
    hr = view->QueryInterface(IID_PPV_ARGS(&folder_view));
    view->Release();
    if (FAILED(hr)) return hr;
    IShellItem* folder = nullptr;
    hr = folder_view->GetFolder(IID_PPV_ARGS(&folder));
    folder_view->Release();
    if (FAILED(hr)) return hr;
    PWSTR value = nullptr;
    hr = folder->GetDisplayName(SIGDN_FILESYSPATH, &value);
    folder->Release();
    if (SUCCEEDED(hr) && value) {
        path.assign(value);
        CoTaskMemFree(value);
    } else if (value) {
        CoTaskMemFree(value);
    }
    return hr;
}

class Command final : public IExplorerCommand, public IObjectWithSite {
public:
    explicit Command(Action action, IUnknown* site = nullptr) : action_(action), site_(site) {
        if (site_) site_->AddRef();
    }

    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID iid, void** object) override {
        if (!object) return E_POINTER;
        *object = nullptr;
        if (iid == IID_IUnknown || iid == IID_IExplorerCommand) {
            *object = static_cast<IExplorerCommand*>(this);
        } else if (iid == IID_IObjectWithSite) {
            *object = static_cast<IObjectWithSite*>(this);
        } else {
            return E_NOINTERFACE;
        }
        AddRef();
        return S_OK;
    }

    ULONG STDMETHODCALLTYPE AddRef() override { return InterlockedIncrement(&refs_); }
    ULONG STDMETHODCALLTYPE Release() override {
        const ULONG remaining = InterlockedDecrement(&refs_);
        if (!remaining) delete this;
        return remaining;
    }

    HRESULT STDMETHODCALLTYPE GetTitle(IShellItemArray*, LPWSTR* value) override {
        return SHStrDupW(title(action_), value);
    }
    HRESULT STDMETHODCALLTYPE GetIcon(IShellItemArray*, LPWSTR* value) override {
        if (!value) return E_POINTER;
        *value = nullptr;
        const std::wstring directory = install_directory();
        if (directory.empty()) return E_NOTIMPL;
        const std::wstring icon = directory + L"\\OLS.exe,0";
        return SHStrDupW(icon.c_str(), value);
    }
    HRESULT STDMETHODCALLTYPE GetToolTip(IShellItemArray*, LPWSTR* value) override {
        if (value) *value = nullptr;
        return E_NOTIMPL;
    }
    HRESULT STDMETHODCALLTYPE GetCanonicalName(GUID* value) override {
        if (!value) return E_POINTER;
        *value = CLSID_OpenLocalServerCommands;
        value->Data4[7] = static_cast<BYTE>(value->Data4[7] + static_cast<int>(action_));
        return S_OK;
    }
    HRESULT STDMETHODCALLTYPE GetState(IShellItemArray*, BOOL, EXPCMDSTATE* state) override {
        if (!state) return E_POINTER;
        *state = ECS_ENABLED;
        return S_OK;
    }
    HRESULT STDMETHODCALLTYPE Invoke(IShellItemArray* items, IBindCtx*) override {
        if (action_ == Action::Root) return E_NOTIMPL;
        std::wstring path;
        HRESULT hr = selected_path(items, site_, path);
        if (FAILED(hr)) return hr;
        const std::wstring directory = install_directory();
        if (directory.empty()) return HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND);

        std::wstring executable;
        std::wstring arguments;
        if (action_ == Action::Add) {
            executable = directory + L"\\OLS.exe";
            arguments = L"--add-project " + quote_path(path.c_str());
        } else {
            executable = directory + L"\\olsc.exe";
            if (action_ == Action::Setup) {
                arguments = L"setup --path " + quote_path(path.c_str());
            } else if (action_ == Action::IgnoreProject) {
                arguments = L"project ignore " + quote_path(path.c_str());
            } else {
                arguments = L"project ignore-site " + quote_path(path.c_str());
            }
        }

        SHELLEXECUTEINFOW info = {};
        info.cbSize = sizeof(info);
        info.fMask = SEE_MASK_NOASYNC;
        info.lpVerb = L"open";
        info.lpFile = executable.c_str();
        info.lpParameters = arguments.c_str();
        info.lpDirectory = directory.c_str();
        info.nShow = SW_SHOWNORMAL;
        return ShellExecuteExW(&info) ? S_OK : HRESULT_FROM_WIN32(GetLastError());
    }
    HRESULT STDMETHODCALLTYPE GetFlags(EXPCMDFLAGS* flags) override {
        if (!flags) return E_POINTER;
        *flags = action_ == Action::Root ? ECF_HASSUBCOMMANDS : ECF_DEFAULT;
        return S_OK;
    }
    HRESULT STDMETHODCALLTYPE EnumSubCommands(IEnumExplorerCommand** commands) override;

    HRESULT STDMETHODCALLTYPE SetSite(IUnknown* site) override {
        if (site) site->AddRef();
        if (site_) site_->Release();
        site_ = site;
        return S_OK;
    }
    HRESULT STDMETHODCALLTYPE GetSite(REFIID iid, void** site) override {
        if (!site) return E_POINTER;
        *site = nullptr;
        return site_ ? site_->QueryInterface(iid, site) : E_FAIL;
    }

private:
    ~Command() { if (site_) site_->Release(); }
    volatile LONG refs_ = 1;
    Action action_;
    IUnknown* site_ = nullptr;
};

class CommandEnumerator final : public IEnumExplorerCommand {
public:
    explicit CommandEnumerator(IUnknown* site = nullptr, ULONG index = 0) : site_(site), index_(index) {
        if (site_) site_->AddRef();
    }
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID iid, void** object) override {
        if (!object) return E_POINTER;
        *object = nullptr;
        if (iid != IID_IUnknown && iid != IID_IEnumExplorerCommand) return E_NOINTERFACE;
        *object = static_cast<IEnumExplorerCommand*>(this);
        AddRef();
        return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return InterlockedIncrement(&refs_); }
    ULONG STDMETHODCALLTYPE Release() override {
        const ULONG remaining = InterlockedDecrement(&refs_);
        if (!remaining) delete this;
        return remaining;
    }
    HRESULT STDMETHODCALLTYPE Next(ULONG requested, IExplorerCommand** values, ULONG* fetched) override {
        if (!values || (requested != 1 && !fetched)) return E_POINTER;
        ULONG count = 0;
        while (count < requested && index_ < 4) {
            const Action action = static_cast<Action>(++index_);
            auto* command = new (std::nothrow) Command(action, site_);
            if (!command) {
                if (fetched) *fetched = count;
                return E_OUTOFMEMORY;
            }
            values[count++] = command;
        }
        if (fetched) *fetched = count;
        return count == requested ? S_OK : S_FALSE;
    }
    HRESULT STDMETHODCALLTYPE Skip(ULONG count) override {
        const ULONG remaining = 4 - index_;
        index_ += (count < remaining) ? count : remaining;
        return count <= remaining ? S_OK : S_FALSE;
    }
    HRESULT STDMETHODCALLTYPE Reset() override { index_ = 0; return S_OK; }
    HRESULT STDMETHODCALLTYPE Clone(IEnumExplorerCommand** value) override {
        if (!value) return E_POINTER;
        *value = new (std::nothrow) CommandEnumerator(site_, index_);
        return *value ? S_OK : E_OUTOFMEMORY;
    }
private:
    ~CommandEnumerator() { if (site_) site_->Release(); }
    volatile LONG refs_ = 1;
    IUnknown* site_ = nullptr;
    ULONG index_ = 0;
};

HRESULT Command::EnumSubCommands(IEnumExplorerCommand** commands) {
    if (!commands) return E_POINTER;
    *commands = nullptr;
    if (action_ != Action::Root) return E_NOTIMPL;
    auto* enumerator = new (std::nothrow) CommandEnumerator(site_);
    if (!enumerator) return E_OUTOFMEMORY;
    *commands = enumerator;
    return S_OK;
}

class ClassFactory final : public IClassFactory {
public:
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID iid, void** object) override {
        if (!object) return E_POINTER;
        *object = nullptr;
        if (iid != IID_IUnknown && iid != IID_IClassFactory) return E_NOINTERFACE;
        *object = static_cast<IClassFactory*>(this);
        AddRef();
        return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return InterlockedIncrement(&refs_); }
    ULONG STDMETHODCALLTYPE Release() override {
        const ULONG remaining = InterlockedDecrement(&refs_);
        if (!remaining) delete this;
        return remaining;
    }
    HRESULT STDMETHODCALLTYPE CreateInstance(IUnknown* outer, REFIID iid, void** object) override {
        if (outer) return CLASS_E_NOAGGREGATION;
        auto* command = new (std::nothrow) Command(Action::Root);
        if (!command) return E_OUTOFMEMORY;
        const HRESULT hr = command->QueryInterface(iid, object);
        command->Release();
        return hr;
    }
    HRESULT STDMETHODCALLTYPE LockServer(BOOL) override { return S_OK; }
private:
    volatile LONG refs_ = 1;
};
}

extern "C" HRESULT __stdcall DllGetClassObject(REFCLSID clsid, REFIID iid, void** object) {
    if (clsid != CLSID_OpenLocalServerCommands) return CLASS_E_CLASSNOTAVAILABLE;
    auto* factory = new (std::nothrow) ClassFactory();
    if (!factory) return E_OUTOFMEMORY;
    const HRESULT hr = factory->QueryInterface(iid, object);
    factory->Release();
    return hr;
}

extern "C" HRESULT __stdcall DllCanUnloadNow() { return S_FALSE; }

BOOL WINAPI DllMain(HINSTANCE, DWORD, LPVOID) { return TRUE; }
