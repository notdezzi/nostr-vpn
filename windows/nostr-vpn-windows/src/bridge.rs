// Maps NativeAppState onto the Slint `App` global and tray icon. This is the
// port of the WPF AppViewModel computed properties and ApplyState/SyncDrafts:
// every string the user sees is computed here so the .slint file stays a dumb
// view. Draft fields are only overwritten on dispatch results (sync_drafts),
// matching the WPF rule that refreshes never clobber in-progress edits.

use slint::{ModelRc, SharedString, VecModel};
use std::rc::Rc;

use nostr_vpn_app_core::native_state::{
    NativePaidRouteOfferState, NativePaidRouteSessionState,
};
use nostr_vpn_app_core::{NativeAppState, NativeNetworkState, NativeParticipantState};

use crate::display as d;
use crate::qr;
use crate::{
    App, AutoRateRow, CustomerRow, DiagRow, ExitRow, HealthRow, NetworkRow, NostrVpnTray,
    OfferRow, ParticipantDetail, ParticipantRow, RelayRow, SessionRow, TrayDeviceRow, TrayExitRow,
};

pub const PAGE_DEVICES: i32 = 0;
pub const PAGE_ADD_NETWORK: i32 = 1;

#[derive(Default)]
pub struct Bridge {
    pub shown_network_id: String,
    pub selected_participant_key: String,
    pub setup_mode: i32,
    pub join_request_prompt_open: bool,
    last_synced_manual_provider: Option<String>,
    last_synced_price_msat_per_gb: Option<u64>,
    filter_editing: bool,
    join_qr_link: String,
    invoice_qr_request: String,
}

pub fn active_network<'a>(bridge: &Bridge, state: &'a NativeAppState) -> Option<&'a NativeNetworkState> {
    state
        .networks
        .iter()
        .find(|network| network.id == bridge.shown_network_id)
        .or_else(|| state.networks.iter().find(|network| network.enabled))
        .or_else(|| state.networks.first())
}

fn runtime_enabled_network<'a>(state: &'a NativeAppState) -> Option<&'a NativeNetworkState> {
    state.networks.iter().find(|network| network.enabled)
}

pub fn selection_key(participant: &NativeParticipantState) -> String {
    if participant.pubkey_hex.trim().is_empty() {
        participant.npub.clone()
    } else {
        participant.pubkey_hex.clone()
    }
}

pub fn participant_is_self(participant: &NativeParticipantState, own_npub: &str) -> bool {
    participant.mesh_state.eq_ignore_ascii_case("local")
        || (!own_npub.trim().is_empty() && participant.npub == own_npub)
}

pub fn participant_matches_key(participant: &NativeParticipantState, key: &str) -> bool {
    if key.trim().is_empty() {
        return false;
    }
    selection_key(participant) == key
        || (!participant.pubkey_hex.trim().is_empty()
            && participant.pubkey_hex.eq_ignore_ascii_case(key))
        || (!participant.npub.trim().is_empty() && participant.npub == key)
}

fn sorted_participants<'a>(
    network: &'a NativeNetworkState,
    own_npub: &'a str,
) -> Vec<&'a NativeParticipantState> {
    let mut participants: Vec<&NativeParticipantState> = network.participants.iter().collect();
    participants.sort_by(|a, b| {
        let a_self = participant_is_self(a, own_npub);
        let b_self = participant_is_self(b, own_npub);
        b_self.cmp(&a_self)
            .then_with(|| b.reachable.cmp(&a.reachable))
            .then_with(|| {
                d::participant_display_name(
                    &a.magic_dns_name,
                    &a.alias,
                    &a.magic_dns_alias,
                    &a.npub,
                )
                .to_lowercase()
                .cmp(
                    &d::participant_display_name(
                        &b.magic_dns_name,
                        &b.alias,
                        &b.magic_dns_alias,
                        &b.npub,
                    )
                    .to_lowercase(),
                )
            })
    });
    participants
}

fn participant_name(participant: &NativeParticipantState) -> String {
    if !participant.magic_dns_name.trim().is_empty() {
        return participant.magic_dns_name.clone();
    }
    if !participant.alias.trim().is_empty() {
        return participant.alias.clone();
    }
    d::short_text(&participant.npub, 10, 6)
}

fn rating_up_text(personal_rating: i64) -> SharedString {
    if personal_rating > 0 {
        "👍 ✓".into()
    } else {
        "👍".into()
    }
}

fn rating_down_text(personal_rating: i64) -> SharedString {
    if personal_rating < 0 {
        "👎 ✓".into()
    } else {
        "👎".into()
    }
}

fn session_display_title(session: &NativePaidRouteSessionState) -> String {
    if !session.title_text.trim().is_empty() {
        return session.title_text.clone();
    }
    if session.allow_routing {
        return "Ready".to_string();
    }
    if session.unpaid_msat > 0 {
        return "Payment needed".to_string();
    }
    if !session.payment_channel_ready {
        return "Needs funds".to_string();
    }
    d::plain_status(
        if session.status_text.trim().is_empty() {
            &session.lifecycle_status
        } else {
            &session.status_text
        },
        "Session",
    )
}

fn session_display_detail(session: &NativePaidRouteSessionState) -> String {
    if !session.detail_text.trim().is_empty() {
        return session.detail_text.clone();
    }
    let access = d::access_title(
        &session.access_state,
        if session.lifecycle_status.trim().is_empty() {
            "session"
        } else {
            &session.lifecycle_status
        },
    );
    let usage = if !session.usage_text.trim().is_empty() {
        session.usage_text.clone()
    } else if session.bytes > 0 {
        format!("{} used", d::format_bytes(session.bytes))
    } else if session.packets > 0 {
        format!("{} packets", session.packets)
    } else {
        format!("{} units", session.delivered_units)
    };
    let due = if session.amount_due_text.trim().is_empty() {
        format!("{} due", d::format_msat(session.amount_due_msat))
    } else {
        session.amount_due_text.clone()
    };
    format!("{access}, {usage}, {due}")
}

