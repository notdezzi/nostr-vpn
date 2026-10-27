// Nostr VPN Windows GUI entry point: single-instance + deep links, core
// worker pumping, adaptive refresh, tray, updater loop, and every Slint
// callback wired to a typed core action. Port of the WPF App/MainWindow
// code-behind plus AppViewModel commands.

mod bridge;
mod deep_link;
mod display;
mod e2e;
mod platform;
mod qr;
mod updater;
mod worker;

slint::include_modules!();

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use nostr_vpn_app_core::{NativeAppAction, NativeAppState};
use slint::{CloseRequestResponse, ComponentHandle, Image, SharedString};

use crate::bridge::Bridge;
use crate::updater::UpdateState;

const IDLE_REFRESH: Duration = Duration::from_secs(15);
const JOIN_REFRESH: Duration = Duration::from_secs(1);
const JOIN_WINDOW: Duration = Duration::from_secs(20);
const TOPUP_REFRESH: Duration = Duration::from_secs(2);
const NOTICE_DELAY: Duration = Duration::from_millis(250);
const DRAIN_POLL: Duration = Duration::from_millis(50);

enum Pending {
    None,
    CreateNetwork,
    ManualAddNetwork,
    AddParticipant { participant: String },
    ImportJoin,
    PreviewToken { token: String },
    RedeemToken,
    ClearProvider,
    ApplyFilter,
}

struct Ctx {
    requests: worker::RequestSender,
    state: Option<NativeAppState>,
    bridge: Bridge,
    action_in_flight: bool,
    join_until: Option<Instant>,
    pending: Pending,
    notice_pending: Option<String>,
    notice_shown: bool,
    update: UpdateState,
    update_tx: std::sync::mpsc::Sender<updater::UpdateEvent>,
    config_path: String,
    quitting: bool,
    startup_synced: bool,
    tray_normal: Image,
    tray_blocked: Image,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    platform::trace_roster_e2e(&format!("startup args: {args:?}"));
    let single = match platform::claim_or_signal(&args) {
        Ok(single) => single,
        Err(()) => {
            platform::trace_roster_e2e("signaled existing instance");
            return;
        }
    };
    if e2e::is_requested(&args) {
        std::process::exit(e2e::run(&args));
    }
    run(args, single);
}

fn run(args: Vec<String>, single: platform::SingleInstance) {
    let version = env!("CARGO_PKG_VERSION").to_string();
    let data_dir = platform::data_dir();
    if let Err(error) = std::fs::create_dir_all(&data_dir) {
        eprintln!("could not create data dir: {error}");
    }
    let config_path = data_dir.join("config.toml");
    let config = config_path.to_string_lossy().into_owned();

    let (requests, results) = worker::spawn(data_dir.to_string_lossy().into_owned(), version);
    let (update_tx, update_rx) = std::sync::mpsc::channel();
    let (link_tx, link_rx) = std::sync::mpsc::channel::<Vec<String>>();

    let app = AppWindow::new().expect("could not create the main window");
    let tray = NostrVpnTray::new().expect("could not create the tray icon");
    let about = AboutDialog::new().expect("could not create the about window");

    let (tray_normal, tray_blocked) = tray_images();
    tray.set_tray_icon(tray_normal.clone());

    app.global::<App>().set_theme_pref(platform::load_theme_pref());
    platform::register_deep_link_protocol();

    let ctx = Rc::new(RefCell::new(Ctx {
        requests: requests.clone(),
        state: None,
        bridge: Bridge::new(),
        action_in_flight: false,
        join_until: None,
        pending: Pending::None,
        notice_pending: None,
        notice_shown: false,
        update: UpdateState {
            auto_install: platform::load_auto_install_updates(),
            ..UpdateState::default()
        },
        update_tx,
        config_path: config.clone(),
        quitting: false,
        startup_synced: false,
        tray_normal,
        tray_blocked,
    }));

    let app_weak = app.as_weak();
    let tray_weak = tray.as_weak();
    let about_weak = about.as_weak();

    // Secondary instances forward their launch args over the named pipe.
    // Rc state can't cross threads, so the listener only relays the args;
    // the drain timer picks them up on the UI thread.
    {
        let link_tx = link_tx.clone();
        std::thread::spawn(move || {
            single.listen(move |launch_args| {
                platform::trace_roster_e2e(&format!("pipe launch args: {launch_args:?}"));
                let _ = link_tx.send(launch_args);
            });
        });
    }

    let notice_timer = Rc::new(slint::Timer::default());
    {
        // Repeating checker implements the WPF deferred notice: the in-progress
        // status is shown only once an action has been running for 250ms.
        let ctx = ctx.clone();
        let app = app_weak.clone();
        notice_timer.start(
            slint::TimerMode::Repeated,
            NOTICE_DELAY,
            move || {
                let Some(app) = app.upgrade() else { return };
                let ctx = ctx.borrow();
                if ctx.action_in_flight && !ctx.notice_shown {
                    if let Some(text) = &ctx.notice_pending {
                        set_notice(&app, text);
                    }
                }
            },
        );
    }

    let refresh_timer = Rc::new(slint::Timer::default());
    {
        let ctx = ctx.clone();
        refresh_timer.start(slint::TimerMode::Repeated, IDLE_REFRESH, move || {
            if !ctx.borrow().action_in_flight {
                let _ = ctx.borrow().requests.send(None);
            }
        });
    }

    // Drain core results and updater events onto the UI thread. Both timers
    // live in run()'s frame, which persists until app.run() returns.
    let drain_timer = slint::Timer::default();
    {
        let ctx = ctx.clone();
        let app = app_weak.clone();
        let tray = tray_weak.clone();
        let refresh_timer = refresh_timer.clone();
        let update_rx = update_rx;
        drain_timer.start(
            slint::TimerMode::Repeated,
            DRAIN_POLL,
            move || {
                let Some(app) = app.upgrade() else { return };
                let Some(tray) = tray.upgrade() else { return };
                while let Ok(launch_args) = link_rx.try_recv() {
                    handle_launch_args(&ctx, &app, &tray, &launch_args, true);
                }
                loop {
                    match results.try_recv() {
                        Ok(result) => handle_result(&ctx, &app, &tray, &refresh_timer, result),
                        Err(_) => break,
                    }
                }
                loop {
                    match update_rx.try_recv() {
                        Ok(event) => handle_update_event(&ctx, &app, event),
                        Err(_) => break,
                    }
                }
            },
        );
    }

    // Updater: initial check plus the periodic poll (default 6 hours).
    let poll_timer = slint::Timer::default();
    {
        let ctx = ctx.clone();
        start_check(&ctx, false);
        let poll = Duration::from_secs(updater::poll_interval_secs().max(1));
        poll_timer.start(slint::TimerMode::Repeated, poll, move || {
            start_check(&ctx, false);
        });
    }

    {
        let ctx = ctx.clone();
        app.window().on_close_requested(move || {
            let close_to_tray = ctx
                .borrow()
                .state
                .as_ref()
                .map(|state| state.close_to_tray_on_close)
                .unwrap_or(false);
            if ctx.borrow().quitting || !close_to_tray {
                ctx.borrow_mut().quitting = true;
                slint::quit_event_loop().ok();
            }
            CloseRequestResponse::HideWindow
        });
    }

    about.on_closed({
        let about = about_weak.clone();
        move || {
            if let Some(about) = about.upgrade() {
                about.hide().ok();
            }
        }
    });

    wire_callbacks(&ctx, &app, &tray, &about);

    handle_launch_args(&ctx, &app, &tray, &args, false);
    if !args.iter().any(|arg| arg.eq_ignore_ascii_case("--hidden")) {
        show_main(&app);
    }

    platform::trace_roster_e2e("view ready");
    app.run().ok();
}

// ---------- shared helpers ----------

fn set_notice(app: &AppWindow, text: &str) {
    let g = app.global::<App>();
    g.set_notice(text.into());
    g.set_notice_visible(!text.trim().is_empty());
}

fn dispatch(
    ctx: &Rc<RefCell<Ctx>>,
    action: NativeAppAction,
    status: &'static str,
) -> bool {
    {
        let mut ctx = ctx.borrow_mut();
        if ctx.action_in_flight {
            return false;
        }
        ctx.action_in_flight = true;
        ctx.notice_pending = Some(status.to_string());
        ctx.notice_shown = false;
    }
    let requests = ctx.borrow().requests.clone();
    let _ = requests.send(Some(action));
    true
}

