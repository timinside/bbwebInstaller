<p align="center">
  <img src="assets/icon.png" width="100" height="100" alt="bbweb Installer icon">
</p>

# bbweb Installer

Lightweight Win32 web installer for bbweb projects (Rust, single `.exe`, no persistent background service/tray process in normal use).

## Features

- Loads the project list and install parameters from a remote JSON manifest (`MANIFEST_URL` in `src/main.rs`). No offline project list is hardcoded.
- A local `manifest.json` next to the exe takes priority over the remote manifest; `--manifest-url <url|path>` overrides both.
- Installs projects with per-project arguments from the manifest (typically `/SILENT /NORESTART`), shows download progress (curl) and status in the window.
- Checks for available updates for installed programs and for bbweb Installer itself; **Install / Update all / Update manifest** actions.
- Self-update: downloads the new version, replaces the exe, and restarts with the same arguments.
- Single GUI instance: if the app is already running (including minimized), launching it again activates the existing window and the new process exits.

## Manifest

Format: JSON with `installer` and `projects[]` objects.

- `installer.version`, `installer.download_url` — bbweb Installer version and download URL for self-update.
- `projects[].id`, `name`, `version`, `url`, `file`, `args` — project identifier, name, version/version source, installer URL, file name, and installer arguments.
- `projects[].installed_version_path` — path to a file containing the installed version (used for update checks).
- `projects[].latest_version_url` — URL of a file containing the latest version (when `version` is not a version URL).
- `version` can be a literal version string or an `http(s)` URL pointing to a version file (preferred latest-version source).

## Build

Requires a Rust toolchain (`cargo`, `rustc`, Windows target).

```bash
cargo build --release
```

Output: `target\release\bbwebinstaller.exe` (release profile: `lto`, `strip`, `opt-level = "z"`).

## Usage

### GUI

- Run `bbwebinstaller.exe` with no arguments to open the installer UI (requires access to the manifest: internet or a local `manifest.json`).
- Select a project and click **Install**. Download progress and installer execution are shown in the log area.
- **Update all** — updates programs with available updates, then bbweb Installer if needed.
- **Update manifest** — reloads the manifest and refreshes the list/status.

### Command-line flags

| Flag | Description |
| --- | --- |
| `--manifest-url <url\|path>` | Override manifest source (URL or local path). |
| `--logs` | Write logs to `C:\Users\<user>\.bbweb\logs\bbwebinstaller.log`. |
| `--nocheck` | Suppress update notifications. |
| `--installed` | Installed-apps context (scan/update/service flows). |
| `--service` | Logon check mode (with `--installed`): shows only a balloon notification if updates exist and exits; if no updates, exits immediately. No window is shown. |
| `--check-update <id\|name>` | (with `--installed`) Check for an update for a specific project. Exit codes: `100` update available, `0` no update, `3` not installed (present in manifest), `2` not found. |
| `--update-product <id\|name>` | (with `--installed`) Download and install only the specified project. Exit codes: `0` success, `1` error, `2` not found. |
| `--update-all` | (with `--installed`) Update all projects with available updates (and bbweb Installer if needed). |
| `--product-check <id\|name>` | (with `--installed`) Show an info dialog with installed/latest version for a matching app. |
| `--help`, `-h`, `/?` | Show help. |

Examples:

```text
bbwebinstaller.exe
bbwebinstaller.exe --installed --service --logs
bbwebinstaller.exe --installed --check-update bbruntime --logs
bbwebinstaller.exe --installed --update-product bbruntime --logs
```

## Service mode behavior (`--installed --service`)

The scheduled task (Inno Setup, task `bbweb Installer Update Check`, trigger: user logon) runs:

```text
bbwebinstaller.exe --installed --service --logs
```

What a run does:

1. Loads the manifest and checks installed versions.
2. If bbweb Installer needs a self-update, performs it and restarts with the same arguments.
3. If there are no updates (or `--nocheck`) — **exits immediately** with no window and no tray icon.
4. If updates are available — shows **only a balloon notification** (no persistent tray icon). After the balloon is hidden/times out the process exits; clicking the balloon starts the update (the window is shown for progress).

In service mode the window is created hidden (no flash).

## Usage statistics and system impact

> No telemetry is built into the app. The section below describes verifiable behavior by design and how to collect actual data on a specific machine.

### How to collect actual statistics

- Enable `--logs`: events are written to `C:\Users\<username>\.bbweb\logs\bbwebinstaller.log` (start, arguments, manifest load, scan results, notifications, update actions).
- Scheduled task history/state: `schtasks /Query /TN "bbweb Installer Update Check" /V /FO LIST` and the Task Scheduler log (Microsoft-Windows-TaskScheduler/Operational).
- Sizes: exe (~0.37 MB); the log grows only when started with `--logs` (one line per event; can be cleared/rotated manually).

### Why the logon check is useful

- Notifies about available updates for installed projects and for bbweb Installer itself (balloon when updates exist) without a persistent background process.
- Enables automation for individual apps via `--check-update` / `--update-product` (exit codes for scripts and third-party programs).

### Why it does not clutter the system

- **No persistent resident process:** runs on logon (ONLOGON), exits quickly when there are no updates; when updates exist, only a notification until it is dismissed or acted on.
- **No persistent tray icon in service mode:** the balloon is shown without leaving an icon after it disappears.
- **Minimal artifacts:** downloads go to the app's local/temp area (`%LOCALAPPDATA%\bbwebInstaller\downloads`); logs are created only with explicit `--logs` in a single file; no separate Windows Service is installed.
- **Network activity:** only during runs (manifest fetch/read and, when updating, installer downloads). There is no background activity without a run.

Limitations: on Windows 10/11 balloons may appear as system toasts and depend on notification settings; behavior should be verified in the target environment.

## Reuse / Fork

This project can be reused for your own purposes: fork the repository, change the server/manifest information (e.g. `MANIFEST_URL` and your hosted `manifest.json`), rename the program/branding as needed — and you're ready to go.

## Technical notes

- Win32 UI (`windows` crate), Segoe UI font, `icon.ico` embedded via `build.rs`/`winres`.
- Downloads use system `curl.exe` (hidden); installers run with arguments from the manifest.
- If the manifest is unavailable (and no local copy exists), the app shows a launch error and exits.
- On download failure (including 404), status/log report the error and any partial file is removed.
- `--product-check`, `--update-all`, `--update-product`, and `--check-update` (with `--installed`) run as non-GUI/utility flows; single-instance enforcement applies to interactive GUI launches.