fn session_display_location(session: &NativePaidRouteSessionState) -> String {
    if !session.location_text.trim().is_empty() {
        return session.location_text.clone();
    }
    let claim = d::country_claim_text(
        &session.claimed_country_code,
        &session.observed_country_code,
        &session.country_claim_status,
    );
    if !session.realized_exit_ip.trim().is_empty() && !claim.trim().is_empty() {
        return format!("{} · {claim}", session.realized_exit_ip);
    }
    if !session.realized_exit_ip.trim().is_empty() {
        session.realized_exit_ip.clone()
    } else {
        claim
    }
}

fn offer_row(offer: &NativePaidRouteOfferState) -> OfferRow {
    let country = if offer.country_code.trim().is_empty() {
        "Unknown country".to_string()
    } else {
        offer.country_code.to_uppercase()
    };
    OfferRow {
        key: offer.key.clone().into(),
        seller: offer.seller_npub.clone().into(),
        personal_rating: offer.personal_rating as i32,
        country: country.into(),
        price: offer.price_text.clone().into(),
        rating: if offer.has_rating {
            format!("Rating {}", offer.rating_score).into()
        } else {
            "".into()
        },
        status: offer.status_text.clone().into(),
        metric: d::metric_text(&offer.quality_text, &offer.bandwidth_text).into(),
        can_rate: offer.can_rate,
        can_use: offer.personal_rating >= 0,
        up_text: rating_up_text(offer.personal_rating),
        down_text: rating_down_text(offer.personal_rating),
    }
}

fn session_row(session: &NativePaidRouteSessionState) -> SessionRow {
    let can_settle = !session.session_id.trim().is_empty()
        && session.payment_channel_ready
        && session.lifecycle_status != "closed"
        && session.lifecycle_status != "expired";
    SessionRow {
        session_id: session.session_id.clone().into(),
        seller: session.seller_npub.clone().into(),
        personal_rating: session.personal_rating as i32,
        title: session_display_title(session).into(),
        detail: session_display_detail(session).into(),
        location: session_display_location(session).into(),
        paid: if session.paid_text.trim().is_empty() {
            format!("{} paid", d::format_msat(session.paid_msat))
        } else {
            session.paid_text.clone()
        }
        .into(),
        behind: if session.unpaid_text.trim().is_empty() {
            if session.unpaid_msat > 0 {
                format!("{} behind", d::format_msat(session.unpaid_msat))
            } else {
                String::new()
            }
        } else {
            session.unpaid_text.clone()
        }
        .into(),
        metric: d::metric_text(&session.quality_text, &session.bandwidth_text).into(),
        settlement: session.settlement_text.clone().into(),
        can_rate: session.can_rate,
        can_use: session.personal_rating >= 0,
        can_open: !session.session_id.trim().is_empty() && !session.payment_channel_ready,
        can_pay: !session.session_id.trim().is_empty()
            && session.payment_channel_ready
            && session.unpaid_msat > 0,
        can_settle,
        up_text: rating_up_text(session.personal_rating),
        down_text: rating_down_text(session.personal_rating),
    }
}

fn vec_model<T: 'static + Clone>(rows: Vec<T>) -> ModelRc<T> {
    ModelRc::from(Rc::new(VecModel::from(rows)))
}

fn parse_positive_u64(value: &str) -> Option<u64> {
    value.trim().parse::<u64>().ok().filter(|parsed| *parsed > 0)
}

