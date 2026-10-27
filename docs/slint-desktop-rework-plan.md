# Slint Desktop UI Rework — Analysis and Implementation Plan

Status: proposal, not scheduled work. Written 2026-10-26 on
`build/windows-installer-current-fixes`.

## TL;DR

Porting the desktop shells to [Slint](https://slint.dev) is **feasible and worth
doing for Windows first**. The Windows WPF shell is the smallest (≈6.3k LOC),
the weakest visually (raw default WPF controls, no custom templates), and the
one whose replacement removes an entire toolchain (.NET) from the build. A
Slint shell talks to the *same* `FfiApp` state/action boundary the other shells
use, so no protocol, policy, or dataplane code changes.

Linux is a reasonable second port (its GTK shell is good, but a shared Slint UI
halves long-term maintenance and deletes ~900 LOC of hand-rolled D-Bus tray
code). **macOS should stay SwiftUI** — it is the most polished shell, its
platform integrations are deeply AppKit, and replacing it trades native quality
away for nothing.

Single-PR "replace all desktop UIs" will not be merged upstream. Stage it:
Windows shell first, ask the maintainer before building, remove WPF only after
parity is proven.

---

## 1. Current state (measured)

All shells already share one model — `crates/nostr-vpn-app-core` defines
`NativeAppAction` (~60 variants) and `NativeAppState` (~97 fields); `FfiApp`
exposes `state() / refresh() / dispatch(action)` and every call returns the
full new state. This is the single biggest enabler for a shared UI toolkit.

| Platform | Shell | LOC | Core boundary | Verdict today |
| --- | --- | --- | --- | --- |
| Windows | WPF/.NET 8 | ≈5.2k C# + 1.2k XAML | JSON C ABI (P/Invoke) | Weakest: default Aero2 chrome, 45 `async void` handlers, visibility-switched single window, god-VM with brute-force property sweeps, dark mode only recently added |
| Linux | GTK4/libadwaita (Rust) | ≈9.3k | Direct typed Rust API | Solid and idiomatic; GNOME-native look; 899 LOC hand-rolled SNI tray |
| macOS | SwiftUI/AppKit | ≈9.9k hand-written + 7.4k generated | UniFFI | Best shell: custom tray, Touch ID privileged runner, verified updater |

Non-UI platform glue that any new shell must keep (it does not move into
Slint): Windows service/Wintun (dispatched via core actions, not UI code),
startup registry entry, `nvpn://` protocol registration + single-instance
mutex/pipe, verified updater; Linux zbarcam QR scan, XDG autostart, desktop
file, deb packaging; macOS privileged command runner, launchd agent sync,
single-instance flock, updater swap script.

## 2. Why Slint fits

- **Rust-native.** The Windows shell becomes pure Rust driving
  `nostr-vpn-app-core` directly (typed API, like Linux does today) — the JSON
  C ABI and the whole .NET layer disappear from the desktop stack.
- **One UI codebase for Windows + Linux.** The 8 Windows pages, 7 Linux pages
  and 6 macOS pages cover the same product surface (per
  `docs/native-ui-parity-matrix.md`); a shared `.slint` page set serves both
  desktop targets from one crate.
- **System tray is built in.** Slint ≥ 1.17 ships `SystemTrayIcon`
  (Windows notification area, macOS menu bar, Wayland/X11 via StatusNotifier
  D-Bus — the same protocol `linux/src/tray.rs` implements by hand today).
- **Dark mode is a built-in color scheme**, closing the biggest Windows
  complaint permanently instead of via resource-dictionary swapping.
- **Licensing is unproblematic.** Project is MIT; Slint can be used under
  GPLv3, or under the Royalty-free license (explicitly covers desktop apps)
  by showing the `AboutSlint` widget in an About/settings dialog or the badge
  on the download page. Zero cost either way.
- **Declarative `.slint` markup with live preview** maps well onto the existing
  rebuild-from-state rendering model; the poll/dispatch loop and
  core-worker-thread pattern can be lifted almost verbatim from
  `linux/src/main/core_worker.rs`.

Not chosen, briefly: Avalonia/WPF-UI keep the .NET dependency and don't unify
with Linux; Tauri adds a JS/web toolchain and bundle weight; egui/iced are
less polished for a product UI. Slint is the only option that unifies
Windows + Linux in Rust with quality widget styling.

## 3. What Slint does *not* replace

Kept native, per platform (the parity matrix's own rule: native owns
lifecycle, permissions, OS integrations):

- Service install/control — already dispatched to the core via
  `InstallSystemService` / `EnableSystemService` actions; UI only displays
  status.
- Updater logic — `nostr_vpn_core::updater` is called directly by the Linux
  crate today; the new Windows crate does the same (the WPF shell currently
  reaches the same code through C ABI JSON wrappers). The Windows
  install/swap step stays a platform concern.
- Single instance, deep links, startup-on-login, protocol registration —
  ~200 LOC each platform, ported as-is.
- QR camera capture — Windows has image import only today; parity preserved.
  Linux `zbarcam` subprocess stays. QR *rendering* uses the `qrcode` crate
  into a Slint image, as `linux/src/qr.rs` does with cairo.
- macOS: everything. See §5.

## 4. Proposed architecture

```
crates/nostr-vpn-ui-slint/        # NEW shared desktop UI
  ui/*.slint                      # pages, components, theme
  src/bridge.rs                   # NativeAppState → Slint models; callbacks → NativeAppAction
  src/core_worker.rs              # port of linux/src/main/core_worker.rs
  src/theme.rs                    # dark/light/system scheme switch

crates/nostr-vpn-windows/         # NEW Windows shell (replaces WPF crate)
  src/main.rs                     # entry, single-instance, deep links, startup registry
  Cargo.toml                      # deps: nostr-vpn-ui-slint, nostr-vpn-app-core, nostr-vpn-core(updater)

linux/                            # OPTIONALLY re-pointed at nostr-vpn-ui-slint
  (keeps: zbarcam scan, autostart, desktop file, deb packaging, service settlement polling)

macos/                            # unchanged
```

- Zero changes to `crates/nostr-vpn-app-core` (the boundary already has
  everything; capability flags like `serviceSupported` already gate UI chrome
  per platform).
- No dataplane, FIPS, or protocol surface changes — this work is entirely
  outside the dataplane concerns in `AGENTS.md`.
- Windows build drops `dotnet` entirely: `cargo build -p nostr-vpn-windows`,
  then the Inno script (`scripts/windows-installer.iss`) swaps the payload.
  The `windows-installer-migrate.ps1` flow is unaffected (same install dir
  layout).

## 5. Phased implementation

### Phase 0 — Spike (1–2 days)
On the Windows dev machine: `cargo new` the shell, one `Window` with the VPN
switch bound to real `FfiApp` state, dispatch `ConnectVpn`, verify the
core-worker-thread pattern and 2s refresh on Windows. Confirms Slint's Windows
backend (winit + femtovg/skia) behaves with the daemon round-trips. Decide
Fluent vs custom style here.

### Phase 1 — Windows parity (2–4 weeks)
1. Shared crate skeleton: theme (dark/light/system), nav, update stripe.
2. Pages, in dependency order: Devices (networks, participants, join-request
   approval, create/join cards incl. WireGuard + exit-DNS fields) → Internet
   (source selection, exit nodes) → Settings (service/CLI cards, startup,
   relay, FIPS, diagnostics) → Wallet (top-up/send/receive/history) → Public
   exits (market) → Sell access (seller). Port `AppViewModel` logic that is
   real (drafts, validation, polling cadence) into the bridge; do not port
   the `RaiseAll` sweeps.
3. Platform glue: single-instance mutex + named pipe, `nvpn://` registration
   and arg forwarding, `HKCU Run` startup, tray via `SystemTrayIcon` (VPN
   toggle, network/exit submenus, status icon), QR image import + render,
   verified updater against `nostr_vpn_core::updater`.
4. Installer: update `.iss`; keep per-user layout; reuse migration script.
5. Verification: existing e2e (`--nvpn-e2e-update-check`) adapted; manual
   parity pass against the matrix's Windows column.

### Phase 2 — Upstream Windows (see §6)
Open the issue/discussion **before** Phase 1 finishes; PR the new shell only
with maintainer buy-in; keep the WPF shell in-tree until the Slint shell has
shipped one release; separate PR removes WPF + .NET from CI/installer.

### Phase 3 — Linux port (1–2 weeks, optional)
Re-point `linux` at the shared UI crate; keep `zbarcam`, autostart, desktop
file, deb metadata, service settlement polling; replace `tray.rs` (899 LOC)
with `SystemTrayIcon`. Net deletion several thousand LOC. Gate: Phase 2
landed well upstream and the maintainer wants it.

### Phase 4 — macOS (not recommended)
Technically possible (Slint supports macOS) but loses SwiftUI polish, the
bespoke tray quality (the codebase deliberately avoids `MenuBarExtra` for its
teardown bugs), and would fight the maintainer's documented "native shells"
philosophy. Only revisit if the maintainer asks.

## 6. Will upstream (mmalmi) accept it?

Evidence from `mmalmi/nostr-vpn` PR history:

- **Your Windows work is already trusted**: #85 (native service control),
  #87 (system/light/dark themes), #88 (dual-stack WG exits) — all merged.
  External fixes #80 (Linux UI), #81 were merged too.
- **But feature/architecture additions get rejected**: #25/#23 (DNS override,
  twice), #73 (iOS tunnel behavior), #58 (FIPS bump), #39/#36 (service fixes)
  — closed unmerged.
