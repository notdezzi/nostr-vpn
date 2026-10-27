# Nostr VPN Windows WPF App — Sitemap

Everything the current WPF app (`windows/NostrVpn.Windows`) can do, page by page,
written so the UI can be rebuilt from scratch without losing functionality.
At the end there is a proposal for splitting it into a simple default UI and a
Pro settings area.

Source of truth: `MainWindow.xaml` (955 lines, all pages stacked in one
ScrollViewer), `Controls/WalletView.xaml`, `MainWindow.xaml.cs` (code-behind
actions), `ViewModels/AppViewModel*.cs` (behavior), `Core/Models.cs` (state).

The app is a single window: a 220px left sidebar and a content column with a
sticky header, an optional update banner, and a scrolling stack of "cards"
(8px radius, 16px padding). Window is 920x640, min 760x560, Segoe UI.

---

## 1. Global chrome (visible on every page)

### 1.1 Sidebar (left, 220px, darker background)
- Nav buttons (icon + label): **Devices**, **Internet**, **Wallet** (hidden when
  the paid-route market is unsupported), **Settings**.
- Bottom: **network switcher** card (clickable). Shows a status dot (only when
  more than one network exists), the active network name, and the subtitle
  "X of Y connected". Clicking opens a popup menu listing every saved network
  (with a green/gray dot when multiple exist) plus an **"Add network"** item
  that goes to the Add Network page.

### 1.2 Header (top of content column)
- Left: status dot + active network name (large), below it the VPN status line
  (error text, or exit-node status, or "VPN on"/"Turning on"/"Off"; red when the
  internet source is blocked).
- Right: the primary **On/Off button**. Enabled when VPN control is supported
  and a network is active (or the service still needs installing). If the
  background service is not installed, the first press installs the service
  instead of toggling.

### 1.3 Update banner (between header and content, when an update exists)
- Text "Update available: vX (you're on vY)".
- Checkbox **"Install automatically"** (persisted in the registry).
- Button **Install** (downloads via the daemon and launches the installer).
  Updates are checked at startup, then every 6 hours (override via
  `NVPN_UPDATE_POLL_SECONDS`).

### 1.4 Notice card (top of the scrolling content)
- Transient one-line messages: action statuses ("Saving relays"), "Copied",
  and errors. Collapses entirely when empty.

---

## 2. Pages

The page enum is `Devices, AddNetwork, AddDevice, Internet, PublicExits,
Wallet, SellAccess, Settings`. PublicExits/Wallet silently fall back to
Devices when the paid-route market is unsupported; SellAccess falls back when
the seller feature is unsupported.

### 2.1 Devices page (default)
Two different states.

**State A — no network yet (also duplicated on the Add Network page):**
- Choice card: buttons **Create Network** and **Join Network**.
- Create flow: Back button, title "Create Network", one text field (network
  name) and a **Create** button. Empty name becomes "My Network". Lands on
  Devices for the new network.
- Join flow: Back button, title "Join Network".
  - If an outbound join request exists (and no network is enabled): a QR code
    of the join request plus a **Copy Request** button.
  - A **"Manual join"** expander containing: explanation text, my Device ID
    (npub) + **Copy my Device ID**, an "Admin Device ID" field with live
    validation ("Not a valid device ID"), a "Network ID" field, and an
    **Add manually** button. Device IDs must be `npub1…` (63 chars, bech32).
    Network ID input is normalized (spaces/dashes stripped, lowercased).

**State B — a network exists: two columns (320px list + detail pane).**