impl Bridge {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn selected_participant<'a>(
        &self,
        state: &'a NativeAppState,
    ) -> Option<&'a NativeParticipantState> {
        let network = active_network(self, state)?;
        if !self.selected_participant_key.trim().is_empty() {
            if let Some(selected) = network
                .participants
                .iter()
                .find(|participant| participant_matches_key(participant, &self.selected_participant_key))
            {
                return Some(selected);
            }
        }
        sorted_participants(network, &state.own_npub).first().copied()
    }

    /// Resolves and stores the selection like NormalizeSelectedParticipant.
    fn normalize_selection(&mut self, state: &NativeAppState) {
        let Some(network) = active_network(self, state) else {
            self.selected_participant_key = String::new();
            return;
        };
        if network.participants.is_empty() {
            self.selected_participant_key = String::new();
            return;
        }
        let matches = network
            .participants
            .iter()
            .any(|participant| participant_matches_key(participant, &self.selected_participant_key));
        if !matches {
            self.selected_participant_key = sorted_participants(network, &state.own_npub)
                .first()
                .map(|participant| selection_key(participant))
                .unwrap_or_default();
        }
    }

    pub fn page_is_visible(&self, page: i32, state: Option<&NativeAppState>) -> bool {
        let Some(state) = state else {
            return true;
        };
        match page {
            4 | 5 => state.paid_route_market.supported,
            6 => state.paid_exit_seller.supported,
            _ => true,
        }
    }

    pub fn apply(&mut self, app: &App, state: &NativeAppState, sync_drafts: bool, action_in_flight: bool) {
        if !self.shown_network_id.trim().is_empty()
            && state.networks.iter().all(|network| network.id != self.shown_network_id)
        {
            self.shown_network_id = String::new();
        }
        let active = active_network(self, state);
        self.normalize_selection(state);

        let page = app.get_page();
        if !self.page_is_visible(page, Some(state)) {
            app.set_page(PAGE_DEVICES);
        }

        // Header
        app.set_network_name(if let Some(network) = active {
            d::display_network_name(&network.name).into()
        } else {
            "Nostr VPN".into()
        });
        app.set_show_network_dot(state.networks.len() > 1);
        app.set_network_dot_ok(active.is_some_and(|network| network.enabled));
        app.set_hero_subtitle(
            format!(
                "{} of {} connected",
                state.connected_peer_count, state.expected_peer_count
            )
            .into(),
        );
        app.set_vpn_status_text(d::vpn_status_text(state).into());
        app.set_vpn_status_danger(state.exit_node_blocked);
        app.set_vpn_on(state.vpn_enabled);
        app.set_vpn_button_text(if state.vpn_enabled { "On" } else { "Off" }.into());
        let has_enabled_network = runtime_enabled_network(state).is_some();
        app.set_vpn_button_enabled(
            !action_in_flight
                && state.vpn_control_supported
                && (has_enabled_network || (state.service_supported && !state.service_installed)),
        );

        // Sidebar
        let market_visible = state.paid_route_market.supported;
        let seller_visible = state.paid_exit_seller.supported;
        app.set_market_visible(market_visible);
        app.set_seller_visible(seller_visible);
        app.set_wallet_nav_text(if state.paid_route_market.wallet.navigation_balance_text.trim().is_empty() {
            "Wallet".into()
        } else {
            state.paid_route_market.wallet.navigation_balance_text.clone().into()
        });
        app.set_has_active_network(active.is_some());
        app.set_add_device_enabled(
            active.is_some_and(|network| network.local_is_admin && network.enabled) && !action_in_flight,
        );

        // Network switcher model
        app.set_networks(vec_model(
            state
                .networks
                .iter()
                .map(|network| NetworkRow {
                    id: network.id.clone().into(),
                    name: d::display_network_name(&network.name).into(),
                    enabled: network.enabled,
                })
                .collect(),
        ));

        // Setup view
        app.set_setup_mode(self.setup_mode);
        app.set_own_npub(state.own_npub.clone().into());
        let join_link = state.join_request_qr_code_or_link.clone();
        app.set_show_join_qr(!has_enabled_network && !join_link.trim().is_empty());
        if self.join_qr_link != join_link {
            self.join_qr_link = join_link.clone();
            app.set_join_qr(qr::render_image(&join_link));
        }
        app.set_join_request_link(join_link.into());
        app.set_active_network_id(
            active
                .map(|network| d::display_network_id(&network.network_id))
                .unwrap_or_default()
                .into(),
        );
        let manual_admin = app.get_manual_join_admin().to_string();
        let manual_mesh = app.get_manual_join_mesh().to_string();
        let manual_invalid =
            !manual_admin.trim().is_empty() && !d::is_valid_device_id(manual_admin.trim());
        app.set_manual_join_invalid(manual_invalid);
        app.set_manual_join_can_submit(
            !action_in_flight
                && !manual_admin.trim().is_empty()
                && !d::normalize_network_id_input(&manual_mesh).trim().is_empty()
                && !manual_invalid,
        );

        // Devices page
        if let Some(network) = active {
            let rows: Vec<ParticipantRow> = sorted_participants(network, &state.own_npub)
                .iter()
                .map(|participant| {
                    let is_self = participant_is_self(participant, &state.own_npub);
                    ParticipantRow {
                        key: selection_key(participant).into(),
                        npub: participant.npub.clone().into(),
                        display_name: d::participant_display_name(
                            &participant.magic_dns_name,
                            &participant.alias,
                            &participant.magic_dns_alias,
                            &participant.npub,
                        )
                        .into(),
                        tunnel_ip: d::clean_tunnel_ip(&participant.tunnel_ip).into(),
                        connectivity: d::participant_connectivity(&participant.state, participant.reachable)
                            .into(),
                        is_self,
                        is_admin: participant.is_admin,
                        offers_exit: participant.offers_exit_node,
                        is_fips_routed: d::participant_is_fips_routed(
                            &participant.state,
                            participant.reachable,
                            &participant.fips_transport_addr,
                        ),
                    }
                })
                .collect();
            app.set_participants(vec_model(rows));
        } else {
            app.set_participants(vec_model(Vec::new()));
        }
        app.set_selected_participant(self.selected_participant_key.clone().into());
        app.set_has_participants(active.is_some_and(|network| !network.participants.is_empty()));
        if let Some(participant) = self.selected_participant(state) {
            let is_self = participant_is_self(participant, &state.own_npub);
            let is_direct = d::participant_is_fips_direct(
                &participant.state,
                participant.reachable,
                &participant.fips_transport_addr,
            );
            let is_routed = d::participant_is_fips_routed(
                &participant.state,
                participant.reachable,
                &participant.fips_transport_addr,
            );
            app.set_detail(ParticipantDetail {
                display_name: d::participant_display_name(
                    &participant.magic_dns_name,
                    &participant.alias,
                    &participant.magic_dns_alias,
                    &participant.npub,
                )
                .into(),
                connectivity: d::participant_connectivity(&participant.state, participant.reachable)
                    .into(),
                is_self,
                is_admin: participant.is_admin,
                offers_exit: participant.offers_exit_node,
                is_fips_direct: is_direct,
                is_fips_routed: is_routed,
                role_text: d::participant_role_text(is_self, participant.is_admin, participant.offers_exit_node)
                    .into(),
                fips_path_text: d::participant_fips_path_text(
                    &participant.state,
                    is_direct,
                    is_routed,
                    &participant.fips_transport_type,
                    participant.fips_srtt_ms,
                )
                .into(),
                last_seen: if participant.last_seen_text.trim().is_empty() {
                    "-".into()
                } else {
                    participant.last_seen_text.clone().into()
                },
                srtt_age: if participant.fips_srtt_age_ms == 0 {
                    "-".into()
                } else {
                    d::format_duration_ms(participant.fips_srtt_age_ms).into()
                },
                control_seen: if participant.last_fips_control_seen_text.trim().is_empty() {
                    "-".into()
                } else {
                    participant.last_fips_control_seen_text.clone().into()
                },
                data_seen: if participant.last_fips_data_seen_text.trim().is_empty() {
                    "-".into()
                } else {
                    participant.last_fips_data_seen_text.clone().into()
                },
                tx: d::format_bytes(participant.tx_bytes).into(),
                rx: d::format_bytes(participant.rx_bytes).into(),
                magic_dns: d::first_non_empty(&[
                    &participant.magic_dns_name,
                    &participant.magic_dns_alias,
                    "-",
                ])
                .into(),
                tunnel_ip: d::clean_tunnel_ip(&participant.tunnel_ip).into(),
                npub: participant.npub.clone().into(),
                hints_text: participant.fips_endpoint_hints.join(", ").into(),
                status_detail: participant.status_text.clone().into(),
            });
            // OneWay bindings in WPF reset these inputs on every state change.
            app.set_alias_input(participant.magic_dns_alias.clone().into());
            app.set_hints_input(participant.fips_endpoint_hints.join(", ").into());
            app.set_can_rename_participant(active.is_some_and(|network| network.local_is_admin));
            app.set_can_manage_participant(
                active.is_some_and(|network| network.local_is_admin) && !is_self,
            );
        } else {
            app.set_can_rename_participant(false);
            app.set_can_manage_participant(false);
        }

        // Add device page
        let admin_mode = active.is_some_and(|network| network.local_is_admin);
        app.set_admin_mode(admin_mode);
        app.set_admin_network_id(
            active
                .map(|network| d::display_network_id(&network.network_id))
                .unwrap_or_default()
                .into(),
        );
        let participant_input = app.get_participant_input().to_string();
        let participant_invalid =
            !participant_input.trim().is_empty() && !d::is_valid_device_id(participant_input.trim());
        app.set_participant_input_invalid(participant_invalid);
        app.set_can_add_participant(
            !action_in_flight
                && active.is_some_and(|network| network.local_is_admin && network.enabled)
                && !participant_input.trim().is_empty()
                && !participant_invalid,
        );

        // Internet page
        app.set_internet_status(state.exit_node_status_text.clone().into());
        app.set_direct_selected(state.internet_source == "direct");
        app.set_wg_selected(state.internet_source == "wireguard");
        app.set_wg_configured(state.wireguard_exit_configured);
        app.set_wg_subtitle(if !state.wireguard_exit_configured {
            "No WireGuard config saved yet".to_string()
        } else if state.wireguard_exit_endpoint.trim().is_empty() {
            "Configured".to_string()
        } else {
            state.wireguard_exit_endpoint.clone()
        }
        .into());
        app.set_paid_auto_selected(state.internet_source == "paid_automatic");
        app.set_paid_manual_selected(state.internet_source == "paid_manual");
        let automatic_selected =
            state.internet_source == "paid_automatic" && !state.exit_node.trim().is_empty();
        let auto_session = if automatic_selected && state.exit_node_active {
            state
                .paid_route_market
                .sessions
                .iter()
                .find(|session| session.seller_npub == state.exit_node && session.can_rate)
        } else {
            None
        };
        app.set_has_auto_session(auto_session.is_some());
        if let Some(session) = auto_session {
            app.set_auto_session(AutoRateRow {
                seller: session.seller_npub.clone().into(),
                up_text: rating_up_text(session.personal_rating),
                down_text: rating_down_text(session.personal_rating),
            });
        }
        if let Some(network) = active {
            let mut peers: Vec<&NativeParticipantState> = network
                .participants
                .iter()
                .filter(|participant| participant.offers_exit_node && !participant_is_self(participant, &state.own_npub))
                .collect();
            peers.sort_by_key(|participant| {
                d::participant_display_name(
                    &participant.magic_dns_name,
                    &participant.alias,
                    &participant.magic_dns_alias,
                    &participant.npub,
                )
                .to_lowercase()
            });
            app.set_peer_exits(vec_model(
                peers
                    .into_iter()
                    .map(|participant| ExitRow {
                        npub: participant.npub.clone().into(),
                        name: participant_name(participant).into(),
                        status: "".into(),
                    })
                    .collect(),
            ));
        } else {
            app.set_peer_exits(vec_model(Vec::new()));
        }
        app.set_advertise_exit(state.advertise_exit_node);
        app.set_leak_protection(state.exit_node_leak_protection);
        app.set_dns_encrypted_selected(state.exit_dns_mode == "encrypted");
        app.set_dns_custom_selected(
            state.exit_dns_mode == "encrypted" && state.exit_dns_doh_provider == "custom",
        );
        app.set_dns_through_selected(state.exit_dns_mode == "through_exit");

        // Public exits page
        let market = &state.paid_route_market;
        let wallet = &market.wallet;
        app.set_market_status(if market.status_text.trim().is_empty() {
            format!("{} internet sellers", market.offers.len())
        } else {
            market.status_text.clone()
        }
        .into());
        app.set_wallet_balance_short(wallet.navigation_balance_text.clone().into());
        app.set_hidden_offers_text(if market.hidden_offer_count == 0 {
            "".to_string()
        } else {
            format!("{} hidden by filters", market.hidden_offer_count)
        }
        .into());
        let visible_offers: Vec<&NativePaidRouteOfferState> =
            if market.hidden_offer_count > 0 || !market.visible_offers.is_empty() {
                market.visible_offers.iter().collect()
            } else {
                market.offers.iter().collect()
            };
        app.set_offers(vec_model(visible_offers.iter().map(|offer| offer_row(offer)).collect()));
        app.set_sessions(vec_model(
            market.sessions.iter().map(session_row).collect(),
        ));
        let payment_action = &market.last_payment_action;
        app.set_last_payment_status(if payment_action.kind.trim().is_empty()
            && payment_action.status_text.trim().is_empty()
        {
            "".to_string()
        } else if payment_action.status_text.trim().is_empty() {
            d::payment_action_title(&payment_action.kind)
        } else {
            payment_action.status_text.clone()
        }
        .into());
        app.set_can_send_envelope(
            !action_in_flight && !payment_action.envelope_json.trim().is_empty(),
        );
        app.set_has_streamable_payments(market.sessions.iter().any(|session| {
            !session.session_id.trim().is_empty()
                && session.payment_channel_ready
                && session.unpaid_msat > 0
        }));
        let manual_provider = app.get_manual_provider().to_string();
        app.set_can_add_provider(
            !action_in_flight && market_visible && !manual_provider.trim().is_empty(),
        );
        app.set_manual_provider_set(!market.manual_provider_link.trim().is_empty());
        app.set_manual_provider_status(market.manual_provider_status_text.clone().into());

        // Wallet page
        app.set_wallet_balance(if wallet.balance_known {
            d::text_or(
                &wallet.total_balance_text,
                &d::format_msat(wallet.total_balance_msat),
            )
        } else {
            String::new()
        }
        .into());
        app.set_wallet_secondary_balance(if wallet.fiat_balance_text.trim().is_empty() {
            String::new()
        } else {
            d::format_msat(wallet.total_balance_msat)
        }
        .into());
        app.set_wallet_rate_text(if wallet.exchange_rate_text.trim().is_empty() {
            String::new()
        } else {
            format!(
                "{} · {}",
                wallet.exchange_rate_text, wallet.exchange_rate_sources
            )
        }
        .into());
        let last_wallet_action = &wallet.last_action;
        app.set_wallet_action_status(if last_wallet_action.kind.trim().is_empty()
            && last_wallet_action.status_text.trim().is_empty()
        {
            "".to_string()
        } else {
            d::text_or(
                &last_wallet_action.status_text,
                &d::wallet_action_title(&last_wallet_action.kind),
            )
        }
        .into());
        let has_mint = !wallet.default_mint.trim().is_empty();
        app.set_has_mint(has_mint);
        let mint_url = app.get_mint_url().to_string();
        app.set_can_add_mint(!action_in_flight && market_visible && !mint_url.trim().is_empty());
        let topup_amount = app.get_topup_amount().to_string();
        app.set_can_topup(
            !action_in_flight && market_visible && has_mint && parse_positive_u64(&topup_amount).is_some(),
        );
        let send_amount = app.get_send_amount().to_string();
        app.set_can_send_token(
            !action_in_flight && market_visible && has_mint && parse_positive_u64(&send_amount).is_some(),
        );
        let receive_token = app.get_receive_token().to_string();
        app.set_can_receive_token(!action_in_flight && !receive_token.trim().is_empty());
        let withdraw_invoice = app.get_withdraw_invoice().to_string();
        app.set_can_withdraw(
            !action_in_flight && has_mint && !withdraw_invoice.trim().is_empty(),
        );
        let invoice_request = last_wallet_action.payment_request.clone();
        app.set_invoice_visible(!invoice_request.trim().is_empty());
        app.set_invoice_request(invoice_request.clone().into());
        if self.invoice_qr_request != invoice_request {
            self.invoice_qr_request = invoice_request.clone();
            app.set_invoice_qr(qr::render_image(&invoice_request));
        }
        app.set_invoice_expiry(if last_wallet_action.expires_at_unix == 0 {
            "".to_string()
        } else {
            format!("Expires {}", d::format_expiry(last_wallet_action.expires_at_unix))
        }
        .into());
        app.set_token_visible(
            last_wallet_action.kind == "send" && !last_wallet_action.token.trim().is_empty(),
        );
        app.set_token_text(last_wallet_action.token.clone().into());
        app.set_preimage_visible(
            last_wallet_action.kind == "withdraw" && !last_wallet_action.preimage.trim().is_empty(),
        );
        app.set_preimage_text(last_wallet_action.preimage.clone().into());
        app.set_mints(vec_model(
            wallet
                .mints
                .iter()
                .map(|mint| {
                    let marker = if mint.is_default { "default" } else { "mint" };
                    let balance = if mint.balance_known {
                        format!(" · {}", d::text_or(&mint.balance_text, &d::format_msat(mint.balance_msat)))
                    } else {
                        String::new()
                    };
                    let row = format!("{marker} · {}{balance}", mint.url);
                    SharedString::from(row)
                })
                .collect::<Vec<SharedString>>(),
        ));

        // Sell access page
        let seller = &state.paid_exit_seller;
        app.set_seller_enabled(seller.enabled);
        app.set_seller_status(if !seller.status_text.trim().is_empty() {
            seller
                .status_text
                .replace("Paid exit selling", "Selling internet")
                .replace("paid exit selling", "selling internet")
        } else if seller.supported {
            "People can pay to use my internet".to_string()
        } else {
            "This platform cannot sell public internet access".to_string()
        }
        .into());
        app.set_credit_text(format!(
            "{}: {}",
            d::text_or(&seller.channel_credit_title_text, "Pending buyer credit"),
            d::text_or(&seller.channel_credit_text, &d::format_msat(seller.channel_credit_msat))
        )
        .into());
        let credit_help = d::text_or(
            &seller.channel_credit_help_text,
            if seller.channel_credit_msat > 0 {
                "Collect to move it into wallet"
            } else {
                ""
            },
        );
        app.set_credit_help(credit_help.clone().into());
        app.set_has_credit_help(!credit_help.trim().is_empty());
        let seller_payment_status = if seller_payment_action_empty(state) {
            String::new()
        } else {
            format!(
                "Payments: {}",
                d::text_or(
                    &state.paid_route_market.last_payment_action.status_text,
                    &d::payment_action_title(&state.paid_route_market.last_payment_action.kind),
                )
            )
        };
        app.set_payment_status(seller_payment_status.clone().into());
        app.set_has_payment_status(!seller_payment_status.trim().is_empty());
        app.set_seller_summary(format!(
            "{} · {}",
            d::text_or(&seller.country_code, "Country unset"),
            seller.price_text
        )
        .into());
        let price_draft = app.get_seller_price().to_string();
        app.set_can_save_price(
            !action_in_flight && seller_visible && parse_positive_u64(&price_draft).is_some(),
        );
        app.set_seller_provider_link(seller.provider_link.clone().into());
        app.set_has_provider_link(!seller.provider_link.trim().is_empty());
        app.set_trial_text(format!(
            "Free test: {} · Grace: {}",
            d::text_or(&seller.free_probe_text, &d::traffic_unit_text(seller.free_probe_units)),
            d::text_or(&seller.grace_text, &d::traffic_unit_text(seller.grace_units)),
        )
        .into());
        app.set_expiry_text(format!(
            "Channel expires: {}",
            d::text_or(&seller.channel_expiry_text, &d::format_remaining(seller.channel_expiry_secs))
        )
        .into());
        app.set_seller_settlement(seller.settlement_text.clone().into());
        app.set_has_seller_settlement(!seller.settlement_text.trim().is_empty());
        app.set_public_ip_text(if seller.public_ip_text.trim().is_empty() {
            String::new()
        } else {
            format!("Public IP: {}", seller.public_ip_text)
        }
        .into());
        app.set_has_public_ip(!seller.public_ip_text.trim().is_empty());
        app.set_mints_text(if seller.accepted_mints.is_empty() {
            "No accepted mints configured".to_string()
        } else {
            format!("Accepted mints: {}", seller.accepted_mints.join(", "))
        }
        .into());
        let mut session_pieces = vec![
            format!("{} connected", seller.current_connection_count),
            format!("{} past", seller.past_connection_count),
            d::text_or(&seller.total_traffic_text, &format!("{} routed", d::format_bytes(seller.total_billable_bytes))),
            format!(
                "{} paid",
                d::text_or(&seller.total_paid_text, &d::format_msat(seller.total_paid_msat))
            ),
            format!(
                "{} due",
                d::text_or(&seller.total_due_text, &d::format_msat(seller.total_due_msat))
            ),
        ];
        if seller.total_unpaid_msat > 0 {
            session_pieces.push(format!(
                "{} behind",
                d::text_or(&seller.total_unpaid_text, &d::format_msat(seller.total_unpaid_msat))
            ));
        }
        app.set_seller_sessions_text(session_pieces.join(" · ").into());
        app.set_customers(vec_model(
            seller.sessions.iter().map(|session| CustomerRow {
                title: session_display_title(session).into(),
                detail: session_display_detail(session).into(),
                paid: if session.paid_text.trim().is_empty() {
                    format!("{} paid", d::format_msat(session.paid_msat))
                } else {
                    session.paid_text.clone()
                }
                .into(),
                behind: if session.unpaid_text.trim().is_empty() {
                    if session.unpaid_msat > 0 {
                        format!("{} behind", d::format_msat(session.unpaid_msat))
                    } else {
                        String::new()
                    }
                } else {
                    session.unpaid_text.clone()
                }
                .into(),
                metric: d::metric_text(&session.quality_text, &session.bandwidth_text).into(),
                settlement: session.settlement_text.clone().into(),
            }).collect(),
        ));

        // Settings page
        app.set_autoconnect(state.autoconnect);
        app.set_launch_startup(state.launch_on_startup);
        app.set_close_tray(state.close_to_tray_on_close);
        app.set_fiat_enabled(state.wallet_fiat_enabled);
        app.set_fiat_currency(state.wallet_fiat_currency.clone().into());
        app.set_relays(vec_model(
            state
                .relays
                .iter()
                .map(|relay| RelayRow {
                    url: relay.url.clone().into(),
                    enabled: relay.enabled,
                    connected: relay.enabled && relay.status.eq_ignore_ascii_case("connected"),
                })
                .collect(),
        ));
        app.set_fips_non_roster(state.connect_to_non_roster_fips_peers);
        app.set_fips_nostr_discovery(state.fips_nostr_discovery_enabled);
        app.set_fips_bootstrap(state.fips_bootstrap_enabled);
        app.set_fips_host_tunnel(state.fips_host_tunnel_enabled);
        app.set_fips_address(if state.own_npub.trim().is_empty() {
            String::new()
        } else {
            format!("{}.fips", state.own_npub)
        }
        .into());
        app.set_service_summary(if state.service_installed {
            "Service installed"
        } else {
            "Service missing"
        }
        .into());
        app.set_cli_summary(if state.cli_installed {
            "CLI installed"
        } else {
            "CLI missing"
        }
        .into());
        app.set_system_version_label(system_version_label(state).into());
        app.set_service_supported(state.service_supported);
        app.set_service_enablement_supported(state.service_enablement_supported);
        app.set_cli_install_supported(state.cli_install_supported);
        app.set_diagnostics(vec_model(diagnostic_rows(state)));
        app.set_health(vec_model(
            state
                .health
                .iter()
                .map(|issue| HealthRow {
                    severity: issue.severity.clone().into(),
                    summary: issue.summary.clone().into(),
                    detail: issue.detail.clone().into(),
                })
                .collect(),
        ));

        let can_manage_network = active.is_some_and(|network| network.local_is_admin);
        let name_draft = app.get_network_name_draft().to_string();
        app.set_can_save_network_name(can_manage_network && !name_draft.trim().is_empty() && !action_in_flight);
        let mesh_draft = app.get_network_mesh_draft().to_string();
        app.set_can_save_mesh(
            can_manage_network
                && !d::normalize_network_id_input(&mesh_draft).trim().is_empty()
                && !action_in_flight,
        );
        app.set_inactive_networks(vec_model(
            state
                .networks
                .iter()
                .filter(|network| !network.enabled)
                .map(|network| NetworkRow {
                    id: network.id.clone().into(),
                    name: d::display_network_name(&network.name).into(),
                    enabled: false,
                })
                .collect(),
        ));

        if sync_drafts {
            self.sync_drafts(app, state, active);
        }
    }

    fn sync_drafts(
        &mut self,
        app: &App,
        state: &NativeAppState,
        active: Option<&NativeNetworkState>,
    ) {
        app.set_node_name(state.node_name.clone().into());
        app.set_endpoint(state.endpoint.clone().into());
        app.set_tunnel_ip(state.tunnel_ip.clone().into());
        app.set_listen_port(state.listen_port.to_string().into());
        app.set_fips_ports(state.fips_host_inbound_tcp_ports.clone().into());
        app.set_wg_config(state.wireguard_exit_config.clone().into());
        app.set_dns_mode(state.exit_dns_mode.clone().into());
        app.set_dns_provider(state.exit_dns_doh_provider.clone().into());
        app.set_dns_custom_url(state.exit_dns_custom_doh_url.clone().into());
        app.set_dns_bootstrap(state.exit_dns_custom_doh_bootstrap_ips.clone().into());
        app.set_dns_through(state.exit_dns_through_exit_servers.clone().into());
        if app.get_mint_url().trim().is_empty() {
            app.set_mint_url(state.paid_route_market.wallet.default_mint.clone().into());
        }
        if self.last_synced_manual_provider.as_deref() != Some(state.paid_route_market.manual_provider_link.as_str()) {
            let draft = app.get_manual_provider().to_string();
            if self.last_synced_manual_provider.is_none()
                || draft == self.last_synced_manual_provider.clone().unwrap_or_default()
            {
                app.set_manual_provider(state.paid_route_market.manual_provider_link.clone().into());
            }
            self.last_synced_manual_provider = Some(state.paid_route_market.manual_provider_link.clone());
        }
        if !self.filter_editing {
            let filter = &state.paid_route_market.filter;
            app.set_filter_country(filter.country_code.to_uppercase().into());
            app.set_filter_sort(
                if filter.sort == "price" || filter.sort == "newest" {
                    filter.sort.clone()
                } else {
                    "quality".to_string()
                }
                .into(),
            );
            app.set_filter_v4(filter.require_ipv4);
            app.set_filter_v6(filter.require_ipv6);
        }
        if self.last_synced_price_msat_per_gb != Some(state.paid_exit_seller.price_msat_per_gb) {
            let draft = app.get_seller_price().to_string();
            if self.last_synced_price_msat_per_gb.is_none()
                || draft
                    == self
                        .last_synced_price_msat_per_gb
                        .unwrap_or_default()
                        .to_string()
            {
                app.set_seller_price(state.paid_exit_seller.price_msat_per_gb.to_string().into());
            }
            self.last_synced_price_msat_per_gb = Some(state.paid_exit_seller.price_msat_per_gb);
        }
        app.set_network_name_draft(
            active
                .map(|network| network.name.clone())
                .unwrap_or_default()
                .into(),
        );
        app.set_network_mesh_draft(
            active
                .map(|network| d::display_network_id(&network.network_id))
                .unwrap_or_default()
                .into(),
        );
    }

    pub fn begin_filter_editing(&mut self) {
        self.filter_editing = true;
    }

    pub fn force_sync_filter(&mut self, app: &App, state: &NativeAppState) {
        self.filter_editing = false;
        let filter = &state.paid_route_market.filter;
        app.set_filter_country(filter.country_code.to_uppercase().into());
        app.set_filter_sort(
            if filter.sort == "price" || filter.sort == "newest" {
                filter.sort.clone()
            } else {
                "quality".to_string()
            }
            .into(),
        );
        app.set_filter_v4(filter.require_ipv4);
        app.set_filter_v6(filter.require_ipv6);
    }
}

