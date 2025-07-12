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
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread::{self, sleep};
use std::time::{Duration, Instant};

/// Configuration structure for aw-watcher-network
#[derive(Debug, Serialize, Deserialize)]
struct AppConfig {
    /// Polling interval in seconds
    #[serde(default = "default_polling_interval")]
    polling_interval: u64,

    /// Wi-Fi SSID scanning interval in seconds
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[serde(default = "default_wifi_scan_interval")]
    wifi_scan_interval: u64,
}

fn default_polling_interval() -> u64 {
    5
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn default_wifi_scan_interval() -> u64 {
    300 // 5 minutes
}

impl AppConfig {
    fn new() -> Result<Self, ConfigError> {
        // Start with default configuration
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let default_config = Self {
            polling_interval: default_polling_interval(),
            wifi_scan_interval: default_wifi_scan_interval(),
        };

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        let default_config = Self {
            polling_interval: default_polling_interval(),
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
        if let Some(path) = config_path {
            if path.exists() {
                builder = builder.add_source(File::from(path));
            }
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
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            let config = AppConfig {
                polling_interval: default_polling_interval(),
                wifi_scan_interval: default_wifi_scan_interval(),
            };

            #[cfg(not(any(target_os = "macos", target_os = "linux")))]
            let config = AppConfig {
                polling_interval: default_polling_interval(),
            };

            config
        }
    };

    let polling_interval = config.polling_interval;

    // Get hostname and create bucket ID with hostname appended
    let hostname = match get_hostname() {
        Ok(name) => name.to_string_lossy().into_owned(),
        Err(_) => "unknown-host".to_string(),
    };

    let bucket_id = format!("aw-watcher-network_{}", hostname);
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let wifi_bucket_id = format!("aw-watcher-wifi_{}", hostname);
    let event_type = "network-status";
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let wifi_event_type = "wifi-status";

    println!(
        "Starting aw-watcher-network-rs with polling interval of {} seconds",
        polling_interval
    );
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    println!(
        "Wi-Fi SSID scanning interval: {} seconds",
        config.wifi_scan_interval
    );
    println!("Using bucket ID: {}", bucket_id);
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    println!("Using Wi-Fi bucket ID: {}", wifi_bucket_id);

    let client = AwClient::new("localhost", 5600, "aw-watcher-network").unwrap();

    // Create or get buckets
    client
        .create_bucket_simple(&bucket_id, event_type)
        .expect("Failed to create network bucket");

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    client
        .create_bucket_simple(&wifi_bucket_id, wifi_event_type)
        .expect("Failed to create Wi-Fi bucket");

    // Start Wi-Fi SSID scanning thread on supported platforms
    #[cfg(any(target_os = "macos", target_os = "linux"))]
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
        if let Ok(mut addrs) = target.to_socket_addrs() {
            if let Some(addr) = addrs.next() {
                if TcpStream::connect_timeout(&addr, Duration::from_secs(1)).is_ok() {
                    return true;
                }
            }
        }
    }
    false
}

/// Function to watch for Wi-Fi SSIDs in a separate thread
#[cfg(any(target_os = "macos", target_os = "linux"))]
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
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn get_wifi_ssids() -> Result<(Option<String>, Vec<String>), String> {
    #[cfg(target_os = "macos")]
    {
        get_wifi_ssids_macos()
    }

    #[cfg(target_os = "linux")]
    {
        get_wifi_ssids_linux()
    }
}

#[cfg(target_os = "macos")]
fn get_wifi_ssids_macos() -> Result<(Option<String>, Vec<String>), String> {
    // Check if Wi-Fi is enabled
    let wifi_status = Command::new("networksetup")
        .args(&["-getairportpower", "en0"])
        .output()
        .map_err(|e| format!("Failed to check Wi-Fi status: {}", e))?;

    let status_str = String::from_utf8_lossy(&wifi_status.stdout);
    let wifi_enabled = status_str.contains("On");

    let mut wifi_was_disabled = false;

    // Turn on Wi-Fi if it's off
    if !wifi_enabled {
        wifi_was_disabled = true;
        Command::new("networksetup")
            .args(&["-setairportpower", "en0", "on"])
            .output()
            .map_err(|e| format!("Failed to enable Wi-Fi: {}", e))?;

        // Wait a moment for Wi-Fi to initialize
        sleep(Duration::from_secs(2));
    }

    // Use system_profiler to get Wi-Fi information
    let scan_output = Command::new("system_profiler")
        .args(&["SPAirPortDataType"])
        .output()
        .map_err(|e| format!("Failed to scan Wi-Fi networks: {}", e))?;

    // Restore previous Wi-Fi state if it was disabled
    if wifi_was_disabled {
        Command::new("networksetup")
            .args(&["-setairportpower", "en0", "off"])
            .output()
            .ok(); // Ignore errors here
    }

    // Parse the output
    let output_str = String::from_utf8_lossy(&scan_output.stdout);
    parse_wifi_output_macos(&output_str)
}

#[cfg(target_os = "macos")]
fn parse_wifi_output_macos(output: &str) -> Result<(Option<String>, Vec<String>), String> {
    // Use a regex to find SSIDs in the system_profiler output
    // This pattern looks for indented lines that end with a colon, following Network Information sections
    let ssid_regex =
        Regex::new(r"(?m)^\s+(.*?):\s*$").map_err(|e| format!("Invalid regex: {}", e))?;

    // Known section headers that aren't SSIDs
    let non_ssids = HashSet::from([
        "Current Network Information",
        "Other Local Wi-Fi Networks",
        "PHY Mode",
        "Channel",
        "Country Code",
        "Network Type",
        "Security",
        "Signal / Noise",
        "Transmit Rate",
        "MCS Index",
        "Software Versions",
        "CoreWLAN",
        "CoreWLANKit",
        "Menu Extra",
        "System Information",
        "IO80211 Family",
        "Diagnostics",
        "AirPort Utility",
    ]);

    // Known interfaces that aren't SSIDs
    let non_ssid_interfaces = HashSet::from([
        "awdl0",
        "llw0",
        "en0",
        "en1",
        "en2",
        "en3",
        "en4",
        "en5",
        "Wi-Fi",
        "Interfaces",
        "Card Type",
        "Firmware Version",
        "MAC Address",
        "Locale",
        "Country Code",
        "Supported PHY Modes",
        "Supported Channels",
        "Wake On Wireless",
        "AirDrop",
        "Auto Unlock",
        "Status",
    ]);

    // Collect unique SSIDs
    let mut ssids = HashSet::new();
    let mut current_ssid: Option<String> = None;
    let mut in_current_network_section = false;

    // Use regex to extract potential SSIDs
    for line in output.lines() {
        // Check if we're entering the current network section
        if line.contains("Current Network Information:") {
            in_current_network_section = true;
            continue;
        } else if line.contains("Other Local Wi-Fi Networks:") {
            in_current_network_section = false;
        }

        // Process lines in the current section
        if in_current_network_section {
            let trimmed = line.trim();
            if trimmed.ends_with(':') {
                let potential_ssid = trimmed.trim_end_matches(':').trim();
                if !potential_ssid.is_empty()
                    && !non_ssids.contains(potential_ssid)
                    && !non_ssid_interfaces.contains(potential_ssid)
                    && !potential_ssid.starts_with("en")
                {
                    // Found the connected SSID
                    current_ssid = Some(potential_ssid.to_string());
                    // Also add to the list of available SSIDs
                    ssids.insert(potential_ssid.to_string());
                }
            }
        }
    }

    // Now collect all SSIDs from the entire output
    for cap in ssid_regex.captures_iter(output) {
        if let Some(m) = cap.get(1) {
            let potential_ssid = m.as_str().trim();

            // Skip if it's a known non-SSID
            if potential_ssid.is_empty()
                || non_ssids.contains(potential_ssid)
                || non_ssid_interfaces.contains(potential_ssid)
                || potential_ssid.starts_with("en")
            {
                continue;
            }

            // Found an SSID
            ssids.insert(potential_ssid.to_string());
        }
    }

    // Convert to sorted Vec
    let mut ssids_vec: Vec<String> = ssids.into_iter().collect();
    ssids_vec.sort();

    Ok((current_ssid, ssids_vec))
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
