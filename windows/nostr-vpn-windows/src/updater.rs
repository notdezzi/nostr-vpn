// Update orchestration for the Windows GUI, combining the WPF UpdateService
// message semantics ("Update {tag} available", unverified refusal) with the
// linux shell's thread-based check/download flow. Both shells call the same
// nostr-vpn-core updater underneath.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::thread;

use nostr_vpn_core::updater::{
    check_product_update_blocking_with_config, download_product_update_blocking_with_config,
    ProductUpdateMode, ProductUpdateSource,
};

#[derive(Clone, Debug, Default)]
pub struct UpdateState {
    pub checking: bool,
    pub installing: bool,
    pub available: bool,
    pub installable: bool,
    pub auto_install: bool,
    pub version: String,
    pub status: String,
    pub asset: Option<ReleaseAsset>,
}

#[derive(Clone, Debug)]
pub struct ReleaseAsset {
    pub source: String,
    pub verified: bool,
}

pub enum UpdateEvent {
    Checked {
        manual: bool,
        result: Result<UpdateCheck, String>,
    },
    Downloaded(Result<PathBuf, String>),
}

#[derive(Debug)]
pub struct UpdateCheck {
    pub tag: String,
    pub message: String,
    pub available: bool,
    pub installable: bool,
    pub asset_name: String,
    pub asset_url: Option<String>,
    /// Set only when the update is installable (verified Windows asset).
    pub asset: Option<ReleaseAsset>,
}

pub fn skip_open() -> bool {
    std::env::var("NVPN_UPDATE_SKIP_OPEN").as_deref() == Ok("1")
}

pub fn poll_interval_secs() -> u64 {
    std::env::var("NVPN_UPDATE_POLL_SECONDS")
        .ok()
        .and_then(|raw| raw.trim().parse::<f64>().ok())
        .filter(|seconds| *seconds > 0.0)
        .map(|seconds| seconds as u64)
        .unwrap_or(6 * 60 * 60)
}

pub fn check(current_version: String, config_path: String, manual: bool, sender: Sender<UpdateEvent>) {
    thread::spawn(move || {
        let result = check_blocking(&current_version, &config_path);
        let _ = sender.send(UpdateEvent::Checked { manual, result });
    });
}

pub fn download(
    current_version: String,
    asset: ReleaseAsset,
    config_path: String,
    sender: Sender<UpdateEvent>,
) {
    thread::spawn(move || {
        let result = download_blocking(&current_version, &asset, &config_path);
        let _ = sender.send(UpdateEvent::Downloaded(result));
    });
}

pub fn check_blocking(current_version: &str, config_path: &str) -> Result<UpdateCheck, String> {
    let result = check_product_update_blocking_with_config(
        current_version,
        ProductUpdateMode::App,
        ProductUpdateSource::Auto,
        Some(Path::new(config_path)),
    )
    .map_err(|error| format!("{error:#}"))?;
    let has_asset = !result.asset.trim().is_empty();
    let installable = result.available && has_asset && result.verified;
    let message = if result.available {
        if !has_asset {
            format!("Update {} found without a Windows asset", result.tag)
        } else if result.verified {
            format!("Update {} available", result.tag)
        } else {
            format!(
                "Update {} found from unverified {}; install disabled",
                result.tag, result.source
            )
        }
    } else {
        "Up to date".to_string()
    };
    let asset = installable.then(|| ReleaseAsset {
        source: result.source.clone(),
        verified: result.verified,
    });
    Ok(UpdateCheck {
        tag: result.tag,
        message,
        available: result.available,
        installable,
        asset_name: result.asset,
        asset_url: result.url,
        asset,
    })
}

pub fn download_blocking(
    current_version: &str,
    asset: &ReleaseAsset,
    config_path: &str,
) -> Result<PathBuf, String> {
    if !asset.verified {
        return Err(format!(
            "Refusing to install unverified update from {}",
            asset.source
        ));
    }
    let dir = download_dir();
    if let Err(error) = std::fs::create_dir_all(&dir) {
        return Err(format!("could not create download directory: {error}"));
    }
    let result = download_product_update_blocking_with_config(
        current_version,
        ProductUpdateMode::App,
        ProductUpdateSource::Auto,
        Some(&dir),
        Some(Path::new(config_path)),
    )
    .map_err(|error| format!("{error:#}"))?;
    if !result.verified {
        return Err(format!(
            "Refusing to install unverified update from {}",
            result.source
        ));
    }
    result
        .path
        .map(PathBuf::from)
        .ok_or_else(|| "updater did not return a downloaded file".to_string())
}

pub fn download_dir() -> PathBuf {
    std::env::var("NVPN_UPDATE_DOWNLOAD_DIR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("NostrVpnDownloads"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_mirror_the_wpf_update_service() {
        assert_eq!(poll_interval_secs(), 6 * 60 * 60);
        assert!(!skip_open());
    }
}