fn seller_payment_action_empty(state: &NativeAppState) -> bool {
    let action = &state.paid_route_market.last_payment_action;
    action.kind.trim().is_empty() && action.status_text.trim().is_empty()
}

fn system_version_label(state: &NativeAppState) -> String {
    let app = state.app_version.trim();
    let daemon = state.daemon_binary_version.trim();
    match (app.is_empty(), daemon.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!("gui v{app}"),
        (true, false) => format!("daemon v{daemon}"),
        (false, false) if app == daemon => format!("v{app}"),
        (false, false) => format!("gui v{app} · daemon v{daemon}"),
    }
}

fn diagnostic_rows(state: &NativeAppState) -> Vec<DiagRow> {
    let interface = if state.network.default_interface.trim().is_empty() {
        "unknown"
    } else {
        &state.network.default_interface
    };
    let ipv4 = if state.network.primary_ipv4.trim().is_empty() {
        "-"
    } else {
        &state.network.primary_ipv4
    };
    let ipv6 = if state.network.primary_ipv6.trim().is_empty() {
        "-"
    } else {
        &state.network.primary_ipv6
    };
    let gateway = d::first_non_empty(&[
        &state.network.gateway_ipv4,
        &state.network.gateway_ipv6,
        "unknown",
    ]);
    let mapping = if state.port_mapping.active_protocol.trim().is_empty() {
        "none"
    } else {
        &state.port_mapping.active_protocol
    };
    let external = if state.port_mapping.external_endpoint.trim().is_empty() {
        "stun/direct"
    } else {
        &state.port_mapping.external_endpoint
    };
    vec![
        DiagRow { label: "Interface".into(), value: interface.into() },
        DiagRow { label: "IPv4".into(), value: ipv4.into() },
        DiagRow { label: "IPv6".into(), value: ipv6.into() },
        DiagRow { label: "Gateway".into(), value: gateway.into() },
        DiagRow { label: "Port mapping".into(), value: mapping.into() },
        DiagRow { label: "External".into(), value: external.into() },
        DiagRow {
            label: "Peers".into(),
            value: format!("{}/{}", state.connected_peer_count, state.expected_peer_count).into(),
        },
        DiagRow {
            label: "FIPS peers".into(),
            value: format!("{}/{} direct", state.fips_connected_peer_count, state.fips_roster_peer_count).into(),
        },
        DiagRow {
            label: "Other FIPS".into(),
            value: format!("{}", state.non_fips_roster_peer_count).into(),
        },
    ]
}