fn reapply(ctx: &Rc<RefCell<Ctx>>, app: &AppWindow, tray: &NostrVpnTray, sync: bool) {
    let (state, in_flight) = {
        let ctx = ctx.borrow();
        (ctx.state.clone(), ctx.action_in_flight)
    };
    let Some(state) = state else { return };
    let mut ctx = ctx.borrow_mut();
    ctx.bridge
        .apply(&app.global::<App>(), &state, sync, in_flight);
    bridge::apply_tray(tray, &state, &ctx.bridge, in_flight);
}

fn set_page(ctx: &Rc<RefCell<Ctx>>, app: &AppWindow, target: i32) {
    let g = app.global::<App>();
    let visible = {
        let ctx = ctx.borrow();
        ctx.bridge.page_is_visible(target, ctx.state.as_ref())
    };
    let next = if visible { target } else { bridge::PAGE_DEVICES };
    {
        let mut ctx = ctx.borrow_mut();
        if next == bridge::PAGE_ADD_NETWORK
            || (g.get_page() == bridge::PAGE_ADD_NETWORK && next != bridge::PAGE_ADD_NETWORK)
        {
            // Entering or leaving Add Network resets the create/join mode.
            ctx.bridge.setup_mode = 0;
        }
    }
    g.set_setup_mode(ctx.borrow().bridge.setup_mode);
    g.set_page(next);
}

fn show_main(app: &AppWindow) {
    app.window().set_minimized(false);
    app.show().ok();
}

fn confirm(message: &str, title: &str) -> bool {
    rfd::MessageDialog::new()
        .set_title(title)
        .set_description(message)
        .set_buttons(rfd::MessageButtons::YesNo)
        .show()
        == rfd::MessageDialogResult::Yes
}

fn info(message: &str, title: &str) {
    rfd::MessageDialog::new()
        .set_title(title)
        .set_description(message)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

fn copy_text(_ctx: &Rc<RefCell<Ctx>>, app: &AppWindow, value: &str) {
    if value.trim().is_empty() {
        return;
    }
    match platform::copy_to_clipboard(value) {
        Ok(()) => set_notice(app, "Copied"),
        Err(error) => set_notice(app, &error),
    }
}

fn normalize_relay_url(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_lowercase();
    Some(if lower.starts_with("ws://") || lower.starts_with("wss://") {
        trimmed.to_string()
    } else {
        format!("wss://{trimmed}")
    })
}

fn relay_lists(state: &NativeAppState) -> (Vec<String>, Vec<String>) {
    let unique = |values: Vec<String>| -> Vec<String> {
        let mut seen = Vec::new();
        for value in values {
            if let Some(url) = normalize_relay_url(&value) {
                if !seen.contains(&url) {
                    seen.push(url);
                }
            }
        }
        seen
    };
    let enabled = unique(
        state
            .relays
            .iter()
            .filter(|relay| relay.enabled)
            .map(|relay| relay.url.clone())
            .collect(),
    );
    let mut disabled = unique(
        state
            .relays
            .iter()
            .filter(|relay| !relay.enabled)
            .map(|relay| relay.url.clone())
            .collect(),
    );
    disabled.retain(|relay| !enabled.contains(relay));
    (enabled, disabled)
}

fn current_rating(state: &NativeAppState, seller: &str) -> Option<i64> {
    let market = &state.paid_route_market;
    let offers = if market.hidden_offer_count > 0 || !market.visible_offers.is_empty() {
        &market.visible_offers
    } else {
        &market.offers
    };
    offers
        .iter()
        .find(|offer| offer.seller_npub == seller)
        .map(|offer| offer.personal_rating)
        .or_else(|| {
            market
                .sessions
                .iter()
                .find(|session| session.seller_npub == seller)
                .map(|session| session.personal_rating)
        })
}

fn parse_positive_u64(value: &str) -> Option<u64> {
    value.trim().parse::<u64>().ok().filter(|parsed| *parsed > 0)
}

// ---------- core result / pending follow-ups ----------

fn handle_result(
    ctx: &Rc<RefCell<Ctx>>,
    app: &AppWindow,
    tray: &NostrVpnTray,
    refresh_timer: &slint::Timer,
    result: worker::CoreResult,
) {
    let g = app.global::<App>();
    {
        let mut ctx = ctx.borrow_mut();
        if result.is_action {
            ctx.action_in_flight = false;
            ctx.notice_pending = None;
        }
        ctx.state = Some(result.state.clone());
    }
    g.set_action_in_flight(ctx.borrow().action_in_flight);
    {
        let mut ctx = ctx.borrow_mut();
        let in_flight = ctx.action_in_flight;
        ctx.bridge.apply(&g, &result.state, result.is_action, in_flight);
        bridge::apply_tray(tray, &result.state, &ctx.bridge, in_flight);
    }
    tray.set_tray_icon(if result.state.exit_node_blocked {
        ctx.borrow().tray_blocked.clone()
    } else {
        ctx.borrow().tray_normal.clone()
    });
    if result.is_action {
        set_notice(app, &result.state.error);
    }
    if !ctx.borrow().startup_synced {
        ctx.borrow_mut().startup_synced = true;
        platform::sync_launch_on_startup(
            result.state.startup_settings_supported,
            result.state.launch_on_startup,
        );
    }

    handle_pending(ctx, app, tray, &result.state);

    let interval = {
        let ctx = ctx.borrow();
        if ctx.join_until.is_some_and(|until| Instant::now() < until) {
            JOIN_REFRESH
        } else if result.state.paid_route_market.wallet.last_action.kind == "topup" {
            TOPUP_REFRESH
        } else {
            IDLE_REFRESH
        }
    };
    let refresh_ctx = ctx.clone();
    refresh_timer.start(
        slint::TimerMode::Repeated,
        interval,
        move || {
            if !refresh_ctx.borrow().action_in_flight {
                let _ = refresh_ctx.borrow().requests.send(None);
            }
        },
    );
}

fn handle_pending(
    ctx: &Rc<RefCell<Ctx>>,
    app: &AppWindow,
    tray: &NostrVpnTray,
    state: &NativeAppState,
) {
    let g = app.global::<App>();
    let pending = std::mem::replace(&mut ctx.borrow_mut().pending, Pending::None);
    match pending {
        Pending::None => {}
        Pending::CreateNetwork => {
            if !state.error.is_empty() {
                return;
            }
            if let Some(network) = state.networks.iter().find(|network| network.enabled) {
                ctx.borrow_mut().bridge.shown_network_id = network.id.clone();
            }
            g.set_new_network_name(SharedString::new());
            ctx.borrow_mut().bridge.setup_mode = 0;
            g.set_setup_mode(0);
            g.set_page(bridge::PAGE_DEVICES);
            reapply(ctx, app, tray, true);
        }
        Pending::ManualAddNetwork => {
            if !state.error.is_empty() {
                return;
            }
            ctx.borrow_mut().join_until = Some(Instant::now() + JOIN_WINDOW);
            if let Some(network) = state.networks.iter().find(|network| network.enabled) {
                ctx.borrow_mut().bridge.shown_network_id = network.id.clone();
            }
            g.set_manual_join_admin(SharedString::new());
            g.set_manual_join_mesh(SharedString::new());
            ctx.borrow_mut().bridge.setup_mode = 0;
            g.set_setup_mode(0);
            g.set_page(bridge::PAGE_DEVICES);
            reapply(ctx, app, tray, true);
        }
        Pending::AddParticipant { participant } => {
            if !state.error.is_empty() {
                return;
            }
            ctx.borrow_mut().join_until = Some(Instant::now() + JOIN_WINDOW);
            let accepted = state.networks.iter().any(|network| {
                network
                    .participants
                    .iter()
                    .any(|item| item.roster_accepted && item.npub == participant)
            });
            if accepted {
                g.set_participant_input(SharedString::new());
                g.set_participant_alias_input(SharedString::new());
                g.set_page(bridge::PAGE_DEVICES);
                set_notice(app, "Device added");
            }
        }
        Pending::ImportJoin => {
            ctx.borrow_mut().bridge.join_request_prompt_open = false;
            if !state.error.is_empty() {
                return;
            }
            g.set_join_request_input(SharedString::new());
            g.set_page(bridge::PAGE_DEVICES);
            set_notice(app, "Device added");
        }
        Pending::PreviewToken { token } => {
            if !state.error.is_empty() || state.paid_route_market.wallet.last_action.kind != "preview" {
                return;
            }
            let action = &state.paid_route_market.wallet.last_action;
            let memo = if action.token_memo.trim().is_empty() {
                String::new()
            } else {
                format!("\nMemo: {}", action.token_memo)
            };
            let message = format!(
                "Amount: {}\nMint: {}{memo}\nStatus: {}",
                action.amount_text, action.mint_url, action.status_text
            );
            if !action.token_redeemable {
                info(&message, "Cashu token");
                return;
            }
            if confirm(&format!("{message}\n\nRedeem this token?"), "Redeem token?") {
                ctx.borrow_mut().pending = Pending::RedeemToken;
                if !dispatch(
                    ctx,
                    NativeAppAction::ReceivePaidRouteWalletToken { token },
                    "Redeeming token",
                ) {
                    ctx.borrow_mut().pending = Pending::None;
                }
            }
        }
        Pending::RedeemToken => {
            if !state.error.is_empty() {
                info(&state.error, "Could not redeem token");
            }
        }
        Pending::ClearProvider => {
            g.set_manual_provider(SharedString::new());
        }
        Pending::ApplyFilter => {
            let mut ctx = ctx.borrow_mut();
            ctx.bridge.force_sync_filter(&g, state);
        }
    }
}

// ---------- updater ----------

fn apply_update(ctx: &Ctx, app: &AppWindow) {
    let g = app.global::<App>();
    let update = &ctx.update;
    g.set_update_visible(update.available);
    let app_version = ctx
        .state
        .as_ref()
        .map(|state| state.app_version.trim().to_string())
        .unwrap_or_default();
    g.set_update_stripe_text(if app_version.is_empty() {
        format!("Update available: {}", update.version)
    } else {
        format!(
            "Update available: {} (you're on {app_version})",
            update.version
        )
    }
    .into());
    g.set_update_install_enabled(
        update.available && update.installable && !update.checking && !update.installing,
    );
    g.set_auto_install_updates(update.auto_install);
    g.set_update_status(update.status.clone().into());
}

fn start_check(ctx: &Rc<RefCell<Ctx>>, manual: bool) {
    let (version, config, tx) = {
        let mut ctx = ctx.borrow_mut();
        if ctx.update.checking || ctx.update.installing {
            return;
        }
        ctx.update.checking = true;
        if manual {
            ctx.update.status = "Checking for updates".to_string();
        }
        (
            env!("CARGO_PKG_VERSION").to_string(),
            ctx.config_path.clone(),
            ctx.update_tx.clone(),
        )
    };
    updater::check(version, config, manual, tx);
}

fn start_download(ctx: &Rc<RefCell<Ctx>>) {
    let (version, config, tx, asset) = {
        let mut ctx = ctx.borrow_mut();
        if !ctx.update.installable || ctx.update.installing || ctx.update.checking {
            return;
        }
        let Some(asset) = ctx.update.asset.clone() else {
            return;
        };
        ctx.update.installing = true;
        ctx.update.status = format!("Downloading {}", ctx.update.version);
        (
            ctx.update.version.clone(),
            ctx.config_path.clone(),
            ctx.update_tx.clone(),
            asset,
        )
    };
    updater::download(version, asset, config, tx);
}

fn handle_update_event(ctx: &Rc<RefCell<Ctx>>, app: &AppWindow, event: updater::UpdateEvent) {
    match event {
        updater::UpdateEvent::Checked { manual, result } => {
            ctx.borrow_mut().update.checking = false;
            match result {
                Ok(check) => {
                    {
                        let mut ctx = ctx.borrow_mut();
                        ctx.update.available = check.available;
                        ctx.update.version = check.tag;
                        ctx.update.installable = check.installable;
                        ctx.update.asset = check.asset;
                        if check.available {
                            ctx.update.status = check.message;
                        } else if manual {
                            ctx.update.status = check.message;
                        } else {
                            ctx.update.status = String::new();
                        }
                    }
                    let should_auto = ctx.borrow().update.auto_install && ctx.borrow().update.installable;
                    apply_update(&ctx.borrow(), app);
                    if check.available && should_auto {
                        start_download(ctx);
                    }
                }
                Err(error) => {
                    {
                        let mut ctx = ctx.borrow_mut();
                        ctx.update.asset = None;
                        ctx.update.installable = false;
                        if manual {
                            ctx.update.status = error;
                        }
                    }
                    apply_update(&ctx.borrow(), app);
                }
            }
        }
        updater::UpdateEvent::Downloaded(result) => {
            ctx.borrow_mut().update.installing = false;
            match result {
                Ok(path) => {
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.to_string_lossy().into_owned());
                    ctx.borrow_mut().update.status = format!("Downloaded {name}");
                    apply_update(&ctx.borrow(), app);
                    if !updater::skip_open() {
                        platform::shell_open(&path.to_string_lossy());
                    }
                }
                Err(error) => {
                    ctx.borrow_mut().update.status = error;
                    apply_update(&ctx.borrow(), app);
                }
            }
        }
    }
}

