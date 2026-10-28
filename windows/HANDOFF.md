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
"Nostr VPN" title on Home and a **back chevron everywhere else** (exactly where
the title sits, so it never moves; the chevron also appears while a device
detail is drilled open on Home); on the right: Account, Wallet, Settings. The
whole row drags the window. Back always returns to the base Home cards in one
step.

**Tray-only lifecycle** (Mullvad-style): a **direct launch — double-clicking
the exe or opening it from the Start menu — shows the window** (bottom-right).
Only the autostart entry launches with `--hidden`, so Windows login keeps the
app in the tray. A **single left click on the tray icon** toggles the window
(click again to hide); the tray menu's "Open Nostr VPN", deep links, and
relaunching the exe also surface it. **Clicking anywhere outside the window
(e.g. the desktop) hides it** immediately; the blur-hide is briefly deferred
so the tray click can toggle instead of fighting it, and the remove-device /
remove-network confirmation dialogs suppress it while open. Closing is always
**to the tray** (`OnClosing` cancels and hides; the only way to exit the app
entirely is the tray menu's **Quit**) — the old "Keep running in the
notification area" setting and its `CloseToTrayOnClose` plumbing were removed.
The launch smoke runs in default (window-required) mode, since a direct launch
now shows the window.

The **tray menu is minimal**: "Nostr VPN" toggle with a status subtitle, an
"Internet Source" submenu (share/current source picker), then Open / Quit —
device lists and Copy Device ID live in the window, not the tray. The menu
pins `Segoe UI 9pt` explicitly because the WinForms menu font otherwise
auto-scales on top of the WPF process DPI awareness and renders oversized at
display scaling above 100%.

The window **spawns in the bottom-right corner of the primary work area, 10px
from each edge** — the same placement Mullvad uses on Windows 11 (their
`getWindowMargin` for Win11 is 10px). `OnSourceInitialized` computes
`Left/Top` from `SystemParameters.WorkArea`; after that, the window keeps
whatever position the user drags it to (also across close-to-tray re-shows).

Scrollbars are **custom minimal**: a 3px rounded thumb (`ScrollThumb` /
`ScrollThumbHover` theme brushes), no track buttons or track fill, applied
globally via a `ScrollBar` style in `App.xaml` (vertical and horizontal).

**Themed chrome (polish pass):** every remaining system-drawn control got an
implicit style in `App.xaml` so nothing renders with light Windows chrome in
the dark theme:
- **List rows** (`ListBoxItem`) draw a rounded hover pill (`HoverBg`, 6px
  corners, 10px horizontal padding) instead of the default sharp-cornered
  highlight that sat flush against the row text; selected rows get
  `AccentSoft`, and keyboard focus gets an accent outline. The Home device
  rows inherit this via `BasedOn="{StaticResource {x:Type ListBoxItem}}"`.
- **Menus** — the footer network switcher's `ContextMenu` renders as a rounded
  `PanelBg` panel with pill-highlighted `MenuItem`s, a ✓ for checked items,
  themed `Separator`s, and themed submenus; the old per-menu brush overrides
  in `OpenNetworkMenu` were dropped (the template owns the chrome).
- **Inputs** — `TextBox` and `ComboBox` are rounded 6px fields with themed
  caret/selection and an accent border on focus/open; `ComboBoxItem`s get the
  same pill highlight; the combo popup is a themed rounded panel.
- **`ToolTip`s** are themed dark/light panels instead of the yellow system tip.
- **Buttons** use 6px corners (matching cards at 8px) and show an accent
  border when keyboard-focused (titlebar icon buttons: soft fill), so focus
  remains visible with the system focus visuals overridden.

Both themes define `HoverBg` (dark `#2A2A2A`, light `#EEF2F5`) for these
hover pills. Also, closing the device drill-down clears the ListBox selection
so no row keeps a stale highlight after returning to Home.

Mullvad's 320px width forced single-column layouts everywhere: dense button
rows (rename/hints/actions, Public Exits filters and offer actions, System
tools, update banner) wrap instead of using side-by-side columns, and the
Internet source picker shows title + subtitle stacked. The in-content Back
buttons were removed — the titlebar chevron is the single back control. The
old **Devices page is deleted**: its device list duplicated Home's card and its
"no network yet" setup cards duplicated the AddNetwork page, so the surviving
device detail now lives **on Home** as a drill-down (`DeviceDetailOpen` swaps
the Home cards for the detail card); `AppPage.Devices` and the unused
`ShowDevicesCommand` are gone.

## New information architecture

**Shell (Mullvad-style):** the left sidebar and the native title bar are gone.
One custom titlebar row: "Nostr VPN" on the left; on the right, in order:
**Account** (user icon), **Wallet**, **Settings**, and the window **close**
button; the whole row drags the window. Below the titlebar: the global update
banner, the notice line, then the page content over the theme (dark theme is
charcoal `#121212` with soft-gray `#888888` accent; light theme unchanged in
spirit).

**Home (new default page)** — the Mullvad "map screen" equivalent:
1. **Devices card** — the device list (dot = reachable, name with
   This device/Admin/Exit badges, IP + state). Clicking a device swaps the Home
   cards for that device's **detail drill-down** (rename, admin, remove,
   addresses, connectivity stats) while **the footer stays pinned**, so the
   network selector and Connect toggle never disappear; the titlebar chevron
   (one Back) returns to the base cards. The rows carry the e2e
   `RosterParticipantAccepted-<npub>` AutomationId. Admins see **+ Add device**.
