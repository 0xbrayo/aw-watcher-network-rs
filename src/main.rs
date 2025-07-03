use aw_client_rust::blocking::AwClient;
use aw_models::Event;
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::{Map, Value};
use std::net::{TcpStream, ToSocketAddrs};
use std::thread::sleep;
use std::time::Duration;

fn main() {
    // Configuration
    let polling_interval_sec = 5;
    let bucket_id = "aw-watcher-network";
    let event_type = "network-status";

    println!(
        "Starting aw-watcher-network-rs with polling interval of {} seconds",
        polling_interval_sec
    );

    // Initialize the ActivityWatch client
    let client = AwClient::new("localhost", 5600, "aw-watcher-network").unwrap();

    // Create or get bucket
    client
        .create_bucket_simple(bucket_id, event_type)
        .expect("Failed to create bucket");

    // Main loop to check network status periodically
    loop {
        let status = check_network_connectivity();
        // Create and send event
        let mut data_map = Map::new();
        data_map.insert("title".to_string(), Value::String(status.to_string()));

        let event = Event {
            id: None,
            timestamp: Utc::now(),
            duration: ChronoDuration::seconds(0),
            data: data_map,
        };

        match client.heartbeat(bucket_id, &event, 2.0 * polling_interval_sec as f64) {
            Ok(_) => println!("Sent heartbeat: {}", status),
            Err(e) => eprintln!("Failed to send heartbeat: {}", e),
        }

        // Wait for the next check
        sleep(Duration::from_secs(polling_interval_sec));
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
                if let Ok(_) = TcpStream::connect_timeout(&addr, Duration::from_secs(1)) {
                    return true;
                }
            }
        }
    }

    false
}
