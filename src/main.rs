use aw_client_rust::blocking::AwClient;
use aw_models::Event;
use chrono::{TimeDelta, Utc};
use config::{Config, ConfigError, File};
use dirs::config_dir;
use hostname::get as get_hostname;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs::{create_dir_all, write};
use std::net::{TcpStream, ToSocketAddrs};
use std::thread::sleep;
use std::time::{Duration, Instant};

/// Configuration structure for aw-watcher-network
#[derive(Debug, Serialize, Deserialize)]
struct AppConfig {
    /// Polling interval in seconds
    #[serde(default = "default_polling_interval")]
    polling_interval: u64,
}

fn default_polling_interval() -> u64 {
    5
}

impl AppConfig {
    fn new() -> Result<Self, ConfigError> {
        // Start with default configuration
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
            AppConfig {
                polling_interval: default_polling_interval(),
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
    let event_type = "network-status";

    println!(
        "Starting aw-watcher-network-rs with polling interval of {} seconds",
        polling_interval
    );
    println!("Using bucket ID: {}", bucket_id);

    let client = AwClient::new("localhost", 5600, "aw-watcher-network").unwrap();

    // Create or get bucket
    client
        .create_bucket_simple(&bucket_id, event_type)
        .expect("Failed to create bucket");

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
