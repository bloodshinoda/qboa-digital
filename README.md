# Qboa Digital

## Architecture

Qboa Digital is a Windows maintenance utility built with Tauri, Rust, HTML, CSS and vanilla JavaScript. The app follows a small layered architecture:

- Frontend UI and task selection
- Task registry metadata and preset resolution
- Task engine and command execution
- Safety manager for backups, restore points and change logging
- Windows integration through native commands and PowerShell

## Task Engine

The backend exposes a central task registry with metadata such as:

- task id
- category
- risk
- administrative requirement
- reversibility
- restore point requirement
- rollback strategy
- preset membership

Tasks are resolved by task id instead of allowing arbitrary frontend commands.

## Safety Model

The safety layer is designed to record changes and protect destructive operations:

- restore points when required
- backup metadata for reversible changes
- change journal for task execution history
- rollback entries and session-level rollback ordering

## Restore Points

Windows System Restore is used as a system-level safety net before risky changes. The Qboa app identifies its own restore points using a consistent description convention such as:

- Qboa Digital — Antes da tarefa: X

This does not replace user-created restore points and the app does not remove existing Windows restore points automatically.

## Rollback

Rollback in Qboa has two layers:

### Windows System Restore
A protection mechanism provided by Windows itself.

### Qboa Rollback
A custom rollback layer for reversible changes such as registry and service adjustments. It is recorded and managed via the change journal and applied in reverse order for a session.

## Presets

The app supports three preset modes:

- Express
- Normal
- Turbo

Express is intentionally limited to low-risk operations. Normal adds routine maintenance. Turbo includes more advanced tasks and stronger protection.

## Security Model

Security principles:

- frontend sends task ids only
- Rust resolves and validates task execution
- no arbitrary shell command construction from the UI
- CSP enabled in Tauri config
- admin elevation is maintained via the Windows manifest when needed

## Development

```bash
npm install
npm run dev
```

## Testing

```bash
cd src-tauri
cargo test
cargo check
```

## Windows Requirements

- Windows 10 or 11
- WebView2 runtime
- Rust toolchain
- MSVC build tools for Windows compilation
- Administrator rights for some tasks such as DISM, SFC and registry modifications

## Build

```bash
npm run build
```

This is valid for a local Tauri build on a compatible machine. Windows-specific packaging and live Windows operations are only valid on Windows.

## GitHub Actions

The workflow `.github/workflows/windows-build.yml` builds the Windows application on `windows-latest` and publishes two artifacts:

- `qboa-digital-windows-installer`: NSIS installer `.exe`
- `qboa-digital-windows-executable`: executable portable `.exe`

It runs automatically on pushes to `main` and version tags such as `v0.2.0`. It can also be started manually from the **Actions** tab using **Run workflow**. After the job finishes, download the artifact from the workflow run.
