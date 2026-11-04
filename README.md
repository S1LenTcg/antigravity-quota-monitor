# Antigravity Quota Monitor (`antigravity-quota-monitor`)

`antigravity-quota-monitor` is a Windows 11 desktop utility built with Rust and Tauri v2 that embeds a live quota indicator directly inside the Windows Taskbar and displays current usage quotas, per-model limits, and reset timers for Google Antigravity CLI (`agy`) and Antigravity IDE.

---

## Why This Exists

Google Antigravity enforces rolling usage quotas across its supported AI model families — specifically a 5-hour rolling window (`5h`) and a 7-day rolling window (`weekly`), split between Gemini models (Gemini 4 Argon, Gemini 3.8 Flash) and third-party models (Claude Opus 5.5, Claude Sonnet 5.5).

Without a dedicated monitor, checking how much quota remains or when a depleted window resets requires interrupting your work to run `/usage` inside an active CLI session or opening IDE settings panels. When multiple terminal or editor sessions are running at the same time, it is easy to hit a rate limit unexpectedly in the middle of a task. This utility places a small two-line status indicator directly inside the left side of the Windows 11 Taskbar that alternates between 5-hour (`5h`) and weekly (`Wk`) quota figures every 10 seconds, and opens a detailed Flyout panel above the taskbar when hovered.

---

## How It Works

1. **Local Installation & Session Discovery:** On startup, the application checks whether Antigravity CLI or Antigravity IDE is installed on the machine and looks for an existing signed-in session across local storage locations:
   - The Windows Credential Manager entry `gemini:antigravity` (created by `agy`).
   - The Antigravity IDE state database at `%APPDATA%\Antigravity IDE\User\globalStorage\state.vscdb` (opened with Win32 `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE` so it can be read while Antigravity IDE has the SQLite database open).
   - The Antigravity ACP token file at `%USERPROFILE%\.gemini\antigravity-acp\acp_token.json`.
2. **Dynamic Token Refresh (Zero Bundled Secrets):** The application does not embed any Google OAuth `client_id` or `client_secret` in its source code or compiled executable, nor does it run a custom OAuth login web server. When a stored `access_token` expires, the backend reads the OAuth client identifier and secret directly from the user's locally installed `agy.exe` or `language_server_windows_x64.exe` binary on disk, exchanges the local `refresh_token` with `https://oauth2.googleapis.com/token`, and caches the resulting short-lived access token in memory.
3. **Quota & Model Retrieval:** Using the valid access token, the Rust backend queries the Antigravity quota endpoints (`loadCodeAssist`, `retrieveUserQuotaSummary`, and `fetchAvailableModels` on `https://daily-cloudcode-pa.googleapis.com` every 5 minutes, with local 60-second countdown ticks), checks active OS file locks on `.lock` files in `%USERPROFILE%\.gemini\antigravity\presence` and `%USERPROFILE%\.gemini\antigravity-cli\presence` (plus running IDE/CLI processes), and passes a structured snapshot to the Tauri WebView2 frontend.
4. **Two-Window Taskbar Widget + Hover Flyout Presentation:**
   - **Embedded Taskbar Pill (`pill` window, `124×38 px`):** Embedded directly inside the Windows 11 Taskbar (`Shell_TrayWnd`) as a Win32 child window (`WS_CHILD` via `SetParent`). It samples the taskbar pixel color via Win32 `GetPixel` to match the user's taskbar theme and alternates every 10 seconds between **5-Hour (`Flash 5h` / `Argon 5h`)** and **Weekly (`Flash Wk` / `Argon Wk`)** remaining or used quotas.
   - **Placement Over the Windows 11 Weather / Temperature Area:** By default, the pill is positioned at the far-left edge of the taskbar (`x = 8 px` scaled for DPI), which is the slot normally used by the Windows 11 Weather / Temperature ("Widgets") button. When running in its default position, this widget sits directly over that Weather / Temperature notification area (or occupies that empty left corner if Windows Widgets are disabled in Taskbar settings). If you prefer to keep the Windows Weather / Temperature button visible alongside the quota monitor, you can drag the quota pill horizontally along the taskbar with the left mouse button; its chosen horizontal offset is saved automatically.
   - **Hover Flyout Dashboard (`main` window, `368×440 px`):** Moving the mouse cursor over the taskbar pill opens a top-level Flyout window directly above the taskbar containing:
     - **Side-by-Side 5h & Weekly Summary Cards:** Shows both 5-hour (`5h`) and 7-day (`7d`) windows side-by-side for Gemini and Claude model groups with percentages, reset countdown timers (`⏱`), and progress bars.
     - **Per-Model Breakdown Grid:** Lists individual model quotas (`Gemini 3.8 Flash`, `Gemini 4 Argon`, `Claude Opus 5.5`, `Claude Sonnet 5.5`, etc.) in a 2-column grid directly below the summary cards without requiring tab switching.
     - **Header Controls & Info Drawer:** Includes a `Left %` / `Used %` mode toggle, 12-language selector, Windows Autostart toggle (`⚡`), collapsible connection/about info drawer (`ⓘ`), manual refresh button, and application quit button (`⏻`), plus an account switcher, active session counter, and window opacity slider in the footer.

