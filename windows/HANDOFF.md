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

## Window behavior (Mullvad-style)

The window is **borderless, fixed at Mullvad's exact size (320x568),
non-resizable, with rounded corners** (`WindowStyle="None"` + `WindowChrome`
with a 1px glass frame for the DWM drop shadow, and an explicit
`DwmSetWindowAttribute(DWMWA_WINDOW_CORNER_PREFERENCE, ROUND)` call on
`OnSourceInitialized` — borderless windows are not rounded by default on
Windows 11; older systems just stay square). The titlebar
has **no close button** — on the left there is a fixed slot that shows the
"Nostr VPN" title on Home and a **back chevron on every other page** (exactly
where the title sits, so it never moves); on the right: Account, Wallet,
Settings. The whole row drags the window. The titlebar back chevron also backs
out of the Devices drill-down (detail → list → Home).

**Tray-only lifecycle** (Mullvad-style): the app **starts into the system
tray** — the window never opens on its own, only via a **single left click on
the tray icon** (toggling it: click again to hide), the tray menu's
"Open Nostr VPN", a deep link, or launching the exe again. **Clicking anywhere
outside the window (e.g. the desktop) hides it** immediately; the blur-hide is
briefly deferred so the tray click can toggle instead of fighting it, and the
remove-device / remove-network confirmation dialogs suppress it while open.
Closing is always **to the tray** (`OnClosing` cancels and hides; the only way
to exit the app entirely is the tray menu's **Quit**) — the old
"Keep running in the notification area" setting and its `CloseToTrayOnClose`
plumbing were removed. The launch smoke runs with `-NoWindowRequired` since no
window appears at startup.

The window **spawns in the bottom-right corner of the primary work area, 10px
from each edge** — the same placement Mullvad uses on Windows 11 (their
`getWindowMargin` for Win11 is 10px). `OnSourceInitialized` computes
`Left/Top` from `SystemParameters.WorkArea`; after that, the window keeps
whatever position the user drags it to (also across close-to-tray re-shows).

Scrollbars are **custom minimal**: 8px rounded thumb (`ScrollThumb` /
`ScrollThumbHover` theme brushes), no track buttons or track fill, applied
globally via a `ScrollBar` style in `App.xaml` (vertical and horizontal).

Mullvad's 320px width forced single-column layouts everywhere: the Devices
page became a **drill-down** (list card, then a device detail card, driven by
the `DeviceDetailOpen` view-model flag), dense button rows (rename/hints/
actions, Public Exits filters and offer actions, System tools, update banner)
wrap instead of using side-by-side columns, and the Internet source picker
shows title + subtitle stacked. The in-content Back buttons were removed —
the titlebar chevron is the single back control.

## New information architecture

**Shell (Mullvad-style):** the left sidebar and the native title bar are gone.
One custom titlebar row: "Nostr VPN" on the left; on the right, in order:
**Account** (user icon), **Wallet**, **Settings**, and the window **close**
button; the whole row drags the window. Below the titlebar: the global update
banner, the notice line, then the page content over the theme (dark theme is
charcoal `#121212` with soft-gray `#888888` accent; light theme unchanged in
spirit).

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

**Account page (the user icon):** a real page switch (like Settings/Wallet),
not a popout — device name, MagicDNS name, VPN IP, **Device ID with Copy**
(with a plain hint: "Share this with a network admin so they can add you"), a
**Wallet card (balance + Open wallet)**, and the app/daemon version.

**Back navigation:** every sub-page (Account, Devices, Add Network, Add
Device, Internet, Public Exits, Wallet, Sell Access, Settings) is left via the
**‹ chevron in the titlebar** (where the app title sits on Home) — there are
no in-content Back buttons anymore.

## Settings — sorted and renamed

Normal (always visible), in order:
1. **Appearance** — Theme: System / Light / Dark.
2. **General** — "Connect automatically when the app starts", "Start Nostr VPN
   when I log in to Windows".
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
- **Tray menu**, deep links (`nvpn://join-request/…`), refresh cadence, join
  coordination fast-refresh, update polling, themes (Light/Dark/System), and
  single-instance behavior. (Close-to-tray is now unconditional; the tray
  icon click toggles the window instead of the old double-click.)
- Pages the user never asked to move: Public Exits (paid market), Sell Access,
  and the Wallet keep their content, just restyled and reachable via
  Back buttons and the header wallet button.

ViewModel additions (small, additive): `AppPage.Home` and `AppPage.Account`
(new default page + account page), `ShowAccountCommand`,
`IsProMode` (persisted in `HKCU\Software\Nostr VPN\ProMode`),
`ConnectButtonText`, `VpnHeadlineText` / `VpnHeadlineBrush`,
`InternetSourceSummary`, and `DeviceDetailOpen` for the Devices drill-down.
Removed during the compact-titlebar pass: `WalletNavigationText` and its
`NavigationBalanceText` model field (the header wallet button is icon-only
now; the balance is on the Account and Wallet pages), and the close-to-tray
setting (`SetCloseToTrayAsync`, `SettingsPatch.CloseToTrayOnClose`,
`NativeAppState.CloseToTrayOnClose`) since closing now always goes to the
tray. Enrollment flows now land on Home instead of Devices.

Theme: `Themes/Dark.xaml` is a charcoal palette — background `#121212`, cards
`#1E1E1E`, primary text `#E0E0E0`, secondary text `#B0B0B0`, borders
`#444444`, accent `#888888` (semantic green/amber/red kept for status dots and
warnings). `Themes/Light.xaml` uses the same green accent on white.

## Known gaps / follow-ups

- The old sidebar's "Add network" entry point is now the Home network card menu
  and Settings → Network; no dedicated page in the nav (matches Mullvad).
- The Devices page still contains the old "no network yet" setup cards; they
  are unreachable in normal flow (Home handles it) and kept only as a safety
  net — candidate for deletion in a follow-up pass.
- At 320px some pre-existing wide controls rely on text trimming (account
  card values, Wallet receive/send rows, relay URLs) rather than dedicated
  narrow layouts.
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
