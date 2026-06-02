use aw_client_rust::blocking::AwClient;
use aw_models::Event;
use chrono::{TimeDelta, Utc};
use config::{Config, ConfigError, File};
use dirs::config_dir;
use hostname::get as get_hostname;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::fs::{create_dir_all, write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, sleep};
use std::time::{Duration, Instant};

/// Configuration structure for aw-watcher-network
#[derive(Debug, Serialize, Deserialize)]
struct AppConfig {
    /// Polling interval in seconds
    #[serde(default = "default_polling_interval")]
    polling_interval: u64,

    /// Wi-Fi SSID scanning interval in seconds
    #[serde(default = "default_wifi_scan_interval")]
    wifi_scan_interval: u64,
}

fn default_polling_interval() -> u64 {
    5
}

fn default_wifi_scan_interval() -> u64 {
    300 // 5 minutes
}

impl AppConfig {
    fn new() -> Result<Self, ConfigError> {
        let default_config = Self {
            polling_interval: default_polling_interval(),
            wifi_scan_interval: default_wifi_scan_interval(),
        };

        // Get the configuration directory
        let config_path = if let Some(config_dir) = config_dir() {
            let aw_config_dir = config_dir.join("activitywatch").join("aw-watcher-network");

            // Create the directory if it doesn't exist
            create_dir_all(&aw_config_dir).ok();

            let config_file = aw_config_dir.join("config.toml");

            // If the config file doesn't exist, create it with default values
            if !config_file.exists() {
                let default_config_str = toml::to_string_pretty(&default_config).unwrap();
                write(&config_file, default_config_str).ok();
            }

            Some(config_file)
        } else {
            None
        };

        // Build configuration
        let mut builder = Config::builder();

        // Add the config file if it exists
        if let Some(path) = config_path
            && path.exists()
        {
            builder = builder.add_source(File::from(path));
        }

        // Build and deserialize the configuration
        match builder.build()?.try_deserialize() {
            Ok(config) => Ok(config),
            Err(_) => Ok(default_config),
        }
    }
}

fn main() {
    // Load configuration
    let config = match AppConfig::new() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Error loading configuration: {}", e);

            AppConfig {
                polling_interval: default_polling_interval(),
                wifi_scan_interval: default_wifi_scan_interval(),
            }
        }
    };

    let polling_interval = config.polling_interval;

    // Get hostname and create bucket ID with hostname appended
    let hostname = match get_hostname() {
        Ok(name) => name.to_string_lossy().into_owned(),
        Err(_) => "unknown-host".to_string(),
    };

    let bucket_id = format!("aw-watcher-network_{}", hostname);
    let wifi_bucket_id = format!("aw-watcher-wifi_{}", hostname);
    let event_type = "network-status";
    let wifi_event_type = "wifi-status";

    println!(
        "Starting aw-watcher-network-rs with polling interval of {} seconds",
        polling_interval
    );
    println!(
        "Wi-Fi SSID scanning interval: {} seconds",
        config.wifi_scan_interval
    );
    println!("Using bucket ID: {}", bucket_id);
    println!("Using Wi-Fi bucket ID: {}", wifi_bucket_id);

    let client = AwClient::new("localhost", 5600, "aw-watcher-network").unwrap();

    // Create or get buckets
    client
        .create_bucket_simple(&bucket_id, event_type)
        .expect("Failed to create network bucket");

    client
        .create_bucket_simple(&wifi_bucket_id, wifi_event_type)
        .expect("Failed to create Wi-Fi bucket");

    // Start Wi-Fi SSID scanning thread on supported platforms
    {
        let wifi_scan_interval = config.wifi_scan_interval;
        // Create a new client instance for the WiFi thread since AwClient doesn't implement Clone
        let wifi_client = AwClient::new("localhost", 5600, "aw-watcher-network").unwrap();
        let wifi_bucket = wifi_bucket_id.clone();

        let current_ssids: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        // Uncomment this line if you need to access SSIDs from the main thread
        // let ssids_for_main = Arc::clone(&current_ssids);

        thread::spawn(move || {
            wifi_ssid_watcher(wifi_scan_interval, wifi_client, wifi_bucket, current_ssids);
        });
    }

    // Main loop to check network status periodically
    loop {
        // Record the start time of this iteration
        let loop_start = Instant::now();

        let status = check_network_connectivity();
        // Create and send event
        let mut data_map = Map::new();
        data_map.insert(
            "title".to_string(),
            Value::String(if status {
                // the extra space serves a purpose in the coloring in the timeline view.
                "online ".to_string()
            } else {
                "offline".to_string()
            }),
        );

        let event = Event {
            id: None,
            timestamp: Utc::now(),
            duration: TimeDelta::seconds(polling_interval as i64),
            data: data_map,
        };

        match client.heartbeat(&bucket_id, &event, polling_interval as f64) {
            Ok(_) => (),
            Err(e) => eprintln!("Error sending heartbeat: {}", e),
        }

        // Calculate how much time has elapsed in this iteration
        let elapsed = loop_start.elapsed();

        // Calculate the time to sleep to maintain consistent intervals
        if elapsed < Duration::from_secs(polling_interval) {
            let sleep_time = Duration::from_secs(polling_interval) - elapsed;
            sleep(sleep_time);
        } else {
            // If operations took longer than polling_interval, don't sleep
            // but log a warning about the missed interval
            eprintln!(
                "Warning: Operations took longer than polling interval ({:?} > {}s)",
                elapsed, polling_interval
            );
        }
    }
}