// ---------- deep links ----------

fn handle_launch_args(
    ctx: &Rc<RefCell<Ctx>>,
    app: &AppWindow,
    tray: &NostrVpnTray,
    args: &[String],
    force_show: bool,
) {
    for arg in args {
        if arg.get(..7).is_some_and(|prefix| prefix.eq_ignore_ascii_case("nvpn://")) {
            handle_deep_link(ctx, app, tray, arg);
        }
    }
    if force_show && !args.iter().any(|arg| arg.eq_ignore_ascii_case("--hidden")) {
        show_main(app);
    }
}

fn handle_deep_link(
    ctx: &Rc<RefCell<Ctx>>,
    app: &AppWindow,
    tray: &NostrVpnTray,
    url: &str,
) {
    platform::trace_roster_e2e(&format!("received {url}"));
    match deep_link::parse(url) {
        Some(deep_link::DeepLink::JoinRequest(request)) => {
            confirm_and_import(ctx, app, tray, &request);
        }
        #[cfg(debug_assertions)]
        Some(deep_link::DeepLink::Debug(action)) => match action {
            deep_link::DebugAction::Tick => {
                let _ = ctx.borrow().requests.send(None);
            }
            deep_link::DebugAction::AcceptJoin {
                network_id,
                requester_npub,
            } => {
                let requested = network_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|id| !id.is_empty());
                let resolved = {
                    let ctx = ctx.borrow();
                    let Some(state) = ctx.state.as_ref() else {
                        return;
                    };
                    let network = state.networks.iter().find(|network| {
                        requested
                            .map(|id| network.id == id || network.network_id == id)
                            .unwrap_or(true)
                    });
                    let Some(network) = network else {
                        platform::trace_roster_e2e(&format!(
                            "network not found: {}",
                            requested.unwrap_or("")
                        ));
                        return;
                    };
                    let network_id = network.id.clone();
                    let requester = requester_npub
                        .clone()
                        .filter(|value| !value.trim().is_empty())
                        .or_else(|| {
                            network
                                .inbound_join_requests
                                .first()
                                .map(|request| request.requester_npub.clone())
                        })
                        .filter(|value| !value.trim().is_empty());
                    requester.map(|requester| (network_id, requester))
                };
                let Some((network_id, requester)) = resolved else {
                    return;
                };
                platform::trace_roster_e2e(&format!("dispatching accept for {network_id}"));
                dispatch(
                    ctx,
                    NativeAppAction::AcceptJoinRequest {
                        network_id,
                        requester_npub: requester,
                    },
                    "Adding device",
                );
            }
        },
        None => {}
    }
}

