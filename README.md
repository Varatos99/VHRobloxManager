# VHRobloxManager

> **Multi-Roblox Client Manager with AFK Prevention**

A fast, lightweight Roblox account manager built with Rust + [egui](https://github.com/emilk/egui). Manage multiple accounts, launch games, and keep them online with built-in AFK prevention bypass.

## Features

- **Multi-Account Management** - Add, remove, organize Roblox accounts with cookie-based auth
- **Encrypted Storage** - AES-256-GCM encryption (your master password encrypts everything)
- **Multi-Instance** - Launch multiple Roblox clients simultaneously (bypasses singleton mutex)
- **Bulk Launch** - Launch selected accounts into the same game sequentially
- **AFK Prevention** - Built-in ESC heartbeat to prevent kick disconnections (Byfron-compatible)
- **Multi Window Tiling** - Organize Roblox windows in a grid after launch
- **Private Servers** - Create and manage VIP servers
- **Friends System** - View friends, presence (experimental)
- **Group Robux** - Display group funds
- **Games Browser** - Search, popular, favorite games
- **Donate Tab** - Support development with crypto

## Disclaimers

⚠️ **Use at your own risk.** This tool:
- Interacts with Roblox authentication cookies
- Launches multiple Roblox instances (ban risk)
- Bypasses Roblox's singleton mutex
- May conflict with Hyperion/Byfron anti-cheat

This project is **not affiliated with or endorsed by Roblox Corporation**.

## Requirements

- Windows 10/11
- Rust 1.75+ ([rustup.rs](https://rustup.rs/))
- Roblox Player Beta installed

## Building

```bash
# Clone
git clone https://github.com/varatos99/VHRobloxManager.git
cd VHRobloxManager

# Build release
cargo build --release

# Run (after build, exe is at target/release/)
.\target\release\VHRobloxManager.exe
```

## Quick Start

1. **First launch** - Set a master password
2. **Add Account** - Click "+ Add Account", paste your `.ROBLOSECURITY` cookie
3. **Launch** - Select account(s), enter Place ID, click Launch
4. **Multi-select** - Ctrl+click to select multiple accounts
5. **AFK Prevention** - Settings > Multi Tab Management > Start AFK Prevention

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| Settings | Access settings, AFK controls |

## Support / Funding

This is a personal project, maintained as-is.

**Donate (Crypto):** See Donate tab in-app

## Credits & Inspiration

- **AFK Prevention** inspired by [Multi Roblox Tab Manager](https://www.youtube.com/watch?v=7gQOGWBuA4A) by @AsphaltCake
- [RobloxAccountManager](https://github.com/ic3w0lf22/Roblox-Account-Manager) - Original reference project
- [egui](https://github.com/emilk/egui) - UI framework
- [Ram2](https://gitlab.com/centerepic/robloxmanager) - Related project (predecessor fork source)

## License

[MIT](LICENSE) - Use freely, fork allowed, no support guarantee.

---

**Note:** This project may not be actively maintained. Bug reports welcome, fixes may or may not happen.