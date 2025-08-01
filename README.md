# Solana Validator Switch CLI

Professional-grade CLI tool for ultra-fast Solana validator switching with runtime node status detection, built in Rust.

> **Built by validators, for validators** - Stop losing sleep over manual switches. Get the fastest switch possible.

## 🎥 Demo

![Solana Validator Switch Demo](assets/demo.gif)


## Installation

### Quick Install (Recommended)

```bash
# Auto-detects your platform and installs the latest version
curl -sSL https://raw.githubusercontent.com/huiskylabs/solana-validator-switch/main/install.sh | bash

# After installation, 'svs' is available immediately
svs
```

<details>
<summary>Alternative installation methods (requires Rust and Cargo)</summary>

#### Clone and Run
```bash
git clone https://github.com/huiskylabs/solana-validator-switch
cd solana-validator-switch
cargo run --release
```

#### Install with Cargo
```bash
cargo install --git https://github.com/huiskylabs/solana-validator-switch

# Add to PATH if not already there
export PATH="$HOME/.cargo/bin:$PATH"
svs
```
</details>

## Usage

### Interactive Mode (Recommended)
```bash
svs           # Opens interactive menu
```

### Command Line Mode
```bash
svs status                    # Check validator status
svs status --validator 0      # Check specific validator by index
svs switch                    # Perform validator switch
svs switch --dry-run          # Preview switch without executing
svs switch --validator 1      # Switch specific validator by index
svs test-alert                # Test Telegram alert configuration
svs --version                 # Show version
svs --help                    # Show help
```


## Configuration

```bash
mkdir -p ~/.solana-validator-switch
cp config.example.yaml ~/.solana-validator-switch/config.yaml
nano ~/.solana-validator-switch/config.yaml
```

See [config.example.yaml](config.example.yaml) for the full configuration template.

### Telegram Alerts Setup (Optional)

To enable Telegram notifications:

1. **Create a Telegram Bot**:
   - Message [@BotFather](https://t.me/botfather) on Telegram
   - Send `/newbot` and follow the prompts
   - Save the bot token (looks like `123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11`)

2. **Get Your Chat ID**:
   - Add the bot to a group or start a private chat with it
   - Send a message to the bot
   - Visit `https://api.telegram.org/bot<YOUR_BOT_TOKEN>/getUpdates`
   - Find your chat ID in the response (negative for groups, positive for private chats)

3. **Configure in config.yaml**:
   ```yaml
   alert_config:
     enabled: true
     delinquency_threshold_seconds: 30  # Alert after 30 seconds without voting
     telegram:
       bot_token: "YOUR_BOT_TOKEN"
       chat_id: "YOUR_CHAT_ID"
   ```

4. **Test Your Configuration**:
   ```bash
   svs test-alert
   ```

You'll receive notifications for:
- **Validator Delinquency** (CRITICAL): When your validator stops voting for more than 30 seconds
  - Only triggers when SSH and RPC are both working (no false alarms)
  - Includes SSH and RPC connection status in the alert
- **SSH Connection Failures** (LOW PRIORITY): When SSH connections fail repeatedly
  - Triggers after 100 consecutive failures or 30 minutes of failures
  - Very loose thresholds to avoid noise
- **RPC Connection Failures** (LOW PRIORITY): When RPC calls fail due to throttling or network issues
  - Triggers after 100 consecutive failures or 30 minutes of failures
  - Very loose thresholds to avoid noise
- **Switch Results**: Success/failure notifications with timing details

## Key Features

- **Ultra-Fast Switching**: Get the fastest switch possible with optimized streaming operations
- **Runtime Status Detection**: Automatic active/standby node detection using RPC calls
- **SSH Connection Pooling**: Persistent connections with multiplexing for ultra-fast operations
- **Optimized Tower Transfer**: Streaming base64 decode + dd for minimal latency
- **Universal Support**: Works with Firedancer, Agave, Solana, and Jito validators
- **Interactive Dashboard**: Real-time monitoring with Ratatui-based terminal UI
  - **Ultra-responsive keyboard controls** - Dedicated thread ensures instant response
  - **Multi-validator support** - Tab key to switch between validator pairs
  - Auto-refresh countdown timer shows time until next refresh
  - Manual refresh (R key) with no cooldown
  - SSH connectivity status monitoring
  - RPC node health status via getHealth checks
  - Responsive keyboard shortcuts (Q)uit, (R)efresh, (S)witch, (Tab) cycle validators
- **Telegram Alerts**: Real-time notifications for validator health and switch events
  - Delinquency alerts when validator stops voting
  - Switch success/failure notifications
  - SSH connection failure alerts (after 30 minutes)
  - RPC connection failure alerts (after 30 minutes)
- **Enhanced Status Display**: Streamlined UI with real-time updates
  - Shows active/standby status based on identity
  - Current validator identity display
  - Version information for all validator types
  - SSH connectivity and RPC health indicators

## Security

- **No credential storage**: SSH private keys never leave your `~/.ssh/` directory
- **Path-only configuration**: Only file paths and hostnames stored in config files
- **No network exposure**: Tool operates through SSH connections only
- **Local execution**: All operations run locally, no external services

## Why SVS?

Built by [Huisky Labs](https://huisky.xyz/) validator team who needed reliable switching tools for our own operations. After countless manual switches and near-misses, we built what we wished existed.

- **Battle-tested**: Used in production by Huisky Labs validators
- **Community-driven**: We actively use and improve this tool daily
- **Open source**: Transparency and security through open development

### Support Development

If SVS saves you time and SOL, consider:
- ⭐ Starring this repo to help other validators find it
- 🗳️ Delegating to [Huisky Labs validators](https://huisky.xyz/) 
- 🐛 Reporting issues or contributing improvements

## Roadmap

### ✅ Completed
- [x] **Ultra-fast switching** - Sub-second identity switches with optimized streaming operations
- [x] **Universal validator support** - Works with Firedancer, Agave, Solana, and Jito
- [x] **Interactive CLI** - User-friendly menu system with guided workflows  
- [x] **Dry-run mode** - Test switches without executing for safety
- [x] **SSH connection pooling** - Persistent connections with multiplexing for instant commands
- [x] **Auto-detect active/standby** - Runtime detection of validator states
- [x] **Optimized tower transfer** - Streaming base64 decode + dd for minimal latency
- [x] **Interactive dashboard** - Real-time monitoring with Ratatui-based terminal UI
- [x] **Modern async architecture** - Tokio-based async runtime with Arc<Session> efficiency
- [x] **Telegram notifications** - Real-time alerts for validator health and switch events
- [x] **Continuous monitoring** - Real-time validator health monitoring with delinquency alerts
- [x] **Multi-validator support** - Manage multiple validator pairs with Tab key switching
- [x] **Ultra-responsive UI** - Dedicated keyboard thread prevents blocking, action-based processing

- [x] **Auto-switch on failure** - Automatic failover when primary validator goes down

Have ideas? [Open an issue](https://github.com/huiskylabs/solana-validator-switch/issues) or contribute!

## License

MIT License

---

<div align="center">
Built with ❤️ by <a href="https://huisky.xyz/">Huisky Labs</a> • <a href="https://github.com/huiskylabs">GitHub</a> • <a href="https://twitter.com/huiskylabs">Twitter</a>
</div>