// Headless update e2e mode (`--nvpn-e2e-update-check`), mirroring the WPF
// App.RunUpdateE2EAsync JSON shape so the shared release pipeline scripts can
// drive either shell without changes.

use std::io::Write;
use std::path::PathBuf;

use crate::updater;

pub const UPDATE_CHECK_FLAG: &str = "--nvpn-e2e-update-check";
pub const INSTALL_UPDATE_FLAG: &str = "--nvpn-e2e-install-update";

pub fn is_requested(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg.eq_ignore_ascii_case(UPDATE_CHECK_FLAG))
}

/// Runs the headless check (and optional download), writes the JSON result to
/// NVPN_UPDATE_E2E_RESULT_PATH or stdout, and returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    let result_path = std::env::var("NVPN_UPDATE_E2E_RESULT_PATH")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let install = args
        .iter()
        .any(|arg| arg.eq_ignore_ascii_case(INSTALL_UPDATE_FLAG));
    let (ok, json) = run_json(install);
    match result_path {
        Some(path) => {
            let path = PathBuf::from(path);
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(error) =
                std::fs::File::create(&path).and_then(|mut file| file.write_all(json.as_bytes()))
            {
                eprintln!("could not write e2e result: {error}");
                return 1;
            }
        }
        None => println!("{json}"),
    }
    if ok {
        0
    } else {
        1
    }
}

fn run_json(install: bool) -> (bool, String) {
    let current_version = std::env::var("NVPN_UPDATE_E2E_CURRENT_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    let config_path = crate::platform::data_dir().join("config.toml");
    let config = config_path.to_string_lossy().into_owned();

    let outcome = (|| -> Result<String, String> {
        let check = updater::check_blocking(&current_version, &config)?;
        let mut fields = vec![
            "\"ok\": true".to_string(),
            "\"platform\": \"windows\"".to_string(),
            format!("\"available\": {}", check.available),
            format!("\"tag\": {}", quote(&check.tag)),
            format!("\"assetName\": {}", quote(&check.asset_name)),
        ];
        match &check.asset_url {
            Some(url) => fields.push(format!("\"assetUrl\": {}", quote(url))),
            None => fields.push("\"assetUrl\": null".to_string()),
        }
        if install {
            if !check.installable {
                return Err("no Windows update asset selected".to_string());
            }
            let asset = check
                .asset
                .clone()
                .ok_or_else(|| "no Windows update asset selected".to_string())?;
            let path = updater::download_blocking(&current_version, &asset, &config)?;
            let bytes = std::fs::metadata(&path).map_err(|error| error.to_string())?.len();
            fields.push(format!("\"downloadedPath\": {}", quote(&path.to_string_lossy())));
            fields.push(format!("\"downloadedBytes\": {bytes}"));
        }
        Ok(format!("{{\n  {}\n}}", fields.join(",\n  ")))
    })();

    match outcome {
        Ok(json) => (true, json),
        Err(error) => (
            false,
            format!(
                "{{\n  \"ok\": false,\n  \"platform\": \"windows\",\n  \"error\": {}\n}}",
                quote(&error)
            ),
        ),
    }
}

fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
