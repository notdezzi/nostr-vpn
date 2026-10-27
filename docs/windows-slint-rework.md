# Windows shell rework: WPF → Slint (handoff for manual review)

Branch: `build/windows-installer-current-fixes`. **No PR has been created** — this
is staged for manual review and acceptance first, per plan
(`docs/slint-desktop-rework-plan.md`).

## What this is

The WPF shell (`windows/NostrVpn.Windows/`, C#/.NET 8) is replaced by a Rust +
Slint desktop app in the new crate `windows/nostr-vpn-windows` (bin
`nostr-vpn-gui`). It drives the same shared core
(`crates/nostr-vpn-app-core`, `FfiApp`/`NativeAppState`/`NativeAppAction`) that
the Linux shell uses, so behavior parity is enforced by the core rather than by
re-implementing policy in the shell.

The WPF project is left in place for this review. Nothing else references it
except the build script (swapped below); it can be deleted in a follow-up once
accepted.

## Layout

```
windows/nostr-vpn-windows/
  Cargo.toml          workspace-excluded crate, own Cargo.lock (sync-versions covers it)
  build.rs            slint-build compiles ui/appwindow.slint
  ui/appwindow.slint  whole window: sidebar nav, 8 pages, setup view, popup, about window
  ui/tray.slint       SystemTrayIcon with the WPF TrayService menu layout
  assets/             icon-256.png, tray-24.png (copied from WPF Assets)
  src/main.rs         app entry: single instance, deep links, timers, ~60 callbacks,
                      updater loop, pending-action continuations
  src/bridge.rs       state → App-global properties/models, draft sync rules, tray push
  src/display.rs      WPF display-text formatting port (msat/bytes/durations/status strings)
  src/platform.rs     mutex + named pipe, Run key, nvpn:// protocol, prefs, clipboard, dialogs
  src/updater.rs      check/download via nostr-vpn-core updater, WPF message strings
  src/e2e.rs          headless --nvpn-e2e-update-check / --nvpn-e2e-install-update
  src/worker.rs       core worker thread (request/result channels)
  src/qr.rs           QR render (qrcode) + decode (rqrr)
  src/deep_link.rs    nvpn:// parsing (ports linux/src/deep_link.rs)
```

## Parity notes (what was ported deliberately)

- **Single instance**: mutex `Local\NostrVpn.Windows`; second launches forward
  argv over `\\.\pipe\NostrVpn.Windows.DeepLinks` (JSON string array,
  `ERROR_PIPE_CONNECTED` accepted), primary shows its window.
- **Startup**: HKCU Run key `Nostr VPN` = `"<exe>" --hidden`; kept in sync with
  the core setting on first state.
- **Protocol**: `HKCU\Software\Classes\nvpn` → `nvpn://` deep links; join
  requests confirm via dialog before `ImportJoinRequest`.
- **Prefs**: `HKCU\Software\Nostr VPN` `AutoInstallUpdates`; **addition**: a
  `Theme` value (0 system / 1 light / 2 dark) since Slint needs an explicit
  dark-mode switch and WPF had no theme setting.
- **Refresh cadence**: 1 s during the 20 s join window, 2 s during wallet
  top-up, else 15 s (matches `RefreshIntervalForState`).
- **Draft sync**: drafts update only on dispatch results, never on refresh;
  guarded fields (manual provider, seller price, filters) mirror the WPF
  `lastSynced`/editing-flag logic. Alias/hints inputs reset on every apply,
  matching WPF OneWay binding.
- **Deferred notices**: in-progress status shows only after 250 ms.
- **Tray**: same menu order (VPN toggle → status → device → Copy Device ID →
  network devices → Internet Source with kinds direct/paid-auto/paid-manual/
  wireguard/peer → Open → Quit), tooltip `Nostr VPN - <status>` truncated to 63
  chars, red-dot overlay at the WPF position when the exit is blocked.
- **Updater**: same strings ("Update {tag} available", "Up to date",
  unverified-refusal), install only when a verified Windows asset exists,
  6 h poll (`NVPN_UPDATE_POLL_SECONDS`), `NVPN_UPDATE_SKIP_OPEN`.
- **E2E**: `--nvpn-e2e-update-check` / `--nvpn-e2e-install-update` write the
  WPF-identical JSON to `NVPN_UPDATE_E2E_RESULT_PATH` (verified against the
  live release feed: found v4.1.19 + setup asset, exit 0).

## Deviations from WPF

- Native message boxes → `rfd` dialogs; file pickers → `rfd` file dialogs.
- TabWidget (Receive/Send wallet tabs), GridLayout device-detail, and light/
  dark theme tokens are Slint-idiomatic; text content mirrors the WPF strings.
- `nostr_vpn_app_core.dll` is no longer shipped (the Rust shell links the core
  statically). Payload is `nostr-vpn-gui.exe` + `nvpn.exe` +
  `binaries\wintun.dll` + `Assets\nostr-vpn.ico`.
- About dialog is a second top-level Slint window (AboutSlint widget) instead
  of a WPF page.

## Build / run / package

```
cargo build --manifest-path windows/nostr-vpn-windows/Cargo.toml   # or scripts/windows-build.ps1
cargo test  --manifest-path windows/nostr-vpn-windows/Cargo.toml   # 4 tests pass
scripts/windows-build.ps1 -Installer                               # Inno Setup installer
```

`scripts/windows-build.ps1`: the dotnet publish stage is replaced by the cargo
GUI build + payload staging (daemon `nvpn.exe` and `wintun.dll` still build
from the workspace first). `scripts/windows-installer.iss`: icons/Run/
UninstallDisplayIcon/CloseApplicationsFilter point at `nostr-vpn-gui.exe`.
`scripts/sync-versions.mjs`: covers the new crate's `Cargo.toml`/`Cargo.lock`
(`--check` on this machine fails on the pre-existing root `Cargo.lock` target
because this checkout uses `core.autocrlf=true`; CI checkouts are LF).

## Suggested review checklist

1. Run `scripts/windows-build.ps1 -Run`, create/join a network, check devices,
   alias/hints, admin toggle, remove.
2. Internet page: direct / peer / share toggle / leak protection / WireGuard
   import; market page filters, buy, sessions; wallet top-up/redeem/QR scan.
3. Tray menu actions incl. blocked-state red dot; close-to-tray setting.
4. `nvpn://<join request>` deep link; second-instance focus behavior.
5. Settings toggles persist after restart; `--hidden` startup.
6. Installer upgrade over an existing WPF install (migrate script handles both
   binary names).