---

## Technical Architecture and Details

### Stack and Crate Dependencies

- **Backend (`src/main.rs`):** Rust (2021 edition) running on `tokio` and `tauri` v2.
- **HTTP Client:** `reqwest` v0.12 configured with `rustls-tls` (no OpenSSL dependency) and a 12-second request timeout.
- **Credential & Taskbar Integration:** Direct Win32 FFI calls via `windows-sys` (`CredReadW`, `CredFree`) and `user32` / `gdi32` (`FindWindowW`, `FindWindowExW`, `SetParent`, `SetWindowLongW`, `SetWindowPos`, `GetPixel`) to read Windows Credential Manager entries and parent the `pill` window inside `Shell_TrayWnd`.
- **Frontend (`ui/pill.html`, `ui/index.html`, `ui/app.js`, `ui/styles.css`):** Vanilla HTML, CSS, and JavaScript communicating with the Rust backend over Tauri IPC. Zero frontend bundler or Node.js runtime dependencies are required.

### Project Structure

```text
.
├── Cargo.toml                 # Crate manifest, dependencies, and release LTO/strip profile
├── build.rs                   # Tauri build script and frontend asset change triggers
├── tauri.conf.json            # Tauri v2 two-window configuration ("pill" and "main")
├── installer.nsi              # NSIS Windows Setup installer script (per-user install + autostart)
├── capabilities/
│   └── default.json           # Tauri v2 ACL permissions for "pill" and "main" windows
├── icons/
│   ├── icon.ico               # Windows executable and installer icon
│   └── icon.png               # Application icon source
├── src/
│   └── main.rs                # Credential readers, binary OAuth scanner, API client, taskbar parenting, and autostart
└── ui/
    ├── pill.html              # Embedded 124x38 Windows 11 taskbar widget window
    ├── index.html             # 368x440 Hover Flyout DOM structure (data-i18n attributes)
    ├── app.js                 # Tauri IPC calls, i18n loader, 5-minute polling timer, and UI state logic
    ├── styles.css             # Flyout layout and theme rules
    └── locales/               # External JSON language packs (ET, EN-US, EN-GB, LV, LT, FI, DE, SV, PL, FR, ES, UK)
        ├── manifest.json
        ├── en-US.json
        ├── en-GB.json
        ├── et.json
        ├── lv.json
        ├── lt.json
        ├── fi.json
        ├── de.json
        ├── sv.json
        ├── pl.json
        ├── fr.json
        ├── es.json
        └── uk.json
```

### Key Internal Mechanisms in `src/main.rs`

