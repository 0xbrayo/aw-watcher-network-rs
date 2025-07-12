# aw-watcher-network-rs

A comprehensive network monitoring watcher for [ActivityWatch](https://activitywatch.net/). This watcher tracks your network connectivity status, reporting whether your device is online or offline, and on macOS and Linux, also scans for available Wi-Fi networks in your vicinity and identifies your currently connected network. It creates separate, well-organized buckets with the hostname appended to distinguish between multiple devices.

## Features

- Monitors network connectivity by checking connection to major DNS servers
- Reports online/offline status to ActivityWatch
- Scans and reports available Wi-Fi networks in your area (macOS and Linux only)
- Identifies and displays your currently connected Wi-Fi network (macOS and Linux only)
- Configurable polling intervals for both network checks and Wi-Fi scans
- Device-specific buckets with hostname in bucket ID
- Handles Wi-Fi state (turns on if off, then returns to previous state) on supported platforms
- Cross-platform support with native command integration (Wi-Fi features on macOS and Linux only)
- Minimizes system impact by managing Wi-Fi resources efficiently

## Installation

### From Source

1. Clone the repository
2. Build the project with Cargo:
   ```bash
   cargo build --release
   ```
3. The binary will be available at `target/release/aw-watcher-network-rs`

## Configuration

The watcher can be configured using a TOML configuration file located at:

- Linux/macOS: `~/.config/activitywatch/aw-watcher-network/config.toml`
- Windows: `%APPDATA%\activitywatch\aw-watcher-network\config.toml`

If this file doesn't exist when the watcher starts, it will be created automatically with default values. You can modify this file at any time, and changes will be applied the next time the watcher starts.

### Configuration Options

| Option | Description | Default | Platform |
|--------|-------------|---------|----------|
| `polling_interval` | How often to check network status (in seconds) | `5` | All |
| `wifi_scan_interval` | How often to scan for Wi-Fi networks (in seconds) | `300` | macOS, Linux |

### Example Configuration

```toml
# Configuration for aw-watcher-network-rs

# Polling interval in seconds
polling_interval = 10

# Wi-Fi scanning interval in seconds (5 minutes) - macOS and Linux only
# Higher values reduce system resource usage, lower values provide more frequent updates
wifi_scan_interval = 300
```

#### Windows Configuration

On Windows, the configuration is simpler as Wi-Fi scanning is not supported:

```toml
# Configuration for aw-watcher-network-rs

# Polling interval in seconds
polling_interval = 10
```

## Usage

Simply run the executable:

```bash
./aw-watcher-network-rs
```

The watcher will start sending network connectivity events to your local ActivityWatch server (ensure your ActivityWatch server is running). Events are stored in the following buckets:

- `aw-watcher-network_<hostname>` - Contains online/offline connectivity status (all platforms)
- `aw-watcher-wifi_<hostname>` - Contains available Wi-Fi networks and signal information (macOS and Linux only)

This separation allows for better organization, independent querying, and enhanced visualization of different types of network data in the ActivityWatch dashboard.

## How It Works

### Network Connectivity

The watcher attempts to establish TCP connections to several reliable DNS servers to determine if your device has internet connectivity. It sends heartbeat events to ActivityWatch with either "online" or "offline" status.

### Wi-Fi Scanning (macOS and Linux only)

On supported platforms (macOS and Linux), the watcher periodically scans for available Wi-Fi networks and identifies your currently connected network using platform-specific native commands:

- **macOS**: Uses `networksetup` to manage Wi-Fi power state and `system_profiler SPAirPortDataType` to scan for networks and identify the connected network, ensuring compatibility with all macOS versions
- **Linux**: Primarily uses `nmcli` (NetworkManager) with fallback to `iwlist` for broader compatibility across different Linux distributions
- **Windows**: Wi-Fi scanning is not currently supported

If Wi-Fi is disabled, the watcher will:
1. Detect the disabled state
2. Temporarily enable the Wi-Fi interface
3. Perform the scan (with appropriate timeouts)
4. Return the Wi-Fi to its original disabled state

This approach ensures accurate network discovery without permanently altering user-configured Wi-Fi settings or causing unexpected battery drain.

The scan results are sent to ActivityWatch as structured events containing:
- A complete list of all available network SSIDs
- The currently connected Wi-Fi network name as the event title, or "Not connected" if not connected to any network
- Proper deduplication of networks that appear multiple times

Each scan runs in a separate thread from the main connectivity checker, ensuring that long-running scans don't block or interfere with basic connectivity reporting.

## Troubleshooting

### Wi-Fi Scanning Issues (macOS and Linux)

If you encounter issues with Wi-Fi scanning on supported platforms:

- Ensure you have the appropriate permissions to manage network interfaces
- On Linux, make sure either NetworkManager (`nmcli`) or Wireless Tools (`iwlist`) is installed
- On macOS, no additional software is required as the implementation uses built-in system tools

### Windows-Specific Notes

- Wi-Fi scanning is not currently supported on Windows platforms
- Only the network connectivity features are available on Windows

## Contributing

Contributions are welcome! Feel free to submit issues or pull requests to improve functionality.

## License

This project is available under the MIT License.