/// Check network connectivity by attempting to establish TCP connections to reliable DNS servers
fn check_network_connectivity() -> bool {
    // List of reliable DNS servers to check connectivity against
    let targets = [
        "1.1.1.1:53", // Cloudflare DNS
        "8.8.8.8:53", // Google DNS
        "9.9.9.9:53", // Quad9 DNS
    ];

    for target in targets {
        // Parse the address and attempt to establish a connection
        if let Ok(mut addrs) = target.to_socket_addrs()
            && let Some(addr) = addrs.next()
            && TcpStream::connect_timeout(&addr, Duration::from_secs(1)).is_ok()
        {
            return true;
        }
    }
    false
}

/// Function to watch for Wi-Fi SSIDs in a separate thread
fn wifi_ssid_watcher(
    scan_interval: u64,
    client: AwClient,
    bucket_id: String,
    ssids: Arc<Mutex<Vec<String>>>,
) {
    loop {
        // Record the start time of this iteration
        let loop_start = Instant::now();

        // Get current Wi-Fi SSIDs
        match get_wifi_ssids() {
            Ok((connected_ssid, detected_ssids)) => {
                // Update the shared SSID list
                let mut ssids_guard = ssids.lock().unwrap();
                *ssids_guard = detected_ssids.clone();
                drop(ssids_guard); // Release the lock

                // Create event data
                let mut data_map = Map::new();

                // Add SSIDs as an array
                let ssids_json: Vec<Value> = detected_ssids
                    .iter()
                    .map(|ssid| Value::String(ssid.clone()))
                    .collect();

                data_map.insert("ssids".to_string(), Value::Array(ssids_json));

                // No need to add connected_ssid as a separate field since it's already in the title

                // Set title to connected network or "Not connected"
                let title = match connected_ssid {
                    Some(ssid) => ssid,
                    None => {
                        if detected_ssids.is_empty() {
                            "No Wi-Fi networks".to_string()
                        } else {
                            "Not connected".to_string()
                        }
                    }
                };

                data_map.insert("title".to_string(), Value::String(title));

                // Create and send event
                let event = Event {
                    id: None,
                    timestamp: Utc::now(),
                    duration: TimeDelta::seconds(scan_interval as i64),
                    data: data_map,
                };

                match client.heartbeat(&bucket_id, &event, scan_interval as f64) {
                    Ok(_) => (),
                    Err(e) => eprintln!("Error sending Wi-Fi heartbeat: {}", e),
                }
            }
            Err(e) => {
                eprintln!("Error scanning Wi-Fi networks: {}", e);
            }
        }

        // Calculate how much time has elapsed in this iteration
        let elapsed = loop_start.elapsed();

        // Calculate the time to sleep to maintain consistent intervals
        if elapsed < Duration::from_secs(scan_interval) {
            let sleep_time = Duration::from_secs(scan_interval) - elapsed;
            sleep(sleep_time);
        } else {
            // If operations took longer than scan_interval, don't sleep
            eprintln!(
                "Warning: Wi-Fi scan operations took longer than polling interval ({:?} > {}s)",
                elapsed, scan_interval
            );
        }
    }
}

