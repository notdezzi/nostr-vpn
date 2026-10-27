# Nostr VPN Windows GUI — Mullvad-style rework (handoff proposal)

Branch: `experimental/wpf-gui-rework`. This is the first implementation pass of
the Mullvad-inspired redesign requested after `windows/sitemap.md` was written.
All functionality from the sitemap is preserved; only the presentation and
navigation changed. Build verified with `scripts/windows-build.ps1
-Configuration Release` (0 warnings / 0 errors) and the launch smoke
(`WINDOWS_APP_LAUNCH_SMOKE_OK`).

## How to run it

- Portable build (no install, no .NET required): `dist/nostr-vpn-portable-win-x64.zip`
  (source folder `dist/portable-win-x64/`). Unzip anywhere, run
  `NostrVpn.Windows.exe`. The bundle contains the GUI, the native app core
  (`nostr_vpn_app_core.dll`), the daemon (`nvpn.exe`), and `binaries/wintun.dll`.
- Rebuild from source: `powershell -File scripts/windows-build.ps1 -Configuration Release -Run`
  (adds `-Publish` for the publish output, `-Installer` for the Inno Setup installer).

## New information architecture

**Shell (Mullvad-style):** the left sidebar is gone. One header bar:
"Nostr VPN" on the left; on the right the **Wallet** button (shows the balance),
the **profile** button (opens a panel under the icon), and the **Settings**
gear. Below the header: the global update banner, the notice line, then the
page content, centered (~720px column) over the navy theme.

**Home (new default page)** — replaces both the old header toggle area and the
map/location idea:
1. **Network card** — active network name, "X of Y connected", chevron. Clicking
   opens the network switcher menu (switch / add network), same behavior as the
   old sidebar switcher.
2. **Devices card** — the device list (dot = reachable, name with
   This device/Admin/Exit badges, IP + state). Clicking a device opens the full
   Devices page for that device (rename, admin, remove, addresses,
   connectivity stats). Admins see **+ Add device**.
3. **Internet card** — one row showing the current internet source in plain
   text; clicking opens the full Internet page (source picker, sharing,
   WireGuard upstream, Exit DNS).
4. **Connect / Disconnect pill** (green when off, neutral when on) with a
   headline under it: **Protected** / **Not protected** / **Connecting…** /
   **Internet blocked**, plus the detailed status line.
   With no network configured, Home shows a single "Set up your network" card
   that opens the create/join flow.

**Profile panel (under the profile icon):** device name, MagicDNS name, VPN IP,
**Device ID with Copy**, a **Wallet summary (balance + Open wallet)**, and the
app/daemon version. This is the "account screen" equivalent.

**Back navigation:** every sub-page (Devices, Add Network, Add Device,
Internet, Public Exits, Wallet, Sell Access, Settings) has a **‹ Back** button
to Home.

## Settings — sorted and renamed

Normal (always visible), in order:
1. **Appearance** — Theme: System / Light / Dark.
2. **General** — "Connect automatically when the app starts", "Start Nostr VPN
   when I log in to Windows", "Keep running in the notification area when the
   window is closed".
3. **Updates** — status line, "Check now", "Install updates automatically".
4. **Network** — rename active network, "Remove this network", "Create or join
   a network", and the list of inactive networks with Activate/Remove.
5. **Internet** — "Share this device's internet", "Block internet if the source
   disconnects", buttons to open the internet source picker and Sell access.
6. **This device** — device name + Save, VPN IP shown read-only.
7. **Pro settings** — a "Show pro settings" switch.

Pro section (only when the switch is on), in order:
1. **This device · advanced** — Endpoint, Listen Port, Tunnel IP, public .fips
   inbound TCP ports, Save.
2. **Nostr relays** (was "Relays") — add/enable/delete, with a one-line
   explanation of what they do.
3. **FIPS** — the three discovery switches.
4. **Public FIPS address** (was "Public FIPS routing") — hosting toggle,
   `<npub>.fips` address, inbound ports, Learn FIPS link.
5. **Wallet display** — fiat value toggle + currency.
6. **System** — versions, service/CLI install buttons, update status.
7. **Diagnostics** — expander with interface/roster/FIPS counters and health
   issues.

Other plain-language renames on existing pages: "Tray on close" → the
notification-area wording above; connect button now says **Connect /
Disconnect** instead of On/Off (the tray keeps "Nostr VPN" with a checkmark, as
before); Home internet row explains the active source ("This device's normal
internet", "Through a device in your network", "Paid internet (automatic)", …).

## Deliberately unchanged

- **All commands, validation, and state behavior** — the rework touches only
  XAML plus small ViewModel additions; `AppViewModel` partials are intact.
- **Automation IDs** used by e2e scripts — all preserved, including
  `ExitDnsInternetNavigation` (now the Home internet row),
  `ManualJoinAdminOpen` (now the Home "+ Add device"), and the new
  `HomeNetworkSwitcher` / `ConnectToggle`.
- **Tray menu**, deep links (`nvpn://join-request/…`), close-to-tray, refresh
  cadence, join coordination fast-refresh, update polling, themes
  (Light/Dark/System), and single-instance behavior.
- Pages the user never asked to move: Public Exits (paid market), Sell Access,
  and the Wallet keep their content, just restyled and reachable via
  Back buttons and the header wallet button.

ViewModel additions (small, additive): `AppPage.Home` (new default page),
`IsProMode` (persisted in `HKCU\Software\Nostr VPN\ProMode`),
`ConnectButtonText`, `VpnHeadlineText` / `VpnHeadlineBrush`,
`InternetSourceSummary`. Enrollment flows now land on Home instead of Devices.

## Known gaps / follow-ups

- The old sidebar's "Add network" entry point is now the Home network card menu
  and Settings → Network; no dedicated page in the nav (matches Mullvad).
- The Devices page still contains the old "no network yet" setup cards; they
  are unreachable in normal flow (Home handles it) and kept only as a safety
  net — candidate for deletion in a follow-up pass.
- Per-device detail could later open as an overlay on Home instead of
  navigating to the Devices page.
- The profile panel is read-only except copy/wallet; editing the device name
  lives in Settings → This device.

## Suggested test pass

1. Portable zip: launch → Home renders, network card shows "Set up your
   network" on a fresh data dir (`NVPN_APP_DATA_DIR` to isolate).
2. Create a network → lands on Home with the connect pill; Connect/Disconnect
   toggles the headline Protected/Not protected.
3. Home device rows → Devices detail (rename, copy, admin actions).
4. Internet page: switch source to This device / peer exit; verify Home
   internet row text updates.
5. Profile panel: copy Device ID; Open wallet switches to the Wallet page.
6. Settings: Pro off hides relays/FIPS/system/diagnostics; toggling Pro
   persists across restarts.
7. Tray menu still toggles VPN and switches internet source; close-to-tray
   still hides the window.