/// Pushes tray icon state; mirrors TrayService.Update + BuildMenu.
pub fn apply_tray(
    tray: &NostrVpnTray,
    state: &NativeAppState,
    bridge: &Bridge,
    action_in_flight: bool,
) {
    let status = d::vpn_status_text(state);
    let mut tooltip = format!("Nostr VPN - {status}");
    if tooltip.len() > 63 {
        tooltip.truncate(60);
        tooltip.push_str("...");
    }
    tray.set_tray_tooltip(tooltip.into());
    tray.set_vpn_on(state.vpn_enabled);
    tray.set_vpn_toggle_enabled(state.vpn_control_supported && !action_in_flight);
    tray.set_status_text(status.into());
    tray.set_device_name(d::device_display_name(state).into());
    tray.set_can_copy_device(!state.own_npub.trim().is_empty());

    let active = active_network(bridge, state);
    tray.set_has_network(active.is_some());
    tray.set_network_title(
        active
            .map(|network| {
                if network.name.trim().is_empty() {
                    "Network Devices".to_string()
                } else {
                    network.name.clone()
                }
            })
            .unwrap_or_else(|| "Network Devices".to_string())
            .into(),
    );
    if let Some(network) = active {
        tray.set_devices(vec_model(
            network
                .participants
                .iter()
                .map(|participant| {
                    let name = participant_name(participant);
                    let title = if participant.tunnel_ip.trim().is_empty()
                        || participant.tunnel_ip == "-"
                    {
                        name
                    } else {
                        format!("{name} ({})", participant.tunnel_ip)
                    };
                    TrayDeviceRow {
                        title: title.into(),
                        npub: participant.npub.clone().into(),
                    }
                })
                .collect(),
        ));
    } else {
        tray.set_devices(vec_model(Vec::new()));
    }
    tray.set_exit_status(state.exit_node_status_text.clone().into());
    tray.set_share_checked(state.advertise_exit_node);
    tray.set_market_visible(state.paid_route_market.supported);
    let mut exits = Vec::new();
    exits.push(TrayExitRow {
        title: "This device".into(),
        checked: state.internet_source == "direct",
        enabled: true,
        kind: 2,
        arg: "".into(),
    });
    if state.paid_route_market.supported {
        exits.push(TrayExitRow {
            title: "Paid · Automatic · Experimental".into(),
            checked: state.internet_source == "paid_automatic",
            enabled: true,
            kind: 3,
            arg: "".into(),
        });
        exits.push(TrayExitRow {
            title: "Paid · Choose manually".into(),
            checked: state.internet_source == "paid_manual",
            enabled: true,
            kind: 4,
            arg: "".into(),
        });
    }
    exits.push(TrayExitRow {
        title: "WireGuard VPN".into(),
        checked: state.internet_source == "wireguard",
        enabled: state.wireguard_exit_configured,
        kind: 5,
        arg: "".into(),
    });
    if let Some(network) = active {
        let mut peers: Vec<&NativeParticipantState> = network
            .participants
            .iter()
            .filter(|participant| participant.offers_exit_node && !participant_is_self(participant, &state.own_npub))
            .collect();
        peers.sort_by_key(|participant| {
            d::participant_display_name(
                &participant.magic_dns_name,
                &participant.alias,
                &participant.magic_dns_alias,
                &participant.npub,
            )
            .to_lowercase()
        });
        for participant in peers {
            exits.push(TrayExitRow {
                title: participant_name(participant).into(),
                checked: state.internet_source == "private_vpn" && state.exit_node == participant.npub,
                enabled: true,
                kind: 6,
                arg: participant.npub.clone().into(),
            });
        }
    }
    tray.set_exits(vec_model(exits));
}