/// Get available Wi-Fi SSIDs using platform-specific commands
fn get_wifi_ssids() -> Result<(Option<String>, Vec<String>), String> {
    #[cfg(target_os = "macos")]
    {
        get_wifi_ssids_macos()
    }

    #[cfg(target_os = "linux")]
    {
        get_wifi_ssids_linux()
    }
    #[cfg(target_os = "windows")]
    {
        get_wifi_ssids_windows()
    }
}

#[cfg(target_os = "macos")]
fn get_wifi_ssids_macos() -> Result<(Option<String>, Vec<String>), String> {
    // Try the CoreWLAN Swift helper first (the only reliable path on macOS 14+)
    if let Some(result) = try_wifi_helper() {
        return Ok(result);
    }

    // Legacy fallback: networksetup + airport (both redacted on macOS 14+, but kept
    // for older systems or when the helper hasn't been built)
    let wifi_device = get_macos_wifi_device()?;

    let wifi_status = Command::new("networksetup")
        .args(["-getairportpower", &wifi_device])
        .output()
        .map_err(|e| format!("Failed to check Wi-Fi status: {}", e))?;
    let status_str = String::from_utf8_lossy(&wifi_status.stdout);
    let wifi_enabled = status_str.contains("On");

    let mut wifi_was_disabled = false;
    if !wifi_enabled {
        wifi_was_disabled = true;
        Command::new("networksetup")
            .args(["-setairportpower", &wifi_device, "on"])
            .output()
            .map_err(|e| format!("Failed to enable Wi-Fi: {}", e))?;
        sleep(Duration::from_secs(2));
    }

    let connected_output = Command::new("networksetup")
        .args(["-getairportnetwork", &wifi_device])
        .output()
        .map_err(|e| format!("Failed to get current Wi-Fi network: {}", e))?;
    let connected_str = String::from_utf8_lossy(&connected_output.stdout);
    let connected_ssid = parse_connected_ssid_macos(&connected_str);

    let mut ssids: Vec<String> = Vec::new();
    if let Some(ref ssid) = connected_ssid {
        ssids.push(ssid.clone());
    }

    // airport -s is deprecated since macOS 14.4 and only emits a warning; skip if that's all we get
    let airport_path =
        "/System/Library/PrivateFrameworks/Apple80211.framework/Versions/Current/Resources/airport";
    if Path::new(airport_path).exists()
        && let Ok(out) = Command::new(airport_path).args(["-s"]).output()
    {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let is_only_warning = stdout.to_ascii_lowercase().contains("deprecated")
            && stdout.lines().filter(|l| !l.trim().is_empty()).count() <= 3;
        if out.status.success()
            && !is_only_warning
            && let Ok(list) = parse_airport_scan_macos(&stdout)
        {
            let mut set = std::collections::BTreeSet::new();
            for s in ssids.iter() {
                set.insert(s.clone());
            }
            for s in list {
                set.insert(s);
            }
            ssids = set.into_iter().collect();
        }
    }

    if wifi_was_disabled {
        Command::new("networksetup")
            .args(["-setairportpower", &wifi_device, "off"])
            .output()
            .ok();
    }

    Ok((connected_ssid, ssids))
}