- **`embed_pill_into_taskbar()` & `raise_pill_in_taskbar()`:** Locates `Shell_TrayWnd` and `TrayNotifyWnd`, converts the `pill` window style to `(style & !WS_POPUP) | WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS`, calls `SetParent(pill_hwnd, tray_hwnd)`, and positions the child window inside the taskbar coordinate space while clamping its horizontal range so it never overlaps the right-hand system tray (`TrayNotifyWnd`).
- **`load_stored_credential()`:** Checks `gemini:antigravity` in Windows Credential Manager first. If absent, `load_ide_vscdb_credential()` opens `%APPDATA%\Antigravity IDE\User\globalStorage\state.vscdb` with `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`, locates the `antigravityUnifiedStateSync.oauthToken` entry, decodes its outer and inner Base64 protobuf payloads, and extracts `ya29.*` access tokens and `1//*` refresh tokens. Falls back to `acp_token.json` if present.
- **`extract_oauth_clients_from_binary()` & `refresh_with_local_oauth_client()`:** Scans the byte slice of the locally installed `%LOCALAPPDATA%\agy\bin\agy.exe` or `%LOCALAPPDATA%\Programs\Antigravity-IDE\resources\app\extensions\antigravity\bin\language_server_windows_x64.exe` for `.apps.googleusercontent.com` client IDs and `GOCSPX-` secrets. Once a pair succeeds against `oauth2.googleapis.com/token`, it is cached in `AppState::cached_oauth_client` for the lifetime of the process.
- **`fetch_usage_internal()`:** Resolves the user's `cloudaicompanionProject` ID (from memory cache, `~/.gemini/antigravity-cli/cache/default_project_id.txt`, or `v1internal:loadCodeAssist`), then executes `v1internal:retrieveUserQuotaSummary` and `v1internal:fetchAvailableModels` concurrently via `tokio::join!`.
- **`set_autostart_sync()` & `ensure_installed_binary_sync()`:** Copies the running binary to `%LOCALAPPDATA%\Programs\AntigravityQuotaMonitor\antigravity-quota-monitor.exe` (if launched from another directory), creates a Start Menu shortcut at `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Antigravity Quota Monitor.lnk`, and manages the `AntigravityQuotaMonitor` value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.

---

## Installation, Auto-Start, and Setup

### Runtime Prerequisites

1. **Operating System:** Windows 10 or Windows 11 (x86_64) with Microsoft Edge WebView2 Runtime installed (included by default on Windows 11).
2. **Antigravity Installation:** You must have either **Google Antigravity CLI (`agy`)** or **Antigravity IDE** installed and signed in on your machine:
   - CLI binary path checked: `%LOCALAPPDATA%\agy\bin\agy.exe`
   - IDE binary path checked: `%LOCALAPPDATA%\Programs\Antigravity-IDE\Antigravity IDE.exe` (or `Antigravity IDE`)

### Option 1: Install via Prebuilt Windows Setup Installer (Recommended)

