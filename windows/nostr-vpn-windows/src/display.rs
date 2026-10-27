// Display-text helpers ported from the WPF shell (Models.cs computed
// properties and NativeDisplayText) so both UIs surface identical strings.

use nostr_vpn_app_core::NativeAppState;

pub fn first_non_empty(values: &[&str]) -> String {
    for value in values {
        if !value.trim().is_empty() {
            return (*value).to_string();
        }
    }
    String::new()
}

pub fn text_or(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

pub fn format_msat(msat: u64) -> String {
    if msat == 0 {
        return "0 sat".to_string();
    }
    let whole = msat / 1_000;
    let remainder = msat % 1_000;
    if remainder == 0 {
        format!("{whole} sat")
    } else {
        format!("{whole}.{remainder:03} sat")
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut index = 0;
    while value >= 1024.0 && index < UNITS.len() - 1 {
        value /= 1024.0;
        index += 1;
    }
    if index == 0 {
        return format!("{bytes} B");
    }
    if (value - value.round()).abs() < 0.05 {
        format!("{value:.0} {}", UNITS[index])
    } else {
        format!("{value:.1} {}", UNITS[index])
    }
}

pub fn traffic_unit_text(units: u64) -> String {
    format_bytes(units)
}

pub fn format_duration_ms(ms: u64) -> String {
    if ms < 1_000 {
        return format!("{ms} ms");
    }
    let seconds = ms / 1_000;
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    format!("{}h", minutes / 60)
}

pub fn format_remaining(seconds: u64) -> String {
    if seconds == 0 {
        return "off".to_string();
    }
    let days = seconds / 86_400;
    if days > 0 {
        let hours = seconds % 86_400 / 3_600;
        return if hours == 0 {
            format!("{days}d")
        } else {
            format!("{days}d {hours}h")
        };
    }
    let hours = seconds / 3_600;
    if hours > 0 {
        let minutes = seconds % 3_600 / 60;
        return if minutes == 0 {
            format!("{hours}h")
        } else {
            format!("{hours}h {minutes}m")
        };
    }
    let minutes = seconds / 60;
    if minutes == 0 {
        return format!("{seconds}s");
    }
    let rem_secs = seconds % 60;
    if rem_secs == 0 {
        format!("{minutes}m")
    } else {
        format!("{minutes}m{rem_secs:02}s")
    }
}

pub fn metric_text(quality_text: &str, bandwidth_text: &str) -> String {
    let mut parts = Vec::new();
    for value in [quality_text, bandwidth_text] {
        let text = value.trim();
        if !text.is_empty() && !text.eq_ignore_ascii_case("Quality unmeasured") {
            parts.push(text.to_string());
        }
    }
    parts.join(" · ")
}

pub fn humanize(value: &str) -> String {
    let text = value.replace('_', " ");
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

pub fn plain_status(value: &str, fallback: &str) -> String {
    let raw = if value.trim().is_empty() { fallback } else { value };
    match raw {
        "opening" => "Opening".into(),
        "probing" => "Checking quality".into(),
        "active" => "Active".into(),
        "paused" => "Paused".into(),
        "closed" => "Closed".into(),
        "session" => "Session".into(),
        other => humanize(other),
    }
}

pub fn access_title(value: &str, fallback: &str) -> String {
    match value {
        "paid" => "Paid".into(),
        "free_probe" => "Free test".into(),
        "grace" => "Grace".into(),
        "suspended" => "Paused".into(),
        other => plain_status(other, fallback),
    }
}

pub fn payment_action_title(kind: &str) -> String {
    match kind {
        "send" => "Payment sent".into(),
        "receive" => "Payment received".into(),
        "apply" => "Payment applied".into(),
        "create" => "Payment ready".into(),
        "open_channel" => "Exit funded".into(),
        "sign" => "Payment ready".into(),
        "close" => "Channel settled".into(),
        "stream" => "Payments sent".into(),
        "probe" => "Quality checked".into(),
        "" => String::new(),
        other => humanize(other),
    }
}

pub fn wallet_action_title(kind: &str) -> String {
    match kind {
        "topup" => "Invoice ready".into(),
        "receive" => "Token imported".into(),
        "send" => "Token ready".into(),
        "withdraw" => "Invoice paid".into(),
        "refresh" => "Wallet refreshed".into(),
        "open_channel" => "Exit funded".into(),
        "" => String::new(),
        other => humanize(other),
    }
}

pub fn country_claim_text(claimed: &str, observed: &str, status: &str) -> String {
    match status {
        "match" => {
            if observed.is_empty() && claimed.is_empty() {
                String::new()
            } else if observed.is_empty() {
                format!("{claimed} verified")
            } else {
                format!("{observed} verified")
            }
        }
        "mismatch" => {
            if claimed.is_empty() {
                "country mismatch".to_string()
            } else {
                format!("claimed {claimed}")
            }
        }
        _ => {
            if !observed.is_empty() {
                observed.to_string()
            } else {
                claimed.to_string()
            }
        }
    }
}

pub fn short_text(value: &str, prefix: usize, suffix: usize) -> String {
    if value.len() <= prefix + suffix + 3 {
        return value.to_string();
    }
    format!("{}...{}", &value[..prefix], &value[value.len() - suffix..])
}

pub fn participant_display_name(
    magic_dns_name: &str,
    alias: &str,
    magic_dns_alias: &str,
    npub: &str,
) -> String {
    first_non_empty(&[magic_dns_name, alias, magic_dns_alias, &short_text(npub, 12, 6)])
}

pub fn clean_tunnel_ip(tunnel_ip: &str) -> String {
    tunnel_ip.split('/').next().unwrap_or("").trim().to_string()
}

pub fn participant_connectivity(state: &str, reachable: bool) -> &'static str {
    match state.to_lowercase().as_str() {
        "off" => "Off",
        "local" | "online" | "present" => "Online",
        "pending" => "Connecting",
        "offline" => "Offline",
        _ if reachable => "Online",
        _ => "Unknown",
    }
}

pub fn participant_is_fips_direct(state: &str, reachable: bool, transport_addr: &str) -> bool {
    reachable
        && !state.eq_ignore_ascii_case("local")
        && !transport_addr.trim().is_empty()
}

pub fn participant_is_fips_routed(state: &str, reachable: bool, transport_addr: &str) -> bool {
    reachable
        && !state.eq_ignore_ascii_case("local")
        && transport_addr.trim().is_empty()
}

pub fn participant_fips_path_text(
    state: &str,
    is_direct: bool,
    is_routed: bool,
    transport_type: &str,
    srtt_ms: u64,
) -> String {
    if state.eq_ignore_ascii_case("local") {
        return "This device".to_string();
    }
    if is_direct {
        let transport = if transport_type.is_empty() {
            String::new()
        } else {
            format!(" ({})", transport_type.to_uppercase())
        };
        return if srtt_ms > 0 {
            format!("Direct connection{transport}, {srtt_ms} ms")
        } else {
            format!("Direct connection{transport}")
        };
    }
    if is_routed {
        return if srtt_ms > 0 {
            format!("Via mesh, {srtt_ms} ms")
        } else {
            "Via mesh".to_string()
        };
    }
    if state.eq_ignore_ascii_case("pending") {
        return "Connecting".to_string();
    }
    "Offline".to_string()
}

pub fn participant_role_text(is_self: bool, is_admin: bool, offers_exit: bool) -> String {
    let mut roles = Vec::new();
    if is_self {
        roles.push("This device");
    }
    if is_admin {
        roles.push("Admin");
    }
    if offers_exit {
        roles.push("Shares internet");
    }
    if roles.is_empty() {
        "Member".to_string()
    } else {
        roles.join(", ")
    }
}

fn daemon_starting(state: &NativeAppState) -> bool {
    state.error.is_empty()
        && state.service_running
        && state.vpn_status.trim() == "Background service starting"
}

pub fn vpn_status_text(state: &NativeAppState) -> String {
    if !state.error.is_empty() {
        return state.error.clone();
    }
    if state.exit_node_blocked {
        return if state.exit_node_status_text.is_empty() {
            "Internet blocked".to_string()
        } else {
            state.exit_node_status_text.clone()
        };
    }
    if state.exit_node_active && !state.exit_node_status_text.is_empty() {
        return state.exit_node_status_text.clone();
    }
    if state.vpn_active {
        return if state.vpn_status.is_empty() {
            "VPN on".to_string()
        } else {
            state.vpn_status.clone()
        };
    }
    if state.vpn_enabled {
        return if state.vpn_status.is_empty() {
            "Turning on".to_string()
        } else {
            state.vpn_status.clone()
        };
    }
    if daemon_starting(state) {
        return state.vpn_status.clone();
    }
    "Off".to_string()
}

pub fn device_display_name(state: &NativeAppState) -> String {
    if !state.self_magic_dns_name.trim().is_empty() {
        return state.self_magic_dns_name.clone();
    }
    if !state.node_name.trim().is_empty() {
        return state.node_name.clone();
    }
    let tunnel_ip = state.tunnel_ip.trim();
    if !tunnel_ip.is_empty() && tunnel_ip != "-" {
        return tunnel_ip.to_string();
    }
    "This Device".to_string()
}

pub fn normalize_network_id_input(value: &str) -> String {
    let trimmed = value.trim();
    let compact: String = trimmed
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '-')
        .collect();
    if compact.is_empty() && trimmed.chars().all(|ch| ch.is_whitespace() || ch == '-') {
        return String::new();
    }
    if !compact.is_empty() && compact.chars().all(|ch| ch.is_ascii_hexdigit()) {
        compact.to_lowercase()
    } else {
        trimmed.to_string()
    }
}

pub fn display_network_id(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() <= 4 || !trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return trimmed.to_string();
    }
    trimmed
        .as_bytes()
        .chunks(4)
        .map(|chunk| String::from_utf8_lossy(chunk).to_string())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn display_network_name(name: &str) -> String {
    if name.trim().is_empty() {
        "Private network".to_string()
    } else {
        name.to_string()
    }
}

const BECH32_BODY_CHARSET: &[u8] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";

/// A valid device ID is a bech32-encoded npub: "npub1" + 58 bech32 chars.
pub fn is_valid_device_id(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.len() != 63 || !trimmed.starts_with("npub1") {
        return false;
    }
    trimmed.as_bytes()[5..]
        .iter()
        .all(|byte| BECH32_BODY_CHARSET.contains(byte))
}

pub fn looks_like_join_request(value: &str) -> bool {
    let prefix = "nvpn://join-request/";
    let trimmed = value.trim();
    trimmed
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        && trimmed.len() > prefix.len()
}

pub fn looks_like_cashu_token(value: &str) -> bool {
    let token = value.trim().as_bytes();
    token.len() > 12 && token[..5].eq_ignore_ascii_case(b"cashu")
}

/// "Expires ..." timestamp for wallet invoices, rendered in UTC because the
/// GUI has no locale database (WPF showed the local "g" format).
pub fn format_expiry(unix: u64) -> String {
    let days = unix / 86_400;
    let secs_of_day = unix % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        secs_of_day / 3_600,
        secs_of_day % 3_600 / 60
    )
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}