2. **Internet card** — one row showing the current internet source in plain
   text; clicking opens the full Internet page (source picker, sharing,
   WireGuard upstream, Exit DNS).
3. **Fixed bottom bar** (pinned, never scrolls away — only on Home):
   the **network selector** (active network name, "X of Y connected", chevron —
   clicking opens the network switcher menu: switch / add network; AutomationId
   `HomeNetworkSwitcher`), the **Connect / Disconnect pill** (green when off,
   neutral when on; AutomationId `ConnectToggle`), and the status headline
   under it: **Protected** / **Not protected** / **Connecting…** /
   **Internet blocked**, plus the detailed status line.
   With no network configured, Home shows a single "Set up your network" card
   that opens the create/join flow (the bottom bar then shows just the toggle
   and status).

**Account page (the user icon):** a real page switch (like Settings/Wallet),
not a popout — device name, MagicDNS name, VPN IP, **Device ID with Copy**
(with a plain hint: "Share this with a network admin so they can add you"), a
**Wallet card (balance + Open wallet)**, and the app/daemon version.

**Back navigation:** every sub-page (Account, Add Network, Add Device,
Internet, Public Exits, Wallet, Sell Access, Settings) is left via the
**‹ chevron in the titlebar** (where the app title sits on Home) — there are
no in-content Back buttons anymore. Navigating always scrolls the content back
to the top, leaving Home closes an open device drill-down, and removing a
device or network closes the detail instead of silently showing another
device.

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
- **Deep links** (`nvpn://join-request/…`), refresh cadence, join
  coordination fast-refresh, update polling, themes (Light/Dark/System), and
  single-instance behavior. (Close-to-tray is now unconditional; the tray
  icon click toggles the window instead of the old double-click, and the
  tray menu was trimmed to a minimal Mullvad-style set.)
- Pages the user never asked to move: Public Exits (paid market), Sell Access,
  and the Wallet keep their content, just restyled and reachable via
  Back buttons and the header wallet button.

ViewModel additions (small, additive): `AppPage.Home` and `AppPage.Account`
(new default page + account page), `ShowAccountCommand`,
`IsProMode` (persisted in `HKCU\Software\Nostr VPN\ProMode`),
`ConnectButtonText`, `VpnHeadlineText` / `VpnHeadlineBrush`,
`InternetSourceSummary`, and `DeviceDetailOpen` for the Home device
drill-down.
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
- The duplicate Devices-page UI (device list, "no network yet" setup cards) was
  deleted; the AddNetwork page is the single create/join flow and Home the
  single device list.
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
3. Home device rows → detail drill-down on Home (footer stays pinned; one Back
   returns to the cards; re-clicking the selected row still reopens it).
4. Internet page: switch source to This device / peer exit; verify Home
   internet row text updates.
5. Profile panel: copy Device ID; Open wallet switches to the Wallet page.
6. Settings: Pro off hides relays/FIPS/system/diagnostics; toggling Pro
   persists across restarts.
7. Tray menu still toggles VPN and switches internet source; close-to-tray
   still hides the window.