Download `antigravity-quota-monitor-windows-x64-setup.exe` from the [GitHub Releases](https://github.com/sidequest-chain/antigravity-quota-monitor/releases) page and run it. The installer requires no administrator elevation and performs the following steps automatically:

1. Installs the binary to `%LOCALAPPDATA%\Programs\AntigravityQuotaMonitor\antigravity-quota-monitor.exe`.
2. Creates a Start Menu shortcut (`Antigravity Quota Monitor`) and an uninstaller entry in Windows *Installed Apps*.
3. Registers `AntigravityQuotaMonitor` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` so the taskbar widget starts automatically whenever you sign in to Windows.
4. Launches the taskbar widget immediately upon completion.

### Option 2: Portable Binary & Self-Install Flags

If you download the standalone `antigravity-quota-monitor-windows-x64.exe` binary (or compile it from source), you can run it directly from any folder, toggle Windows auto-start using the `⚡` button in the Flyout header, or install/uninstall it from PowerShell:

```powershell
# Copy to %LOCALAPPDATA%\Programs\AntigravityQuotaMonitor, create Start Menu shortcut, enable Windows auto-start, and launch
.\antigravity-quota-monitor.exe --install

# Remove Windows auto-start registry entry and Start Menu shortcut
.\antigravity-quota-monitor.exe --uninstall
```

### Option 3: Building from Source

To compile the standalone release executable on Windows, you need the Rust toolchain (`rustup` + `cargo`) with the `x86_64-pc-windows-msvc` target and Visual Studio C++ Build Tools.

```powershell
# 1. Clone the repository
git clone https://github.com/sidequest-chain/antigravity-quota-monitor.git
cd antigravity-quota-monitor

# 2. Compile the optimized, stripped release binary
cargo build --release

# 3. Install into %LOCALAPPDATA%\Programs\AntigravityQuotaMonitor with Windows auto-start enabled
.\target\release\antigravity-quota-monitor.exe --install
```

The compiled executable (`target\release\antigravity-quota-monitor.exe`) is self-contained (the HTML/CSS/JS frontend for both windows and all 12 locale JSON files are embedded into the binary at compile time by Tauri).

### Automated GitHub Actions Builds & Releases

The repository includes an automated GitHub Actions pipeline (`.github/workflows/build.yml`) running on `windows-latest`:

- **On pushes to `main` or version tags (`v*`, e.g., `v0.2.0`):**
  1. Compiles the stripped Windows x64 portable executable (`antigravity-quota-monitor-windows-x64.exe`).
  2. Builds the NSIS Windows Setup installer (`antigravity-quota-monitor-windows-x64-setup.exe`) via `makensis installer.nsi`.
  3. Generates SHA-256 checksums (`SHA256SUMS.txt`) for both executables.
  4. Attaches all three files as workflow artifacts and publishes them to the corresponding GitHub Release.

---

## Current Scope and Limitations

- **Windows-Only Credential and Taskbar Integration:** Credential lookup uses Win32 `CredReadW` (`windows-sys`) and taskbar embedding targets the Windows `Shell_TrayWnd` window class. Linux and macOS are not supported.
- **Taskbar Left-Edge Weather / Widgets Slot:** Because the `pill` window is parented inside `Shell_TrayWnd` on the far-left side of the taskbar by default, it covers the area where Windows 11 normally renders the Weather / Temperature ("Widgets") button. Users who want both visible can either drag the quota pill to the right along the taskbar or disable the Windows Widgets button in *Settings → Personalization → Taskbar*.
- **Read-Only Session Consumer:** This utility never writes or modifies credentials in Windows Credential Manager or `state.vscdb`. Initial authentication and account switching must be performed inside the official Antigravity CLI (`agy`) or Antigravity IDE.
- **Internal API Dependency:** Quota data is retrieved from Google's `daily-cloudcode-pa.googleapis.com/v1internal:*` endpoints used by the Antigravity client. If Google changes the response schema of those internal endpoints in a future release, the parser in `fetch_usage_internal()` will require an update.
- **Externalized Multi-Language UI (`ui/locales/*.json`):** User-facing strings live in JSON translation files (`et`, `en-US`, `en-GB`, `lv`, `lt`, `fi`, `de`, `sv`, `pl`, `fr`, `es`, `uk`).

---

## Author, License & Non-Affiliation Disclaimer

- **Author:** [sidequest-chain](https://github.com/sidequest-chain)
- **Repository:** [github.com/sidequest-chain/antigravity-quota-monitor](https://github.com/sidequest-chain/antigravity-quota-monitor)
- **License:** Licensed under the **MIT License** (provided `"AS IS"`, without warranty of any kind) — see [LICENSE](LICENSE).

**Non-Affiliation Disclaimer:** This software is an independent, unofficial open-source companion utility and is **not** affiliated with, endorsed by, sponsored by, or supported by Google LLC or Anthropic PBC. It contains no proprietary keys or credentials and operates exclusively on the user's own local installation.