/// Locate `aw-wifi-helper.app`: check the stable user install path first, then
/// fall back to a bundle alongside the main executable.
#[cfg(target_os = "macos")]
fn find_wifi_helper_app() -> Option<std::path::PathBuf> {
    // 1. Stable install: ~/Library/Application Support/aw-watcher-network/
    if let Some(home) = std::env::var_os("HOME") {
        let stable = std::path::PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("aw-watcher-network")
            .join("aw-wifi-helper.app");
        if stable.exists() {
            return Some(stable);
        }
    }
    // 2. Same directory as the running binary
    let exe_dir = std::env::current_exe().ok()?;
    let alongside = exe_dir.parent()?.join("aw-wifi-helper.app");
    if alongside.exists() {
        return Some(alongside);
    }
    None
}

/// Call the bundled Swift CoreWLAN helper via `open -W` and parse its JSON output.
///
/// We launch through LaunchServices (`open -W`) rather than executing the binary
/// directly. This gives the helper a proper user-session context so macOS can
/// display the Location Services authorization dialog and persist the TCC grant.
/// The helper writes its JSON result to a temp file (passed as argv[1]).
#[cfg(target_os = "macos")]
fn try_wifi_helper() -> Option<(Option<String>, Vec<String>)> {
    let app_bundle = find_wifi_helper_app()?;

    // Temp file for the helper to write its JSON output into
    let tmp_out = std::env::temp_dir().join(format!("aw-wifi-{}.json", std::process::id()));

    let (tx, rx) = mpsc::channel();
    let bundle = app_bundle.clone();
    let tmp = tmp_out.clone();
    thread::spawn(move || {
        // `open -W -F <app> --args <output_path>`
        //   -W: wait for the app to quit before returning
        //   -F: always open a fresh instance (don't reuse a running one)
        let result = Command::new("open")
            .args(["-W", "-F"])
            .arg(&bundle)
            .arg("--args")
            .arg(&tmp)
            .output();
        let _ = tx.send(result);
    });

    let open_result = rx.recv_timeout(Duration::from_secs(40));
    let json = std::fs::read_to_string(&tmp_out);
    let _ = std::fs::remove_file(&tmp_out);

    match open_result {
        Err(_) => {
            eprintln!("Wi-Fi helper timed out");
            return None;
        }
        Ok(Err(e)) => {
            eprintln!("Wi-Fi helper launch error: {}", e);
            return None;
        }
        Ok(Ok(_)) => {}
    }

    parse_wifi_helper_output(&json.ok()?)
}