fn confirm_and_import(
    ctx: &Rc<RefCell<Ctx>>,
    app: &AppWindow,
    tray: &NostrVpnTray,
    request: &str,
) {
    let network_name = {
        let ctx = ctx.borrow();
        if ctx.bridge.join_request_prompt_open
            || !display::looks_like_join_request(request)
        {
            return;
        }
        let Some(state) = ctx.state.as_ref() else { return };
        let Some(network) = bridge::active_network(&ctx.bridge, state) else {
            return;
        };
        if !network.local_is_admin {
            return;
        }
        if network.name.trim().is_empty() {
            "this network".to_string()
        } else {
            network.name.clone()
        }
    };
    ctx.borrow_mut().bridge.join_request_prompt_open = true;
    if confirm(
        &format!("Add the device from this join request to {network_name}?"),
        "Add device?",
    ) {
        ctx.borrow_mut().pending = Pending::ImportJoin;
        if !dispatch(
            ctx,
            NativeAppAction::ImportJoinRequest {
                request: request.to_string(),
            },
            "Adding device",
        ) {
            ctx.borrow_mut().bridge.join_request_prompt_open = false;
            ctx.borrow_mut().pending = Pending::None;
        }
    } else {
        ctx.borrow_mut().bridge.join_request_prompt_open = false;
    }
    let _ = (app, tray);
}

// ---------- tray icons ----------

fn tray_images() -> (Image, Image) {
    let normal = image_from_png(include_bytes!("../assets/tray-24.png"));
    let mut blocked = image::load_from_memory(include_bytes!("../assets/tray-24.png"))
        .expect("embedded tray icon must decode")
        .to_rgba8();
    let (width, height) = blocked.dimensions();
    let diameter = ((width.min(height) / 3) as u32).max(6);
    let center_x = (width.saturating_sub(diameter) - 1) as i64 + diameter as i64 / 2;
    let center_y = 1 + diameter as i64 / 2;
    let radius = diameter as f64 / 2.0;
    for y in 0..height {
        for x in 0..width {
            let dx = x as f64 + 0.5 - center_x as f64;
            let dy = y as f64 + 0.5 - center_y as f64;
            if (dx * dx + dy * dy).sqrt() <= radius {
                blocked.put_pixel(x, y, image::Rgba([220, 38, 38, 255]));
            }
        }
    }
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        blocked.as_raw(),
        blocked.width(),
        blocked.height(),
    );
    (normal, Image::from_rgba8(buffer))
}

fn image_from_png(bytes: &[u8]) -> Image {
    let decoded = image::load_from_memory(bytes)
        .expect("embedded tray icon must decode")
        .to_rgba8();
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        decoded.as_raw(),
        decoded.width(),
        decoded.height(),
    );
    Image::from_rgba8(buffer)
}

// ---------- callback wiring ----------

// Pin the closure parameter types for `on_app!` bodies: without an expected
// signature, inference takes the constraints from the body and conflicts with
// the handle arguments.
fn invoke0<R>(
    body: impl FnOnce(&Rc<RefCell<Ctx>>, &AppWindow, &NostrVpnTray) -> R,
    ctx: &Rc<RefCell<Ctx>>,
    app: &AppWindow,
    tray: &NostrVpnTray,
) -> R {
    body(ctx, app, tray)
}

fn invoke1<A, R>(
    body: impl FnOnce(&Rc<RefCell<Ctx>>, &AppWindow, &NostrVpnTray, A) -> R,
    ctx: &Rc<RefCell<Ctx>>,
    app: &AppWindow,
    tray: &NostrVpnTray,
    a1: A,
) -> R {
    body(ctx, app, tray, a1)
}

fn invoke2<A, B, R>(
    body: impl FnOnce(&Rc<RefCell<Ctx>>, &AppWindow, &NostrVpnTray, A, B) -> R,
    ctx: &Rc<RefCell<Ctx>>,
    app: &AppWindow,
    tray: &NostrVpnTray,
    a1: A,
    a2: B,
) -> R {
    body(ctx, app, tray, a1, a2)
}