List pane:
- Title "Devices" + subtitle "X of Y connected".
- **+ Add device** button (admins only) → Add Device page.
- Selectable list of participants. Each row: dot, display name, inline badges
  ("This device" green, "Admin" accent, "Exit" warn, "via mesh" muted), and a
  second line with the tunnel IP and online state. Each row has a **Copy**
  button (copies the device's npub). Rows auto-select: self first, then
  reachable, then alphabetical.

Detail pane (selected participant):
- Name (large), connectivity text top-right, badges (This device / Admin /
  Exit / "direct connection" / "via mesh").
- **Name row** (admin only): editable name text box + **Save** (renames via
  MagicDNS alias).
- **Admin / Remove row** (admin only, never for self): **Admin** button
  toggles admin role, **Remove** button (confirm dialog) removes the device
  from the roster.
- **Hints row** (admin only, not self): "Address hints" text box
  (host or host:port, comma/space separated) + **Save**.
- **Addresses** section: MagicDNS name, VPN IP, Device ID (wrapped, with
  **Copy**).
- **Connectivity** section, a 3x3 grid of label/value pairs: Role ("This
  device, Admin, Shares internet" / "Member"), State (Online/Connecting/
  Offline/…), FIPS path ("Direct connection (TCP/UDP), N ms" / "Via mesh, N ms"
  / "This device"), Last seen, Latency age, Control seen, Data seen,
  Sent (bytes), Received (bytes). Below: Address hints line and a status
  detail line.
- Empty state when nothing is selectable: "No devices yet".

### 2.2 Add Network page
Same choice/create/join cards as Devices State A. Reached only from the
sidebar network switcher's "Add network" item. Leaving the page resets the
flow to the choice card.

### 2.3 Add Device page (admins only)
- **Link Device** card (shown when the network is enabled): explanation that
  pasting a joiner's request opens the confirmation automatically, a text box
  (typing or pasting anything starting with `nvpn://join-request/` triggers a
  confirm dialog "Add device?"), and an **Import join request QR file** button
  (decodes a PNG/JPG/etc. and validates it is a join request).
  On success: notice "Device added" and jump to Devices. After an add, the app
  refreshes every second for ~20 seconds so the new device appears fast.
- **For manual join** card (admins): my Device ID + **Copy admin Device ID**,
  the network's Network ID + **Copy Network ID**, a "Joining Device ID" field
  with validation, an optional "Name" field, and **Add by Device ID**.

### 2.4 Internet page
- **Internet Source** card. Status line first. Then radio-style full-width
  rows (current choice marked with a filled dot):
  - **This device** — "Use this device's normal internet".
  - **WireGuard upstream** — subtitle shows the config's endpoint or "No
    WireGuard config saved yet"; disabled until a config exists.
  - **Paid · Automatic · Experimental** (only if the market is supported) —
    "choose a working, reasonably priced provider". When a provider is active
    and connected, a 👍/👎 rating row and **Try another** button appear.
  - **Paid · Choose manually** (market supported) — jumps to the Public Exits
    page.
  - One row per network participant that offers an exit, with their status.
- **Sharing card**:
  - Toggle **"Share this device's internet"** (advertise this device as an
    exit node).
  - Toggle **"Block internet if source disconnects"** (leak protection,
    default on).
  - Button **"Sell internet access · Experimental"** (only when selling is
    supported) → Sell Access page.
- **WireGuard Upstream** card: explanation, a monospace multi-line editor for
  a full WireGuard config, **Import File** (.conf/.txt) and **Save WireGuard**.
- **Exit DNS** card: "MagicDNS stays local. Public DNS follows this policy
  while an internet exit is active."
  - Mode combo: Automatic (recommended) / Encrypted DNS / DNS through exit.
  - Encrypted → provider combo: Cloudflare / Quad9 / Custom DoH; custom shows
    a DoH URL field and a bootstrap-IPs field.
  - Through-exit → a comma-separated DNS server IPs field.
  - **Save Exit DNS** button.

### 2.5 Public Exits page (paid market; "Find Internet to Buy")
- Top status line (current internet source status).
- Card:
  - Right-aligned **Find** (discover sellers) and **Pay** (stream payments for
    usage; enabled only when something is payable) buttons. Title "Find
    Internet to Buy", subtitle "Experimental", "Wallet <balance>".
  - Market status line.
  - **Manual provider** row: text box for a provider npub or paid-exit link +
    **Add provider** / **Clear** (Clear only when one is set), and its status
    line.
  - **Filter row**: country code (2 letters), sort combo (Quality / Price /
    Newest), IPv4 and IPv6 checkboxes, **Clear filters** and **Apply**.
    "N hidden by filters" line when filters hide offers.
  - Last payment action status line, plus a **Send payment** button when a
    signed payment envelope is waiting.
  - **Available** list: one row per offer — country, price, rating score,
    👍/👎 (toggle own rating; negative rating stops the provider), quality/
    bandwidth metrics line, and a **Connect** button.
  - **Your Paid Internet** list: one row per session — title ("Ready",
    "Payment needed", "Needs funds", …), detail line (access state, usage,
    amount due), location line (exit IP + verified/country-claim status),
    paid/behind amounts, metrics, settlement line. Buttons per row:
    **Connect**, **Probe** (quality check), **Fund** (open payment channel),
    **Pay** (sign payment), **Settle** (close channel) — enabled by session
    state.

### 2.6 Wallet page (paid market supported)
- Intro: "Pay for internet access and receive earnings when you sell
  bandwidth."
- Card:
  - **Refresh** button; large balance, secondary fiat balance, exchange-rate
    line ("rate · sources"), last-action status ("Invoice ready", "Token
    imported", …).
  - **Mint URL** row: text box + **Add**.
  - Tabs:
    - **Receive**: Lightning — amount field + **Create Invoice** (needs a
      mint; 2-second refresh while an invoice is pending). Token — paste field
      (auto-preview when it looks like a Cashu token) + **Scan QR** (pick an
      image file, decode, validate it is a Cashu token). Both go through a
      preview dialog (amount, mint, memo, status) and a **Redeem this token?**
      confirm.
    - **Send**: Lightning — invoice field + **Pay**. Token — amount + 
      **Export**, producing a token to copy.
  - Result sections (each with a Copy button): **Invoice** (QR + expiry time +
    string), **Token**, **Preimage**.
  - **Mints** list: "default · URL · balance" / "mint · URL · balance" lines.

### 2.7 Sell Access page (seller supported; "Share My Internet")
- Status line (e.g. "People can pay to use my internet").
- When selling is available:
  - Toggle **Share my internet** (the listing on/off).
  - Pending buyer credit line + help text ("Collect to move it into wallet").
  - Payments status line.
  - Summary line: "Country · price".
  - **Price row**: msat/GB text box + **Save price**.
  - Read-only **provider link** + **Copy link**.
  - Info lines: free test / grace amounts, channel expiry, settlement, public
    IP, accepted mints, and a totals line ("N connected · N past · traffic ·
    paid · due · behind").
  - **Active Customers** list: per-session title, detail, paid, behind,
    metrics, settlement.

### 2.8 Settings page
Stack of cards:
- **This Device**: Name, Tunnel IP, Endpoint, Listen Port fields + **Save**
  (one patch saves all, plus the FIPS inbound ports below).
- **General**: toggles — Start VPN automatically, Launch on startup,
  Tray on close.
- **Appearance**: radio System / Light / Dark (live theme switch).
- **Wallet** (market supported): toggle Show fiat value; currency combo
  USD/EUR/GBP/CAD/AUD/JPY/CHF; note "Rates from Coinbase and Kraken".
- **Relays**: relay URL box + **Add** (wss:// prepended if missing); list rows
  with a green/gray connected dot, enable checkbox, **Delete**.
- **FIPS**: toggles — Connect to non-roster FIPS peers, Find peers over Nostr
  relays, Use bootstrap servers.
- **Public FIPS routing**: explainer + "Learn FIPS" hyperlink; toggle "Keep
  npub.fips outside the tunnel"; read-only "Your public FIPS address"
  (`<npub>.fips`); "Public .fips inbound TCP ports" field; **Save** (shares the
  This Device save action).
- **Networks**: name box + **Add**; for the active network — Name field +
  **Save** (admin only), Network ID field (grouped in 4s by dashes) +
  **Copy** / **Save** (admin only); **Remove this network** (confirm dialog);
  list of inactive networks with **Activate** / **Remove**.
- **System**: version label ("v1.2.3" or "gui vX · daemon vY"); "Service
  installed/missing", "CLI installed/missing", update status; buttons —
  Install service, Enable, Disable, Install CLI, Updates.
- **Diagnostics** (expander): Interface, Gateway, IPv4, IPv6, Mapping
  (UPnP/NAT-PMP/PCP or "none"), External endpoint, Remote roster
  "connected/expected", Roster FIPS "direct count", Other FIPS count; then a
  health-issues list (severity, summary, detail).

---

## 3. Non-page functionality that must survive the rebuild

- **Tray icon** (`TrayService`): tooltip "Nostr VPN - <status>"; icon gets a
  red dot when the internet source is blocked. Double-click opens the window.
  Menu: **Nostr VPN** toggle (checkmark, with a disabled status subtitle row),
  device display name, **Copy Device ID**, **Network Devices** submenu (each
  device → copy ID), **Internet Source** submenu (status row, Share This
  Device toggle, This device, Paid·Automatic, Paid·Choose manually, WireGuard
  VPN, every peer exit — checkmarks on the active one), **Open Nostr VPN**,
  **Quit**.
- **Deep link** `nvpn://join-request/…`: single-instance handoff; if the local
  device is an admin, shows the "Add device?" confirmation and imports.
  (Debug builds also accept `nvpn://debug/accept-join`.)
- **Close-to-tray**: closing the window hides it when "Tray on close" is set.
- **Polling**: full state refresh every 15s; every 1s for ~20s after join/add
  actions; every 2s while a wallet top-up invoice is pending.
- **Single action at a time**: every mutating command is gated by an in-flight
  flag; buttons disable while an action runs. Status notices are delayed 250ms
  so fast actions don't flash the layout.
- **Theme**: Light/Dark.xaml resource dictionaries swapped at runtime;
  preference persisted by ThemeService.
- **UI automation IDs** used by tests must be kept (names may change):
  `ExitDnsInternetNavigation`, `ManualJoinCreateNetworkChoice`,
  `ManualJoinCreateNetworkName`, `ManualJoinCreateNetworkSubmit`,
  `ManualJoinChooseJoin`, `ManualJoinExpander`, `ManualJoinJoinerDeviceIdValue`,
  `ManualJoinAdminId`, `ManualJoinNetworkId`, `ManualJoinSubmit`,
  `ManualJoinAdminOpen`, `ManualJoinAdminDeviceIdValue`,
  `ManualJoinAdminNetworkIdValue`, `ManualJoinAdminDeviceId`,
  `ManualJoinAdminDeviceName`, `ManualJoinAdminSubmit`,
  `RosterParticipantAccepted-<npub>` (per accepted roster row),
  `InternetSourceStatus`, `ExitDnsMode`, `ExitDnsProvider`, `ExitDnsCustomUrl`,
  `ExitDnsBootstrapIps`, `ExitDnsThroughServers`, `ExitDnsSave`,
  `PaidExitCurrentInternet`, `ManualPaidExitProvider`,
  `PaidRouteFilterCountry`, `PaidRouteFilterSort`, `PaidExitPriceMsatPerGb`,
  `ThemeSystemOption`, `ThemeLightOption`, `ThemeDarkOption`.

---

## 4. Proposed rebuild split: normal UI vs Pro settings

Goal: a default UI a non-Nostr person can use, with everything expert-y moved
behind a **Pro mode** switch in Settings (hidden until enabled; nothing is
deleted, only relocated).

### Normal UI (default)

**Home** (replaces the current header + Devices):
- Big connect toggle with plain status ("Protected", "Connecting…", "Off",
  or the error), network name, "X of Y devices connected".
- **Internet source** summary chip with a simple picker: "My internet",
  each peer that shares internet ("through <name>"), and — if the market is
  supported — "Paid internet (automatic)". Picking "Paid internet" opens one
  card with: balance, Find/Connect, current provider + 👍/👎 + Try another.
  No filters, no manual provider link, no channel buttons (they move to Pro or
  become automatic).
- Devices list (simplified rows: name, online dot, IP, Copy ID) and a detail
  view with just: name (rename for admins), Make admin / Remove for admins,
  device ID + copy.
- First-run wizard: "Create a network" / "Join a network" cards, exactly the
  current flows, before anything else is reachable.
- Global: update banner, notice area, tray as today.

**Settings (normal)**:
- Appearance (System/Light/Dark), Launch on startup, Tray on close,
  Start VPN automatically, "Block internet if source disconnects",
  "Share this device's internet", Check for updates + Install automatically.
- "Join my other networks": list of inactive networks with Activate, and
  Create/Join entry points.

### Pro settings (only when Pro mode is on)

- **Device**: Name, Tunnel IP, Endpoint, Listen Port.
- **Networks**: rename, edit Network ID, Copy/Save, Remove; add by name.
- **Internet (advanced)**: WireGuard upstream editor + import; Exit DNS mode/
  provider/custom DoH/bootstrap IPs/through-exit servers; manual paid provider
  link; market filters (country/sort/IPv4/IPv6); per-session Fund / Pay /
  Settle / Probe buttons; payment envelope "Send payment"; "Stream payments".
- **Sell internet**: listing toggle, price msat/GB, provider link, accepted
  mints, trial/grace, channel expiry, settlement, public IP, active customers.
- **Wallet**: mints (add/list), Receive/Send tabs (Lightning + Cashu tokens,
  QR scan), invoice/token/preimage results.
- **Relays**: add/enable/delete.
- **FIPS**: non-roster peers, Nostr discovery, bootstrap servers, public FIPS
  routing (npub.fips outside tunnel, inbound TCP ports).
- **System**: install/enable/disable service, install CLI, version labels,
  Diagnostics (interface/gateway/IPv4/IPv6/mapping/external/roster/FIPS) and
  the health-issues list.

### Suggested structure for the rebuild

- Sidebar: Home, Devices, Internet, (Wallet), Settings — same as today; the
  Pro cards live inside Settings as collapsible sections or a second
  "Advanced" sidebar group that appears with Pro mode.
- Keep one page visible at a time (today all eight pages are stacked in one
  ScrollViewer with visibility converters — replacing that with real per-page
  views is the single biggest maintainability win).
- Keep all commands, validation, AutomationIds, polling behavior, deep links,
  and tray behavior listed in sections 2–3 unchanged; only presentation moves.