#[cfg(target_os = "macos")]
fn parse_wifi_helper_output(json: &str) -> Option<(Option<String>, Vec<String>)> {
    let v: Value = serde_json::from_str(json.trim()).ok()?;
    let connected = v
        .get("connected")
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let networks = v
        .get("networks")
        .and_then(|n| n.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some((connected, networks))
}

#[cfg(target_os = "macos")]
fn get_macos_wifi_device() -> Result<String, String> {
    let output = Command::new("networksetup")
        .args(["-listallhardwareports"])
        .output()
        .map_err(|e| format!("Failed to list hardware ports: {}", e))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let lines = text.lines();
    let mut last_was_wifi = false;
    for line in lines {
        if line.trim().starts_with("Hardware Port: Wi-Fi") {
            last_was_wifi = true;
            continue;
        }
        if last_was_wifi && line.trim().starts_with("Device:") {
            let parts: Vec<&str> = line.splitn(2, ':').collect();
            if parts.len() == 2 {
                let dev = parts[1].trim().to_string();
                if !dev.is_empty() {
                    return Ok(dev);
                }
            }
        }
        if line.trim().is_empty() {
            last_was_wifi = false;
        }
    }
    // Fallback to en0 if not found
    Ok("en0".to_string())
}

#[cfg(target_os = "macos")]
fn parse_connected_ssid_macos(output: &str) -> Option<String> {
    // Expected: "Current Wi-Fi Network: <SSID>"
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("Current Wi-Fi Network:") {
            let ssid = rest.trim();
            if !ssid.is_empty() && !ssid.to_lowercase().contains("redacted") {
                return Some(ssid.to_string());
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn parse_airport_scan_macos(output: &str) -> Result<Vec<String>, String> {
    // airport -s prints a table; SSID is the first column (can contain spaces)
    // We parse by taking characters up to the RSSI column start when possible, otherwise split on two+ spaces
    let split_re = Regex::new(r"\s{2,}").map_err(|e| format!("Regex error: {}", e))?;
    let mut ssids = HashSet::new();
    for (idx, line) in output.lines().enumerate() {
        // Skip header line(s)
        if idx == 0 && line.to_ascii_lowercase().contains("ssid") && line.contains("bssid") {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Heuristic: SSID is before two or more spaces followed by a hex bssid
        // Split by two or more spaces
        let parts: Vec<&str> = split_re.split(trimmed).collect();
        if !parts.is_empty() {
            let ssid = parts[0].trim();
            if !ssid.is_empty() && !ssid.to_lowercase().contains("redacted") {
                ssids.insert(ssid.to_string());
            }
        }
    }
    let mut v: Vec<String> = ssids.into_iter().collect();
    v.sort();
    Ok(v)
}

#[cfg(target_os = "linux")]
fn get_wifi_ssids_linux() -> Result<(Option<String>, Vec<String>), String> {
    // Check if Wi-Fi is enabled (using nmcli)
    let wifi_status = Command::new("nmcli")
        .args(&["radio", "wifi"])
        .output()
        .map_err(|e| format!("Failed to check Wi-Fi status: {}", e))?;

    let status_str = String::from_utf8_lossy(&wifi_status.stdout);
    let wifi_enabled = status_str.trim() == "enabled";

    let mut wifi_was_disabled = false;

    // Turn on Wi-Fi if it's off
    if !wifi_enabled {
        wifi_was_disabled = true;
        Command::new("nmcli")
            .args(&["radio", "wifi", "on"])
            .output()
            .map_err(|e| format!("Failed to enable Wi-Fi: {}", e))?;

        // Wait a moment for Wi-Fi to initialize
        sleep(Duration::from_secs(2));
    }

    // Get currently connected network
    let connected_network = if wifi_enabled || wifi_was_disabled {
        let conn_output = Command::new("nmcli")
            .args(&["-t", "connection", "show", "--active"])
            .output()
            .ok();

        if let Some(output) = conn_output {
            let conn_str = String::from_utf8_lossy(&output.stdout);
            for line in conn_str.lines() {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 3 && parts[2] == "wifi" {
                    // Found connected Wi-Fi network
                    return Ok((Some(parts[0].to_string()), vec![parts[0].to_string()]));
                }
            }
        }
        None
    } else {
        None
    };

    // Try to scan with nmcli first (most common)
    let scan_output = Command::new("nmcli")
        .args(&["-t", "device", "wifi", "list"])
        .output()
        .or_else(|_| {
            // Try with iwlist if nmcli fails
            Command::new("iwlist")
                .args(&["scanning"])
                .output()
                .map_err(|e| format!("Failed to scan Wi-Fi networks: {}", e))
        })?;

    // Restore previous Wi-Fi state if it was disabled
    if wifi_was_disabled {
        Command::new("nmcli")
            .args(&["radio", "wifi", "off"])
            .output()
            .ok(); // Ignore errors here
    }

    // Parse the output
    let output_str = String::from_utf8_lossy(&scan_output.stdout);
    let (_, ssids) = parse_wifi_output_linux(&output_str)?;

    Ok((connected_network, ssids))
}

#[cfg(target_os = "linux")]
fn parse_wifi_output_linux(output: &str) -> Result<(Option<String>, Vec<String>), String> {
    let mut ssids = HashSet::new();
    let mut connected_ssid: Option<String> = None;

    // Check which tool's output we're dealing with
    if output.contains("SSID:") || output.contains(":SSID:") {
        // Parse nmcli output with regex
        let nmcli_regex = Regex::new(r"(?m).*?:.*?:(.*?):")
            .map_err(|e| format!("Invalid regex for nmcli: {}", e))?;

        // Look for the connected network (marked with *)
        let connected_regex = Regex::new(r"(?m).*?\*:.*?:(.*?):")
            .map_err(|e| format!("Invalid regex for nmcli connected: {}", e))?;

        // First try to find the connected network
        for cap in connected_regex.captures_iter(output) {
            if let Some(m) = cap.get(1) {
                let ssid = m.as_str().trim();
                if !ssid.is_empty() {
                    connected_ssid = Some(ssid.to_string());
                    break;
                }
            }
        }

        // Then collect all networks
        for cap in nmcli_regex.captures_iter(output) {
            if let Some(m) = cap.get(1) {
                let ssid = m.as_str().trim();
                if !ssid.is_empty() {
                    ssids.insert(ssid.to_string());
                }
            }
        }
    } else if output.contains("ESSID:") {
        // Parse iwlist output with regex
        let iwlist_regex = Regex::new(r#"ESSID:"([^"]*)"#)
            .map_err(|e| format!("Invalid regex for iwlist: {}", e))?;

        // Parse for currently connected network
        // Note: iwlist doesn't directly show connected state in scan results
        // This will be handled by the nmcli connection check earlier

        for cap in iwlist_regex.captures_iter(output) {
            if let Some(m) = cap.get(1) {
                let ssid = m.as_str().trim();
                if !ssid.is_empty() {
                    ssids.insert(ssid.to_string());
                }
            }
        }
    }

    // Convert to sorted Vec
    let mut ssids_vec: Vec<String> = ssids.into_iter().collect();
    ssids_vec.sort();

    Ok((connected_ssid, ssids_vec))
}

// Windows
#[cfg(target_os = "windows")]
fn get_wifi_ssids_windows() -> Result<(Option<String>, Vec<String>), String> {
    let connected_output = Command::new("powershell")
        .args(&["-Command", "netsh wlan show interfaces"])
        .output()
        .map_err(|e| format!("Failed to run netsh for connected SSID: {}", e))?;

    let connected_str = String::from_utf8_lossy(&connected_output.stdout);
    let connected_ssid = parse_connected_ssid_windows(&connected_str);

    let scan_output = Command::new("powershell")
        .args(&["-Command", "netsh wlan show networks"])
        .output()
        .map_err(|e| format!("Failed to run netsh for networks: {}", e))?;

    let scan_str = String::from_utf8_lossy(&scan_output.stdout);
    let ssids = parse_wifi_output_windows(&scan_str)?;

    Ok((connected_ssid, ssids))
}

#[cfg(target_os = "windows")]
fn parse_connected_ssid_windows(output: &str) -> Option<String> {
    for line in output.lines() {
        if line.trim_start().starts_with("SSID") && !line.contains("BSSID") {
            let parts: Vec<&str> = line.splitn(2, ':').collect();
            if parts.len() == 2 {
                let ssid = parts[1].trim();
                if !ssid.is_empty() && ssid != "N/A" {
                    return Some(ssid.to_string());
                }
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn parse_wifi_output_windows(output: &str) -> Result<Vec<String>, String> {
    let mut ssids = HashSet::new();
    let ssid_regex =
        Regex::new(r"^\s*SSID\s+\d+\s*:\s*(.+)$").map_err(|e| format!("Regex error: {}", e))?;

    for line in output.lines() {
        if let Some(caps) = ssid_regex.captures(line) {
            let ssid = caps[1].trim();
            if !ssid.is_empty() {
                ssids.insert(ssid.to_string());
            }
        }
    }

    let mut ssids_vec: Vec<String> = ssids.into_iter().collect();
    ssids_vec.sort();
    Ok(ssids_vec)
}
