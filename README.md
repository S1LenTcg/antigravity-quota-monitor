# Antigravity Quota Monitor (`antigravity-quota-monitor`)

`antigravity-quota-monitor` is a lightweight, 100% native Windows 11 desktop utility built in pure Rust and Win32 GDI/DWM (~1.7 MB RAM working set, ~2.5 MB binary, zero WebView2/Chromium processes) that places a live quota indicator directly on the Windows Taskbar and displays current usage quotas, per-model limits, and reset timers for Google Antigravity CLI (`agy`) and Antigravity IDE.

---

## Why This Exists

Google Antigravity enforces rolling usage quotas across its supported AI model families — specifically a 5-hour rolling window (`5h`) and a 7-day rolling window (`weekly`), split between Gemini models (Gemini 4 Argon, Gemini 3.8 Flash) and third-party models (Claude Opus 5.5, Claude Sonnet 5.5).

Without a dedicated monitor, checking how much quota remains or when a depleted window resets requires interrupting your work to run `/usage` inside an active CLI session or opening IDE settings panels. When multiple terminal or editor sessions are running at the same time, it is easy to hit a rate limit unexpectedly in the middle of a task. This utility places a small two-line status indicator directly over the left side of the Windows 11 Taskbar that alternates between 5-hour (`5h`) and weekly (`Wk`) quota figures every 10 seconds, and opens a detailed Flyout panel above the taskbar when hovered.

---

## How It Works

1. **Local Installation & Session Discovery:** On startup, the application checks whether Antigravity CLI or Antigravity IDE is installed on the machine and looks for an existing signed-in session across local storage locations:
   - The Windows Credential Manager entry `gemini:antigravity` (created by `agy`).
   - The Antigravity IDE state database at `%APPDATA%\Antigravity IDE\User\globalStorage\state.vscdb` (opened with Win32 `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE` so it can be read while Antigravity IDE has the SQLite database open).
   - The Antigravity ACP token file at `%USERPROFILE%\.gemini\antigravity-acp\acp_token.json`.
2. **Dynamic Token Refresh (Zero Bundled Secrets):** The application does not embed any Google OAuth `client_id` or `client_secret` in its source code or compiled executable, nor does it run a custom OAuth login web server. When a stored `access_token` expires, the backend reads the OAuth client identifier and secret directly from the user's locally installed `agy.exe` or `language_server_windows_x64.exe` binary on disk, exchanges the local `refresh_token` with `https://oauth2.googleapis.com/token`, caches the resulting short-lived access token in memory, and immediately trims temporary startup pages from the process working set via `K32EmptyWorkingSet`.
3. **Quota & Model Retrieval:** Using the valid access token, a single-threaded background `tokio` worker queries the Antigravity quota endpoints (`loadCodeAssist`, `retrieveUserQuotaSummary`, and `fetchAvailableModels` on `https://daily-cloudcode-pa.googleapis.com` every 5 minutes, with local 60-second countdown ticks), checks active OS file locks on `.lock` files in `%USERPROFILE%\.gemini\antigravity\presence` and `%USERPROFILE%\.gemini\antigravity-cli\presence` (plus active IDE log/SHM locks), and notifies the native Win32 UI thread via `PostMessageW`.
4. **Pure Native Win32 Taskbar Widget + Hover Flyout Presentation (`~1.7 MB` RAM):**
   - **Taskbar Pill (`AGYPillNativeWnd`, `124×38 px` logical):** Rendered via a custom anti-aliased 32-bit `DIBSection` SDF rasterizer and ClearType `Segoe UI` fonts. Owned by `Shell_TrayWnd` (`GWLP_HWNDPARENT`) and composited by DWM directly above the taskbar's XAML layer, it samples the taskbar pixel color via Win32 `GetPixel` and alternates every 10 seconds between **5-Hour (`Flash 5h` / `Argon 5h`)** and **Weekly (`Flash Wk` / `Argon Wk`)** remaining or used quotas.
   - **Placement Over the Windows 11 Weather / Temperature Area:** By default, the pill is positioned at the far-left edge of the taskbar (`x = 8 px` scaled for DPI), which is the slot normally used by the Windows 11 Weather / Temperature ("Widgets") button. When running in its default position, this widget sits directly over that Weather / Temperature notification area (or occupies that empty left corner if Windows Widgets are disabled in Taskbar settings). If you prefer to keep the Windows Weather / Temperature button visible alongside the quota monitor, you can drag the quota pill horizontally along the taskbar with the left mouse button; its chosen horizontal offset is saved automatically.
   - **Hover Flyout Dashboard (`AGYFlyoutNativeWnd`, `368×440 px` logical):** Moving the mouse cursor over the taskbar pill opens a hardware-rounded DWM Flyout window directly above the taskbar containing:
     - **Side-by-Side 5h & Weekly Summary Cards:** Shows both 5-hour (`5h`) and 7-day (`7d`) windows side-by-side for Gemini and Claude model groups with percentages, reset countdown timers (`⏱`), and gradient progress bars.
     - **Per-Model Breakdown Grid:** Lists individual model quotas (`Gemini 3.8 Flash`, `Gemini 4 Argon`, `Claude Opus 5.5`, `Claude Sonnet 5.5`, etc.) in a scrollable 2-column grid directly below the summary cards.
     - **Header Controls & Info Drawer:** Includes a `Left %` / `Used %` mode toggle, 12-language native popup menu (`TrackPopupMenu`), Windows Autostart toggle (`⚡`), collapsible connection/about info drawer (`ⓘ`), manual refresh button (`↻`), and application quit button (`⏻`), plus an account switcher, active session counter, and interactive window opacity slider in the footer.