fn wire_callbacks(ctx: &Rc<RefCell<Ctx>>, app: &AppWindow, tray: &NostrVpnTray, about: &AboutDialog) {
    let g = app.global::<App>();

    // The body is a caller-token closure `|ctx, app, tray, args...| {...}`:
    // macro_rules hygiene keeps definition-side bindings invisible to the
    // body, so the handles must flow in as closure arguments.
    macro_rules! on_app {
        ($name:ident, || $body:expr) => {{
            let ctx = ctx.clone();
            let app = app.as_weak();
            let tray = tray.as_weak();
            g.$name(move || {
                let Some(app) = app.upgrade() else { return };
                let Some(tray) = tray.upgrade() else { return };
                #[allow(unused_variables)]
                invoke0($body, &ctx, &app, &tray);
            });
        }};
        ($name:ident, |$a1:ident : $t1:ty| $body:expr) => {{
            let ctx = ctx.clone();
            let app = app.as_weak();
            let tray = tray.as_weak();
            g.$name(move |$a1 : $t1| {
                let Some(app) = app.upgrade() else { return };
                let Some(tray) = tray.upgrade() else { return };
                #[allow(unused_variables)]
                invoke1($body, &ctx, &app, &tray, $a1);
            });
        }};
        ($name:ident, |$a1:ident : $t1:ty, $a2:ident : $t2:ty| $body:expr) => {{
            let ctx = ctx.clone();
            let app = app.as_weak();
            let tray = tray.as_weak();
            g.$name(move |$a1 : $t1, $a2 : $t2| {
                let Some(app) = app.upgrade() else { return };
                let Some(tray) = tray.upgrade() else { return };
                #[allow(unused_variables)]
                invoke2($body, &ctx, &app, &tray, $a1, $a2);
            });
        }};
    }

    // Navigation and shared
    on_app!(on_request_page, |target: i32| |ctx, app, tray, target| {
        set_page(&ctx, &app, target);
    });
    on_app!(on_toggle_vpn, || |ctx, app, tray| {
        toggle_vpn(&ctx);
    });
    on_app!(on_refresh_now, || |ctx, app, tray| {
        let _ = ctx.borrow().requests.send(None);
    });
    on_app!(on_copy_text, |value: SharedString| |ctx, app, tray, value| {
        copy_text(&ctx, &app, &value);
    });
    on_app!(on_open_url, |url: SharedString| |ctx, app, tray, url| {
        platform::shell_open(&url);
    });
    on_app!(on_select_shown_network, |id: SharedString| |ctx, app, tray, id| {
        ctx.borrow_mut().bridge.shown_network_id = id.to_string();
        reapply(&ctx, &app, tray, true);
    });
    on_app!(on_goto_add_network, || |ctx, app, tray| {
        ctx.borrow_mut().bridge.setup_mode = 0;
        set_page(&ctx, &app, bridge::PAGE_ADD_NETWORK);
    });

    // About lives in a second window, so its handler owns that weak handle.
    {
        let about = about.as_weak();
        g.on_request_about(move || {
            if let Some(about) = about.upgrade() {
                about.show().ok();
            }
        });
    }

    // Setup
    on_app!(on_setup_create, || |ctx, app, tray| {
        ctx.borrow_mut().bridge.setup_mode = 1;
        app.global::<App>().set_setup_mode(1);
    });
    on_app!(on_setup_join, || |ctx, app, tray| {
        ctx.borrow_mut().bridge.setup_mode = 2;
        app.global::<App>().set_setup_mode(2);
    });
    on_app!(on_setup_back, || |ctx, app, tray| {
        ctx.borrow_mut().bridge.setup_mode = 0;
        app.global::<App>().set_setup_mode(0);
    });
    on_app!(on_create_network, || |ctx, app, tray| {
        let name = app.global::<App>().get_new_network_name().trim().to_string();
        let name = if name.is_empty() { "My Network".to_string() } else { name };
        ctx.borrow_mut().pending = Pending::CreateNetwork;
        if !dispatch(
            &ctx,
            NativeAppAction::AddNetwork { name },
            "Creating network",
        ) {
            ctx.borrow_mut().pending = Pending::None;
        }
    });
    on_app!(on_manual_add_network, || |ctx, app, tray| {
        let g = app.global::<App>();
        let admin = g.get_manual_join_admin().trim().to_string();
        let mesh = display::normalize_network_id_input(&g.get_manual_join_mesh());
        if !display::is_valid_device_id(&admin) || mesh.trim().is_empty() {
            return;
        }
        ctx.borrow_mut().pending = Pending::ManualAddNetwork;
        if !dispatch(
            &ctx,
            NativeAppAction::ManualAddNetwork {
                admin_npub: admin,
                mesh_network_id: mesh,
            },
            "Adding network",
        ) {
            ctx.borrow_mut().pending = Pending::None;
        }
    });
    on_app!(on_import_join_qr_file, || |ctx, app, tray| {
        let Some(path) = platform::open_file_dialog(
            "Import join request",
            &[
                ("Images", &["png", "jpg", "jpeg", "bmp", "gif"]),
                ("All files", &["*"]),
            ],
        ) else {
            return;
        };
        match qr::decode_file(&path.to_string_lossy()) {
            Err(error) => set_notice(&app, &error),
            Ok(value) => {
                let value = value.trim().to_string();
                if display::looks_like_join_request(&value) {
                    confirm_and_import(&ctx, &app, tray, &value);
                } else {
                    set_notice(&app, "Not a Nostr VPN join request.");
                }
            }
        }
    });

    // Devices
    on_app!(on_select_participant, |key: SharedString| |ctx, app, tray, key| {
        ctx.borrow_mut().bridge.selected_participant_key = key.to_string();
        reapply(&ctx, &app, tray, false);
    });
    on_app!(on_save_alias, || |ctx, app, tray| {
        let alias = app.global::<App>().get_alias_input().trim().to_string();
        let Some(npub) = selected_npub(&ctx) else { return };
        dispatch(
            &ctx,
            NativeAppAction::SetParticipantAlias { npub, alias },
            "Saving alias",
        );
    });
    on_app!(on_toggle_participant_admin, || |ctx, app, tray| {
        let (network_id, npub, is_admin) = {
            let ctx = ctx.borrow();
            let Some(state) = ctx.state.as_ref() else { return };
            let Some(network) = bridge::active_network(&ctx.bridge, state) else { return };
            let Some(participant) = ctx.bridge.selected_participant(state) else { return };
            (
                network.id.clone(),
                participant.npub.clone(),
                participant.is_admin,
            )
        };
        let (action, status) = if is_admin {
            (
                NativeAppAction::RemoveAdmin { network_id, npub },
                "Removing admin",
            )
        } else {
            (
                NativeAppAction::AddAdmin { network_id, npub },
                "Adding admin",
            )
        };
        dispatch(&ctx, action, status);
    });
    on_app!(on_remove_participant, || |ctx, app, tray| {
        let (npub, name) = {
            let ctx = ctx.borrow();
            let Some(state) = ctx.state.as_ref() else { return };
            let Some(network) = bridge::active_network(&ctx.bridge, state) else { return };
            if !network.local_is_admin {
                return;
            }
            let Some(participant) = ctx.bridge.selected_participant(state) else { return };
            if participant_is_self(participant, &state.own_npub) {
                return;
            }
            (
                participant.npub.clone(),
                {
                    let name = display::participant_display_name(
                        &participant.magic_dns_name,
                        &participant.alias,
                        &participant.magic_dns_alias,
                        &participant.npub,
                    );
                    if name.trim().is_empty() {
                        "this device".to_string()
                    } else {
                        name
                    }
                },
            )
        };
        if confirm(
            "This removes the device from the network's roster. They keep the network locally but won't be in this roster anymore.",
            &format!("Remove {name}?"),
        ) {
            let network_id = {
                let ctx = ctx.borrow();
                ctx.state
                    .as_ref()
                    .and_then(|state| bridge::active_network(&ctx.bridge, state))
                    .map(|network| network.id.clone())
                    .unwrap_or_default()
            };
            dispatch(
                &ctx,
                NativeAppAction::RemoveParticipant { network_id, npub },
                "Removing device",
            );
        }
    });
    on_app!(on_save_hints, || |ctx, app, tray| {
        let hints = app.global::<App>().get_hints_input().to_string();
        let mut parsed: Vec<String> = hints
            .split([',', '\n', '\r', '\t', ' '])
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
        parsed.dedup();
        let Some(npub) = selected_npub(&ctx) else { return };
        dispatch(
            &ctx,
            NativeAppAction::SetParticipantEndpointHints {
                npub,
                endpoint_hints: parsed,
            },
            "Saving address hints",
        );
    });

    // Add device
    on_app!(on_join_input_edited, |value: SharedString| |ctx, app, tray, value| {
        refresh_add_device(&ctx, &app);
        let trimmed = value.trim().to_string();
        let eligible = {
            let ctx = ctx.borrow();
            !ctx.action_in_flight
                && ctx
                    .state
                    .as_ref()
                    .and_then(|state| bridge::active_network(&ctx.bridge, state))
                    .is_some_and(|network| network.local_is_admin)
        };
        if eligible && display::looks_like_join_request(&trimmed) {
            confirm_and_import(&ctx, &app, tray, &trimmed);
        }
    });
    on_app!(on_add_participant, || |ctx, app, tray| {
        let g = app.global::<App>();
        let participant = g.get_participant_input().trim().to_string();
        if participant.is_empty() {
            return;
        }
        let alias = g.get_participant_alias_input().trim().to_string();
        let network_id = {
            let ctx = ctx.borrow();
            match ctx
                .state
                .as_ref()
                .and_then(|state| bridge::active_network(&ctx.bridge, state))
            {
                Some(network) if network.local_is_admin => network.id.clone(),
                _ => return,
            }
        };
        ctx.borrow_mut().pending = Pending::AddParticipant {
            participant: participant.clone(),
        };
        if !dispatch(
            &ctx,
            NativeAppAction::AddParticipant {
                network_id,
                npub: participant,
                alias: if alias.is_empty() { None } else { Some(alias) },
            },
            "Adding device",
        ) {
            ctx.borrow_mut().pending = Pending::None;
        }
    });

    // Internet
    on_app!(on_select_direct_exit, || |ctx, app, tray| {
        dispatch(
            &ctx,
            update_settings_patch(|patch| patch.internet_source = Some("direct".to_string())),
            "Saving internet source",
        );
    });
    on_app!(on_select_wireguard_exit, || |ctx, app, tray| {
        dispatch(
            &ctx,
            update_settings_patch(|patch| patch.internet_source = Some("wireguard".to_string())),
            "Saving internet source",
        );
    });
    on_app!(on_select_paid_auto_exit, || |ctx, app, tray| {
        dispatch(
            &ctx,
            update_settings_patch(|patch| patch.internet_source = Some("paid_automatic".to_string())),
            "Saving internet source",
        );
    });
    on_app!(on_select_paid_manual_exit, || |ctx, app, tray| {
        set_page(&ctx, &app, 4);
    });
    on_app!(on_select_peer_exit, |npub: SharedString| |ctx, app, tray, npub| {
        dispatch(
            &ctx,
            update_settings_patch(|patch| {
                patch.internet_source = Some("private_vpn".to_string());
                patch.exit_node = Some(npub.to_string());
            }),
            "Saving internet source",
        );
    });
    on_app!(on_rate_exit, |seller: SharedString, vote: i32| |ctx, app, tray, seller, vote| {
        let current = {
            let ctx = ctx.borrow();
            ctx.state
                .as_ref()
                .and_then(|state| current_rating(state, &seller))
                .unwrap_or(0)
        };
        let rating = if current == vote as i64 { 0 } else { vote as i64 };
        dispatch(
            &ctx,
            NativeAppAction::RatePaidExit {
                seller_npub: seller.to_string(),
                rating,
            },
            "Saving public rating",
        );
    });
    on_app!(on_reselect_paid_exit, || |ctx, app, tray| {
        dispatch(&ctx, NativeAppAction::ReselectPaidExit, "Finding another provider");
    });
    on_app!(on_set_advertise_exit, |enabled: bool| |ctx, app, tray, enabled| {
        dispatch(
            &ctx,
            update_settings_patch(|patch| patch.advertise_exit_node = Some(enabled)),
            "Saving internet sharing",
        );
    });
    on_app!(on_set_leak_protection, |enabled: bool| |ctx, app, tray, enabled| {
        dispatch(
            &ctx,
            update_settings_patch(|patch| patch.exit_node_leak_protection = Some(enabled)),
            "Saving internet protection",
        );
    });
    on_app!(on_import_wg_file, || |ctx, app, tray| {
        let Some(path) = platform::open_file_dialog(
            "Import WireGuard config",
            &[("WireGuard configs", &["conf", "txt"]), ("All files", &["*"])],
        ) else {
            return;
        };
        let config = match std::fs::read_to_string(&path) {
            Ok(config) => config,
            Err(error) => {
                set_notice(&app, &format!("Could not read WireGuard config: {error}"));
                return;
            }
        };
        if config.trim().is_empty() {
            set_notice(&app, "Selected WireGuard config is empty.");
            return;
        }
        app.global::<App>().set_wg_config(config.clone().into());
        dispatch(
            &ctx,
            update_settings_patch(|patch| patch.wireguard_exit_config = Some(config)),
            "Saving WireGuard",
        );
    });
    on_app!(on_save_wg_config, || |ctx, app, tray| {
        let config = app.global::<App>().get_wg_config().to_string();
        dispatch(
            &ctx,
            update_settings_patch(|patch| patch.wireguard_exit_config = Some(config)),
            "Saving WireGuard",
        );
    });
    on_app!(on_save_exit_dns, || |ctx, app, tray| {
        let g = app.global::<App>();
        let (mode, provider, url, bootstrap, through) = (
            g.get_dns_mode().to_string(),
            g.get_dns_provider().to_string(),
            g.get_dns_custom_url().to_string(),
            g.get_dns_bootstrap().to_string(),
            g.get_dns_through().to_string(),
        );
        dispatch(
            &ctx,
            update_settings_patch(|patch| {
                patch.exit_dns_mode = Some(mode);
                patch.exit_dns_doh_provider = Some(provider);
                patch.exit_dns_custom_doh_url = Some(url);
                patch.exit_dns_custom_doh_bootstrap_ips = Some(bootstrap);
                patch.exit_dns_through_exit_servers = Some(through);
            }),
            "Saving Exit DNS",
        );
    });

    // Public exits
    on_app!(on_discover_offers, || |ctx, app, tray| {
        dispatch(
            &ctx,
            NativeAppAction::DiscoverPaidRouteOffers { duration_secs: 5 },
            "Finding sellers",
        );
    });
    on_app!(on_stream_payments, || |ctx, app, tray| {
        dispatch(
            &ctx,
            NativeAppAction::StreamPaidRoutePayments {
                publish: true,
                min_increment_msat: 1,
                limit: 0,
            },
            "Paying for usage",
        );
    });
    on_app!(on_set_manual_provider, || |ctx, app, tray| {
        let provider = app.global::<App>().get_manual_provider().trim().to_string();
        if provider.is_empty() {
            return;
        }
        dispatch(
            &ctx,
            NativeAppAction::SetManualPaidExitProvider { provider },
            "Adding provider",
        );
    });
    on_app!(on_clear_manual_provider, || |ctx, app, tray| {
        ctx.borrow_mut().pending = Pending::ClearProvider;
        if !dispatch(&ctx, NativeAppAction::ClearManualPaidExitProvider, "Clearing provider") {
            ctx.borrow_mut().pending = Pending::None;
        }
    });
    on_app!(on_filter_editing, || |ctx, app, tray| {
        ctx.borrow_mut().bridge.begin_filter_editing();
    });
    on_app!(on_apply_filter, || |ctx, app, tray| {
        let g = app.global::<App>();
        let (country, sort, v4, v6) = (
            g.get_filter_country().trim().to_string(),
            g.get_filter_sort().to_string(),
            g.get_filter_v4(),
            g.get_filter_v6(),
        );
        ctx.borrow_mut().pending = Pending::ApplyFilter;
        if !dispatch(
            &ctx,
            NativeAppAction::SetPaidRouteMarketFilter {
                query: String::new(),
                country_code: country,
                mint_url: String::new(),
                require_ipv4: v4,
                require_ipv6: v6,
                sort,
            },
            "Filtering sellers",
        ) {
            ctx.borrow_mut().pending = Pending::None;
        }
    });
    on_app!(on_clear_filter, || |ctx, app, tray| {
        ctx.borrow_mut().pending = Pending::ApplyFilter;
        if !dispatch(
            &ctx,
            NativeAppAction::SetPaidRouteMarketFilter {
                query: String::new(),
                country_code: String::new(),
                mint_url: String::new(),
                require_ipv4: false,
                require_ipv6: false,
                sort: "quality".to_string(),
            },
            "Clearing seller filters",
        ) {
            ctx.borrow_mut().pending = Pending::None;
        }
    });
    on_app!(on_send_envelope, || |ctx, app, tray| {
        let envelope = {
            let ctx = ctx.borrow();
            ctx.state
                .as_ref()
                .map(|state| state.paid_route_market.last_payment_action.envelope_json.trim().to_string())
                .unwrap_or_default()
        };
        if envelope.is_empty() {
            return;
        }
        dispatch(
            &ctx,
            NativeAppAction::SendPaidRoutePaymentEnvelope {
                envelope_json: envelope,
            },
            "Sending payment",
        );
    });
    on_app!(on_buy_offer, |key: SharedString| |ctx, app, tray, key| {
        if key.trim().is_empty() {
            return;
        }
        dispatch(
            &ctx,
            NativeAppAction::BuyPaidRouteOffer {
                offer_key: key.to_string(),
                mint_url: None,
                channel_capacity_sat: None,
            },
            "Connecting",
        );
    });
    on_app!(on_use_session, |session_id: SharedString| |ctx, app, tray, session_id| {
        if session_id.trim().is_empty() {
            return;
        }
        dispatch(
            &ctx,
            NativeAppAction::SelectPaidRouteSession {
                session_id: session_id.to_string(),
                connect: true,
            },
            "Connecting",
        );
    });
    on_app!(on_probe_session, |session_id: SharedString| |ctx, app, tray, session_id| {
        if session_id.trim().is_empty() {
            return;
        }
        dispatch(
            &ctx,
            NativeAppAction::ProbePaidRouteSession {
                session_id: session_id.to_string(),
                timeout_secs: 5,
            },
            "Checking connection",
        );
    });
    on_app!(on_fund_session, |session_id: SharedString| |ctx, app, tray, session_id| {
        if session_id.trim().is_empty() {
            return;
        }
        dispatch(
            &ctx,
            NativeAppAction::OpenPaidRouteChannelFromWallet {
                session_id: session_id.to_string(),
                mint_url: None,
                paid_msat: None,
                max_amount_per_output: None,
                keyset_id: None,
            },
            "Funding seller",
        );
    });
    on_app!(on_pay_session, |session_id: SharedString| |ctx, app, tray, session_id| {
        if session_id.trim().is_empty() {
            return;
        }
        dispatch(
            &ctx,
            NativeAppAction::SignPaidRoutePaymentEnvelopeFromWallet {
                session_id: session_id.to_string(),
                kind: "balance-update".to_string(),
                delivered_units: None,
                paid_msat: None,
            },
            "Paying seller",
        );
    });
    on_app!(on_settle_session, |session_id: SharedString| |ctx, app, tray, session_id| {
        if session_id.trim().is_empty() {
            return;
        }
        dispatch(
            &ctx,
            NativeAppAction::ClosePaidRouteChannelFromWallet {
                session_id: session_id.to_string(),
                publish: true,
            },
            "Settling channel",
        );
    });

    // Wallet
    on_app!(on_refresh_wallet, || |ctx, app, tray| {
        dispatch(
            &ctx,
            NativeAppAction::RefreshPaidRouteWallet { refresh: true },
            "Refreshing wallet",
        );
    });
    on_app!(on_add_mint, || |ctx, app, tray| {
        let url = app.global::<App>().get_mint_url().trim().to_string();
        if url.is_empty() {
            return;
        }
        dispatch(
            &ctx,
            NativeAppAction::AddPaidRouteWalletMint { url, label: None },
            "Adding mint",
        );
    });
    on_app!(on_topup_wallet, || |ctx, app, tray| {
        let Some(amount) = parse_positive_u64(&app.global::<App>().get_topup_amount()) else {
            return;
        };
        dispatch(
            &ctx,
            NativeAppAction::TopUpPaidRouteWallet {
                mint_url: None,
                amount_sat: amount,
            },
            "Creating top-up invoice",
        );
    });
    on_app!(on_send_token, || |ctx, app, tray| {
        let Some(amount) = parse_positive_u64(&app.global::<App>().get_send_amount()) else {
            return;
        };
        dispatch(
            &ctx,
            NativeAppAction::SendPaidRouteWalletToken {
                mint_url: None,
                amount_sat: amount,
            },
            "Exporting token",
        );
    });
    on_app!(on_redeem_token, || |ctx, app, tray| {
        let token = app.global::<App>().get_receive_token().trim().to_string();
        if token.is_empty() {
            return;
        }
        app.global::<App>().set_receive_token(SharedString::new());
        ctx.borrow_mut().pending = Pending::PreviewToken { token: token.clone() };
        if !dispatch(
            &ctx,
            NativeAppAction::PreviewPaidRouteWalletToken { token },
            "Checking token",
        ) {
            ctx.borrow_mut().pending = Pending::None;
        }
    });
    on_app!(on_pay_invoice, || |ctx, app, tray| {
        let invoice = app.global::<App>().get_withdraw_invoice().trim().to_string();
        if invoice.is_empty() {
            return;
        }
        dispatch(
            &ctx,
            NativeAppAction::WithdrawPaidRouteWalletLightning {
                mint_url: None,
                invoice,
            },
            "Paying invoice",
        );
    });
    on_app!(on_scan_token_qr, || |ctx, app, tray| {
        let Some(path) = platform::open_file_dialog(
            "Scan token QR",
            &[
                ("Images", &["png", "jpg", "jpeg", "bmp", "gif"]),
                ("All files", &["*"]),
            ],
        ) else {
            return;
        };
        let token = match qr::decode_file(&path.to_string_lossy()) {
            Err(error) => {
                set_notice(&app, &error);
                return;
            }
            Ok(value) => value.trim().to_string(),
        };
        if !display::looks_like_cashu_token(&token) {
            set_notice(&app, "QR does not contain a Cashu token");
            return;
        }
        ctx.borrow_mut().pending = Pending::PreviewToken { token: token.clone() };
        if !dispatch(
            &ctx,
            NativeAppAction::PreviewPaidRouteWalletToken { token },
            "Checking token",
        ) {
            ctx.borrow_mut().pending = Pending::None;
        }
    });

    // Sell
    on_app!(on_set_seller_enabled, |enabled: bool| |ctx, app, tray, enabled| {
        dispatch(
            &ctx,
            update_settings_patch(|patch| patch.paid_exit_enabled = Some(enabled)),
            "Saving listing",
        );
    });
    on_app!(on_save_seller_price, || |ctx, app, tray| {
        let Some(price) = parse_positive_u64(&app.global::<App>().get_seller_price()) else {
            return;
        };
        dispatch(
            &ctx,
            update_settings_patch(|patch| patch.paid_exit_price_msat_per_gb = Some(price)),
            "Saving price",
        );
    });

    // Settings
    on_app!(on_save_node, || |ctx, app, tray| {
        let g = app.global::<App>();
        let (name, endpoint, tunnel, port, ports) = (
            g.get_node_name().to_string(),
            g.get_endpoint().to_string(),
            g.get_tunnel_ip().to_string(),
            g.get_listen_port().trim().parse::<u16>().ok(),
            g.get_fips_ports().to_string(),
        );
        dispatch(
            &ctx,
            update_settings_patch(|patch| {
                patch.node_name = Some(name);
                patch.endpoint = Some(endpoint);
                patch.tunnel_ip = Some(tunnel);
                patch.listen_port = port;
                patch.fips_host_inbound_tcp_ports = Some(ports);
            }),
            "Saving device",
        );
    });
    on_app!(on_add_relay, || |ctx, app, tray| {
        let url = match normalize_relay_url(&app.global::<App>().get_relay_input()) {
            Some(url) => url,
            None => return,
        };
        let (mut enabled, mut disabled) = {
            let ctx = ctx.borrow();
            match ctx.state.as_ref() {
                Some(state) => relay_lists(state),
                None => (Vec::new(), Vec::new()),
            }
        };
        disabled.retain(|relay| *relay != url);
        if !enabled.contains(&url) {
            enabled.push(url.clone());
        }
        app.global::<App>().set_relay_input(SharedString::new());
        dispatch(
            &ctx,
            update_settings_patch(|patch| {
                patch.relays = Some(enabled);
                patch.disabled_relays = Some(disabled);
            }),
            "Saving relays",
        );
    });
    on_app!(on_set_relay_enabled, |url: SharedString, enable: bool| |ctx, app, tray, url, enable| {
        let Some(url) = normalize_relay_url(&url) else { return };
        let (mut enabled, mut disabled) = {
            let ctx = ctx.borrow();
            match ctx.state.as_ref() {
                Some(state) => relay_lists(state),
                None => return,
            }
        };
        enabled.retain(|relay| *relay != url);
        disabled.retain(|relay| *relay != url);
        if enable {
            enabled.push(url);
        } else {
            disabled.push(url);
        }
        dispatch(
            &ctx,
            update_settings_patch(|patch| {
                patch.relays = Some(enabled);
                patch.disabled_relays = Some(disabled);
            }),
            "Saving relays",
        );
    });
    on_app!(on_remove_relay, |url: SharedString| |ctx, app, tray, url| {
        let Some(url) = normalize_relay_url(&url) else { return };
        let (mut enabled, mut disabled) = {
            let ctx = ctx.borrow();
            match ctx.state.as_ref() {
                Some(state) => relay_lists(state),
                None => return,
            }
        };
        enabled.retain(|relay| *relay != url);
        disabled.retain(|relay| *relay != url);
        dispatch(
            &ctx,
            update_settings_patch(|patch| {
                patch.relays = Some(enabled);
                patch.disabled_relays = Some(disabled);
            }),
            "Saving relays",
        );
    });
    on_app!(on_set_autoconnect, |enabled: bool| |ctx, app, tray, enabled| {
        dispatch(
            &ctx,
            update_settings_patch(|patch| patch.autoconnect = Some(enabled)),
            "Saving VPN option",
        );
    });
    on_app!(on_set_launch_startup, |enabled: bool| |ctx, app, tray, enabled| {
        if let Err(error) = platform::set_launch_on_startup(enabled) {
            set_notice(&app, &error);
            return;
        }
        dispatch(
            &ctx,
            update_settings_patch(|patch| patch.launch_on_startup = Some(enabled)),
            "Saving startup",
        );
    });
    on_app!(on_set_close_tray, |enabled: bool| |ctx, app, tray, enabled| {
        dispatch(
            &ctx,
            update_settings_patch(|patch| patch.close_to_tray_on_close = Some(enabled)),
            "Saving tray behavior",
        );
    });
    on_app!(on_set_theme, |pref: i32| |ctx, app, tray, pref| {
        app.global::<App>().set_theme_pref(pref);
        platform::save_theme_pref(pref);
    });
    on_app!(on_set_fiat_enabled, |enabled: bool| |ctx, app, tray, enabled| {
        dispatch(
            &ctx,
            update_settings_patch(|patch| patch.wallet_fiat_enabled = Some(enabled)),
            "Saving wallet display",
        );
    });
    on_app!(on_set_fiat_currency, |currency: SharedString| |ctx, app, tray, currency| {
        dispatch(
            &ctx,
            update_settings_patch(|patch| patch.wallet_fiat_currency = Some(currency.to_string())),
            "Saving wallet currency",
        );
    });
    on_app!(on_set_fips_host_tunnel, |enabled: bool| |ctx, app, tray, enabled| {
        dispatch(
            &ctx,
            update_settings_patch(|patch| patch.fips_host_tunnel_enabled = Some(enabled)),
            "Saving FIPS option",
        );
    });
    on_app!(on_set_fips_non_roster, |enabled: bool| |ctx, app, tray, enabled| {
        dispatch(
            &ctx,
            update_settings_patch(|patch| patch.connect_to_non_roster_fips_peers = Some(enabled)),
            "Saving FIPS option",
        );
    });
    on_app!(on_set_fips_nostr_discovery, |enabled: bool| |ctx, app, tray, enabled| {
        dispatch(
            &ctx,
            update_settings_patch(|patch| patch.fips_nostr_discovery_enabled = Some(enabled)),
            "Saving FIPS option",
        );
    });
    on_app!(on_set_fips_bootstrap, |enabled: bool| |ctx, app, tray, enabled| {
        dispatch(
            &ctx,
            update_settings_patch(|patch| patch.fips_bootstrap_enabled = Some(enabled)),
            "Saving FIPS option",
        );
    });
    on_app!(on_rename_network, || |ctx, app, tray| {
        let (network_id, name) = {
            let ctx = ctx.borrow();
            let Some(state) = ctx.state.as_ref() else { return };
            let Some(network) = bridge::active_network(&ctx.bridge, state) else { return };
            (network.id.clone(), app.global::<App>().get_network_name_draft().trim().to_string())
        };
        if name.is_empty() {
            return;
        }
        dispatch(
            &ctx,
            NativeAppAction::RenameNetwork { network_id, name },
            "Renaming network",
        );
    });
    on_app!(on_save_mesh_id, || |ctx, app, tray| {
        let mesh = display::normalize_network_id_input(&app.global::<App>().get_network_mesh_draft());
        if mesh.trim().is_empty() {
            return;
        }
        let network_id = {
            let ctx = ctx.borrow();
            match ctx
                .state
                .as_ref()
                .and_then(|state| bridge::active_network(&ctx.bridge, state))
            {
                Some(network) => network.id.clone(),
                None => return,
            }
        };
        dispatch(
            &ctx,
            NativeAppAction::SetNetworkMeshId { network_id, mesh_id: mesh },
            "Saving network ID",
        );
    });
    on_app!(on_remove_network, |network_id: SharedString| |ctx, app, tray, network_id| {
        if network_id.trim().is_empty() {
            return;
        }
        dispatch(
            &ctx,
            NativeAppAction::RemoveNetwork {
                network_id: network_id.to_string(),
            },
            "Deleting network",
        );
    });
    on_app!(on_activate_network, |network_id: SharedString| |ctx, app, tray, network_id| {
        if network_id.trim().is_empty() {
            return;
        }
        dispatch(
            &ctx,
            NativeAppAction::SetNetworkEnabled {
                network_id: network_id.to_string(),
                enabled: true,
            },
            "Switching network",
        );
    });
    on_app!(on_install_service, || |ctx, app, tray| {
        dispatch(&ctx, NativeAppAction::InstallSystemService, "Installing service");
    });
    on_app!(on_enable_service, || |ctx, app, tray| {
        dispatch(&ctx, NativeAppAction::EnableSystemService, "Enabling service");
    });
    on_app!(on_disable_service, || |ctx, app, tray| {
        dispatch(&ctx, NativeAppAction::DisableSystemService, "Disabling service");
    });
    on_app!(on_install_cli, || |ctx, app, tray| {
        dispatch(&ctx, NativeAppAction::InstallCli, "Installing CLI");
    });
    on_app!(on_check_updates, || |ctx, app, tray| {
        start_check(&ctx, true);
    });
    on_app!(on_install_update, || |ctx, app, tray| {
        start_download(&ctx);
    });
    on_app!(on_set_auto_install, |enabled: bool| |ctx, app, tray, enabled| {
        platform::save_auto_install_updates(enabled);
        ctx.borrow_mut().update.auto_install = enabled;
        apply_update(&ctx.borrow(), &app);
        if enabled {
            start_download(&ctx);
        }
    });

    // Tray
    tray.on_toggle_vpn({
        let ctx = ctx.clone();
        move || toggle_vpn(&ctx)
    });
    {
        let ctx = ctx.clone();
        let app = app.as_weak();
        tray.on_copy_device(move || {
            let Some(app) = app.upgrade() else { return };
            let npub = ctx
                .borrow()
                .state
                .as_ref()
                .map(|state| state.own_npub.clone())
                .unwrap_or_default();
            copy_text(&ctx, &app, &npub);
        });
    }
    {
        let ctx = ctx.clone();
        let app = app.as_weak();
        tray.on_copy_text(move |value: SharedString| {
            if let Some(app) = app.upgrade() {
                copy_text(&ctx, &app, &value);
            }
        });
    }
    {
        let ctx = ctx.clone();
        tray.on_share_toggle(move || {
            let advertise = {
                let ctx = ctx.borrow();
                ctx.state
                    .as_ref()
                    .map(|state| state.advertise_exit_node)
                    .unwrap_or(false)
            };
            dispatch(
                &ctx,
                update_settings_patch(|patch| patch.advertise_exit_node = Some(!advertise)),
                "Saving internet sharing",
            );
        });
    }
    {
        let ctx = ctx.clone();
        let app = app.as_weak();
        tray.on_tray_select_exit(move |kind: i32, arg: SharedString| {
            let Some(app) = app.upgrade() else { return };
            match kind {
                2 => {
                    dispatch(
                        &ctx,
                        update_settings_patch(|patch| patch.internet_source = Some("direct".to_string())),
                        "Saving internet source",
                    );
                }
                3 => {
                    dispatch(
                        &ctx,
                        update_settings_patch(|patch| {
                            patch.internet_source = Some("paid_automatic".to_string());
                        }),
                        "Saving internet source",
                    );
                }
                4 => {
                    set_page(&ctx, &app, 4);
                }
                5 => {
                    dispatch(
                        &ctx,
                        update_settings_patch(|patch| patch.internet_source = Some("wireguard".to_string())),
                        "Saving internet source",
                    );
                }
                6 => {
                    dispatch(
                        &ctx,
                        update_settings_patch(|patch| {
                            patch.internet_source = Some("private_vpn".to_string());
                            patch.exit_node = Some(arg.to_string());
                        }),
                        "Saving internet source",
                    );
                }
                _ => {}
            }
        });
    }
    {
        let app = app.as_weak();
        tray.on_open_window(move || {
            if let Some(app) = app.upgrade() {
                show_main(&app);
            }
        });
    }
    {
        let ctx = ctx.clone();
        tray.on_quit_app(move || {
            ctx.borrow_mut().quitting = true;
            slint::quit_event_loop().ok();
        });
    }
}

