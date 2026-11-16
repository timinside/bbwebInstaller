<div align="center">

  <img width="100" height="100" alt="bbweb Installer icon" src="assets/icon.png" />

  # bbweb Installer

  **Lightweight manifest-driven installer and updater for bbweb projects.**  
  Single `.exe` · Win32 UI · Self-update · CLI automation · Windows Service

</div>

---

## Features

- Loads the project list and install parameters from a remote JSON manifest (`MANIFEST_URL` in `src/main.rs`). No offline project list is hardcoded.
- A local `manifest.json` next to the exe takes priority over the remote manifest; `--manifest-url <url|path>` overrides both.
- Installs projects with per-project arguments from the manifest (typically `/SILENT /NORESTART`) and shows download progress/status.
- Checks for available updates for installed programs and for bbweb Installer itself; **Install / Update all / Update manifest** actions.
- Self-update: downloads the new version, replaces the exe, and restarts with the same arguments.
- Optional logon check (`--installed --service`): shows a balloon notification when updates exist and exits when there is nothing to do.
- Single GUI instance: if the app is already running (including minimized), launching it again activates the existing window.

## Usage

### GUI

- Run `bbwebinstaller.exe` with no arguments to open the installer UI (requires access to the manifest: internet or a local `manifest.json`).
- Select a project and click **Install**.
- **Update all** — updates programs with available updates, then bbweb Installer if needed.
- **Update manifest** — reloads the manifest and refreshes the list/status.

### Command-line flags

| Flag | Description |
| --- | --- |
| `--manifest-url <url\|path>` | Override manifest source (URL or local path). |
| `--logs` | Write logs to `C:\Users\<user>\.bbweb\logs\bbwebinstaller.log`. |
| `--nocheck` | Suppress update notifications. |
| `--installed` | Installed-apps context (scan/update/service flows). |
| `--service` | Logon check mode (with `--installed`): balloon if updates exist, else exit immediately. |
| `--check-update <id\|name>` | (with `--installed`) Check update for a project. Exit: `100` available, `0` none, `3` not installed, `2` not found. |
| `--update-product <id\|name>` | (with `--installed`) Install/update only that project. Exit: `0` success, `1` error, `2` not found. |
| `--update-all` | (with `--installed`) Update all available projects (and bbweb Installer if needed). |
| `--product-check <id\|name>` | (with `--installed`) Show installed/latest info dialog. |
| `--help`, `-h`, `/?` | Show help. |

Examples:

```text
bbwebinstaller.exe
bbwebinstaller.exe --installed --service --logs
bbwebinstaller.exe --installed --check-update bbruntime --logs
bbwebinstaller.exe --installed --update-product bbruntime --logs
```

## Manifest

Format: JSON with `installer` and `projects[]` objects.

- `installer.version`, `installer.download_url` — bbweb Installer version and download URL for self-update.
- `projects[].id`, `name`, `version`, `url`, `file`, `args` — project info, installer URL/file, and install arguments.
- `projects[].installed_version_path` — path to a file containing the installed version.
- `projects[].latest_version_url` — URL of a file containing the latest version (when `version` is not a version URL).
- `version` can be a literal version string or an `http(s)` URL pointing to a version file.

## Build

Requires a Rust toolchain (`cargo`, `rustc`, Windows MSVC target).

```bash
cargo build --release
```

Output: `target\release\bbwebinstaller.exe` (release profile: `lto`, `strip`, `opt-level = "z"`, static CRT).

## Service mode and system impact

`--installed --service` (typically run by Task Scheduler at logon) loads the manifest, checks installed versions, and:

- exits immediately if there are no updates (or `--nocheck`);
- shows only a balloon notification if updates exist (no persistent tray icon/process) and exits after it is dismissed or acted on;
- performs self-update and restarts with the same arguments if the installer itself needs updating.

No telemetry is built in. Logs are written only with `--logs` to `C:\Users\<username>\.bbweb\logs\bbwebinstaller.log`. Network activity happens only during runs.

## Reuse / Fork

This project can be reused for your own purposes: fork the repository, change the server/manifest information (e.g. `MANIFEST_URL` and your hosted `manifest.json`), rename the program/branding as needed — and you're ready to go.

## Wishlist

- [ ] Better UI
- [ ] All project, integration with bbweb Installer updates system
- [ ] Optional silent/background update flow improvements
- [ ] Better diagnostics and troubleshooting output
- [ ] More flexible manifest/project source options

## FAQ

<details>
<summary><b>Does it require the Visual C++ Redistributable?</b></summary>

No. Recent builds are linked with the static CRT, so <code>VCRUNTIME140.dll</code> is not required on a clean Windows install.
</details>

<details>
<summary><b>Where does it download installers?</b></summary>

Downloads are saved to <code>%LOCALAPPDATA%\bbwebInstaller\downloads</code>, then run with per-project arguments from the manifest.
</details>

<details>
<summary><b>Does the logon check run in the background all the time?</b></summary>

No. The scheduled task runs at logon, checks for updates, and exits quickly if there are none. If updates exist, it shows a notification and exits after it is dismissed or handled.
</details>

<details>
<summary><b>Can I use my own project list/server?</b></summary>

Yes. You can point to your own manifest with <code>--manifest-url</code>, place a local <code>manifest.json</code> next to the exe, or fork and change <code>MANIFEST_URL</code>.
</details>

---

## Author & Socials

Want to follow the development, see what **bbzer0** is working on, and get news about FlaxyIDE and other projects? Join the Telegram channel. Everyone is welcome!

<table>
  <tr>
    <td align="center" width="64">
      <a href="https://t.me/bbzer0_community">
        <img src="https://github.com/user-attachments/assets/6a992317-a16b-4f6e-a596-81aad27f5d2d" width="32" height="32" alt="Telegram" />
      </a>
    </td>
    <td>
      <b><a href="https://t.me/bbzer0_community"><i>my name is bbzer0</i> on Telegram</a></b><br />
      Information about my projects, updates, and announcements.<br />
      <sub>🇷🇺 The channel is in Russian. Please join us!</sub>
    </td>
  </tr>
</table>

<div align="center">
  Made with ❤️ by <a href="https://github.com/timinside">bbzer0</a>
</div>