---

## Technical Architecture and Details

### Stack and Crate Dependencies

- **UI & Rendering (`src/ui_win32.rs`):** 100% native Win32 (`user32`, `gdi32`, `dwmapi`) with Per-Monitor DPI Awareness V2, 32-bit top-down `DIBSection` signed-distance-field (SDF) anti-aliased primitives, and ClearType `DrawTextW` typography. Zero WebView2, Chromium, or GPU browser processes.
- **Networking & Auth (`src/main.rs`):** Rust (2021 edition) with a single-threaded `tokio` runtime and `reqwest` v0.12 (`rustls-tls`, no OpenSSL dependency).
- **Zero External Win32 Crates:** Direct `extern "system"` FFI bindings to `user32`, `gdi32`, `dwmapi`, `advapi32`, and `kernel32`.

### Project Structure

```text
.
├── Cargo.toml                 # Crate manifest, dependencies, and release LTO/strip profile
├── installer.nsi              # NSIS Windows Setup installer script (per-user install + autostart)
├── icons/
│   └── icon.ico               # Windows installer and shortcut icon
├── src/
│   ├── main.rs                # Credential readers, binary OAuth scanner, API client, taskbar geometry, and autostart
│   └── ui_win32.rs            # Pure Win32 window classes, SDF 2D canvas, ClearType text, and interactive Flyout UI
└── locales/                   # Embedded JSON language packs (ET, EN-US, EN-GB, LV, LT, FI, DE, SV, PL, FR, ES, UK)
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

### Key Internal Mechanisms

- **`embed_pill_into_taskbar()` & `raise_pill_in_taskbar()` (`src/main.rs`):** Locates `Shell_TrayWnd` and `TrayNotifyWnd`, assigns `Shell_TrayWnd` as the owner (`GWLP_HWNDPARENT`) of the top-level `WS_POPUP | WS_EX_TOPMOST | WS_EX_LAYERED` pill window so DWM renders it above the Windows 11 XAML taskbar island, and clamps its horizontal range so it never overlaps the right-hand system tray (`TrayNotifyWnd`).
- **`Canvas` & SDF Primitives (`src/ui_win32.rs`):** Renders anti-aliased rounded rectangles, horizontal gradients, borders, and vector icons directly into a 32-bit ARGB memory bitmap and blits to the window DC in a single pass.
- **`load_all_stored_credentials()` (`src/main.rs`):** Checks `gemini:antigravity` in Windows Credential Manager (`CredReadW`) and `%APPDATA%\Antigravity IDE\User\globalStorage\state.vscdb` (opened with `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`), decoding protobuf/Base64 token entries and falling back to `acp_token.json` if present.
- **`extract_oauth_clients_from_binary()` & `refresh_with_local_oauth_client()` (`src/main.rs`):** Scans the locally installed `agy.exe` or `language_server_windows_x64.exe` binary for `.apps.googleusercontent.com` client IDs and `GOCSPX-` secrets, caches the working pair in memory, and calls `K32EmptyWorkingSet` immediately after scanning so file buffers are released back to the OS.
- **`set_autostart_sync()` (`src/main.rs`):** Copies the running binary to `%LOCALAPPDATA%\Programs\AntigravityQuotaMonitor\antigravity-quota-monitor.exe` (if launched from another directory), creates a Start Menu shortcut at `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Antigravity Quota Monitor.lnk`, and manages the `AntigravityQuotaMonitor` value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.

---

## Installation, Auto-Start, and Setup

### Runtime Prerequisites

1. **Operating System:** Windows 10 or Windows 11 (x86_64). No WebView2 or external runtime is required.
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

The compiled executable (`target\release\antigravity-quota-monitor.exe`, ~2.5 MB) is 100% self-contained with all 12 locale JSON files embedded at compile time.

### Automated GitHub Actions Builds & Releases

The repository includes an automated GitHub Actions pipeline (`.github/workflows/build.yml`) running on `windows-latest`:

- **On pushes to `main` or version tags (`v*`, e.g., `v0.3.0`):**
  1. Compiles the stripped Windows x64 portable executable (`antigravity-quota-monitor-windows-x64.exe`).
  2. Builds the NSIS Windows Setup installer (`antigravity-quota-monitor-windows-x64-setup.exe`) via `makensis installer.nsi`.
  3. Generates SHA-256 checksums (`SHA256SUMS.txt`) for both executables.
  4. Attaches all three files as workflow artifacts and publishes them to the corresponding GitHub Release.

---

## Current Scope and Limitations

- **Windows-Only Credential and Taskbar Integration:** Credential lookup uses Win32 `CredReadW` and taskbar integration targets the Windows `Shell_TrayWnd` window class. Linux and macOS are not supported.
- **Taskbar Left-Edge Weather / Widgets Slot:** Because the `pill` window is positioned over `Shell_TrayWnd` on the far-left side of the taskbar by default, it covers the area where Windows 11 normally renders the Weather / Temperature ("Widgets") button. Users who want both visible can either drag the quota pill to the right along the taskbar or disable the Windows Widgets button in *Settings → Personalization → Taskbar*.
- **Read-Only Session Consumer:** This utility never writes or modifies credentials in Windows Credential Manager or `state.vscdb`. Initial authentication and account switching must be performed inside the official Antigravity CLI (`agy`) or Antigravity IDE.
- **Internal API Dependency:** Quota data is retrieved from Google's `daily-cloudcode-pa.googleapis.com/v1internal:*` endpoints used by the Antigravity client. If Google changes the response schema of those internal endpoints in a future release, the parser in `fetch_usage_internal()` will require an update.
- **Multi-Language UI (`locales/*.json`):** User-facing strings live in JSON translation files (`et`, `en-US`, `en-GB`, `lv`, `lt`, `fi`, `de`, `sv`, `pl`, `fr`, `es`, `uk`).

---

## Author, License & Non-Affiliation Disclaimer

- **Author:** [sidequest-chain](https://github.com/sidequest-chain)
- **Repository:** [github.com/sidequest-chain/antigravity-quota-monitor](https://github.com/sidequest-chain/antigravity-quota-monitor)
- **License:** Licensed under the **MIT License** (provided `"AS IS"`, without warranty of any kind) — see [LICENSE](LICENSE).

**Non-Affiliation Disclaimer:** This software is an independent, unofficial open-source companion utility and is **not** affiliated with, endorsed by, sponsored by, or supported by Google LLC or Anthropic PBC. It contains no proprietary keys or credentials and operates exclusively on the user's own local installation.