fn toggle_vpn(ctx: &Rc<RefCell<Ctx>>) {
    let Some(state) = ctx.borrow().state.clone() else { return };
    if !state.vpn_enabled && state.service_supported && !state.service_installed {
        dispatch(ctx, NativeAppAction::InstallSystemService, "Installing service");
    } else if state.vpn_enabled {
        dispatch(ctx, NativeAppAction::DisconnectVpn, "Turning VPN off");
    } else {
        dispatch(ctx, NativeAppAction::ConnectVpn, "Turning VPN on");
    }
}

fn selected_npub(ctx: &Rc<RefCell<Ctx>>) -> Option<String> {
    let ctx = ctx.borrow();
    let state = ctx.state.as_ref()?;
    ctx.bridge
        .selected_participant(state)
        .map(|participant| participant.npub.clone())
}

fn participant_is_self(participant: &nostr_vpn_app_core::NativeParticipantState, own_npub: &str) -> bool {
    bridge::participant_is_self(participant, own_npub)
}

fn refresh_add_device(ctx: &Rc<RefCell<Ctx>>, app: &AppWindow) {
    let g = app.global::<App>();
    let (invalid, can) = {
        let ctx = ctx.borrow();
        let Some(state) = ctx.state.as_ref() else { return };
        let input = g.get_participant_input().to_string();
        let invalid =
            !input.trim().is_empty() && !display::is_valid_device_id(input.trim());
        let admin_enabled = bridge::active_network(&ctx.bridge, state)
            .is_some_and(|network| network.local_is_admin && network.enabled);
        let can = !ctx.action_in_flight && admin_enabled && !input.trim().is_empty() && !invalid;
        (invalid, can)
    };
    g.set_participant_input_invalid(invalid);
    g.set_can_add_participant(can);
}

/// Builds `UpdateSettings` with a fully-Other patch — every untouched field
/// stays None so the core merges only what the caller set.
fn update_settings_patch(
    configure: impl FnOnce(&mut nostr_vpn_app_core::SettingsPatch),
) -> NativeAppAction {
    let mut patch = nostr_vpn_app_core::SettingsPatch::default();
    configure(&mut patch);
    NativeAppAction::UpdateSettings { patch }
}
