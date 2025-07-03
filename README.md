# aw-watcher-network-rs

A network connectivity watcher for [ActivityWatch](https://activitywatch.net/). This watcher monitors your network status and reports whether your device is online or offline. It creates a bucket with the hostname appended to distinguish between multiple devices.

## Features

- Monitors network connectivity by checking connection to major DNS servers
- Reports online/offline status to ActivityWatch
- Configurable polling interval
- Device-specific buckets with hostname in bucket ID

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

- Linux/macOS: `~/.config/activitywatch/aw-watcher-network-rs.toml`
- Windows: `%APPDATA%\activitywatch\aw-watcher-network-rs.toml`

If this file doesn't exist when the watcher starts, it will be created with default values.

### Configuration Options

| Option | Description | Default |
|--------|-------------|---------|
| `polling_interval` | How often to check network status (in seconds) | `5` |

### Example Configuration

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

The watcher will start sending network connectivity events to your local ActivityWatch server. Events are stored in a bucket named `aw-watcher-network_<hostname>` to distinguish between different devices.

## How It Works

The watcher attempts to establish TCP connections to several reliable DNS servers to determine if your device has internet connectivity. It sends heartbeat events to ActivityWatch with either "online" or "offline" status.

## License

This project is available under the MIT License.