- **The parity matrix states the philosophy explicitly**: it is a source
  snapshot, not a migration backlog; native code owns presentation, lifecycle,
  permissions, OS integrations. A wholesale three-platform replacement of
  presentation contradicts that document.

Honest assessment:

- A single "port all desktops to Slint" PR: **almost certainly rejected**.
- A Windows-only shell replacement, proposed first via issue with the
  reasoning in this doc (smallest shell, worst UX, removes .NET, zero boundary
  changes, licensing clean): **genuinely plausible** — the maintainer merges
  well-scoped Windows improvements from this account routinely, and the WPF
  shell is the one nobody is attached to.
- Linux/macOS replacement: **unlikely** unless the Windows port ships cleanly
  and the maintainer opts in.

Recommended sequence: open a GitHub issue on `mmalmi/nostr-vpn` after Phase 0
with screenshots/mockups from the spike, this analysis, and an explicit
Windows-only scope. If declined, the fork ships it under MIT regardless — the
shared-boundary design means the fork stays cheap to maintain against
upstream (rebase surface is `crates/nostr-vpn-app-core` types only).

## 7. Risks

| Risk | Mitigation |
| --- | --- |
| Slint widget polish on Windows is Fluent-ish, not pixel-native | Acceptable — current shell has zero custom styling; pick the Fluent style and a small token theme |
| Slint Windows backend (winit) quirks with tray/hidden window startup (`--hidden`) | Spike explicitly tests boot-to-tray; fallback is `tray-icon` crate + manual window show |
| Maintainer rejects direction | Fork-only path is fully viable (MIT); boundary untouched keeps rebase cost low |
| Updater install UX regresses vs WPF flow | Port `UpdateService` semantics 1:1; keep `--nvpn-e2e-update-check` as the gate |
| Effort overrun on wallet/paid-market pages (densest UI) | Port in dependency order; pages 5–6 can ship one release later without breaking parity (matrix records intentional differences) |

## 8. Effort summary

- Phase 0 spike: 1–2 days
- Windows parity: 2–4 weeks
- Upstream negotiation: parallel, zero-code
- Linux port (optional): 1–2 weeks
- macOS: not planned

Total for the recommended scope (Windows first, Linux optional): roughly
3–6 weeks of focused work, with an expected net deletion of the entire .NET
surface (~6.3k LOC) and, if Linux follows, several thousand GTK LOC replaced
by one shared UI crate.
