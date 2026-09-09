# Home Lab Manager

A self-hosted home lab inventory that runs as **one Linux executable** with a **local redb database**. The complete English-language React interface is embedded in the binary. No Docker, Node.js runtime, external database server, cloud account, or OpenAI account is required to use it.

**Status: MVP, v0.1.0.** This is a manually maintained inventory, not a live network monitor.

## Features

- Create, edit, and delete devices: name, model, type, IPv4 address, location, notes, and manually recorded operational status.
- Describe parent connections and display the resulting network topology.
- Define IPv4 subnets with arbitrary CIDR prefixes (`/0` through `/32`), an optional gateway, and an optional VLAN ID.
- Keep a manual register of TCP/UDP ports, services, access paths, and Cloudflare Tunnel URLs.
- Inspect assigned and available addresses and allocate the next available IP.
- Record installed and target firmware versions.
- Schedule in-app firmware checks, including due-today and overdue indicators.
- Sign in with one local administrator account.
- Initialize the database, change the password, back up, and restore from the command line.
- Store all application data locally and keep it across application updates.

### MVP boundaries

- No network scanning, ping monitoring, SNMP polling, automatic firmware discovery, or automatic firmware installation.
- Port and tunnel records are manually maintained; this version does not inspect listeners, firewalls, or Cloudflare configuration.
- No email, push, or desktop notifications. Reminders are displayed when you open the application; overdue status is calculated from your browser's local date.
- IPv4 only; one address and at most one parent connection per device.
- Subnets cannot overlap, even when they have different VLAN IDs. VRFs and overlapping address spaces are not supported.
- Every IPv4 address is unique across the inventory.
- Firmware versions are compared for equality only, not semantic ordering. A different target version means **planned**, not **confirmed newer**.
- One administrator. There is no self-registration or multi-user authorization.
- No built-in TLS. Use it on localhost or your trusted LAN/VPN. See [Network access](#network-access).

## Supported builds

| Download/build target | Intended systems |
| --- | --- |
| `x86_64-unknown-linux-musl` | 64-bit Intel/AMD Linux, including Ubuntu and NixOS |
| `aarch64-unknown-linux-musl` | 64-bit ARM Linux, including Raspberry Pi OS 64-bit and ARM64 NixOS |

These are separate, statically linked executables. An x86_64 executable cannot run on ARM64. A 64-bit Raspberry Pi CPU still needs a **64-bit operating system**. 32-bit Raspberry Pi OS is not supported in this release.

The build uses musl and Rust's self-contained linker rather than the host's glibc. A compatible Linux kernel, filesystem, and CPU are still required. Cross-compilation is not a substitute for testing on every distribution and physical device.

## Download

Open the repository's [Actions](https://github.com/sound-lime/homelab-manager/actions) page, select a successful **CI** run on `main`, and download the artifact for your architecture. Artifacts contain `homelab`, this README, the systemd example, and a SHA-256 checksum. Downloading Actions artifacts requires a GitHub account; running the application does not.

Tagged releases also publish archives through the release workflow. If the [Releases](https://github.com/sound-lime/homelab-manager/releases) page is empty, use Actions artifacts or build from source below. Actions artifacts are retained for 30 days; keep your own copy.

Verify the downloaded executable:

```bash
sha256sum --check SHA256SUMS
chmod +x homelab
./homelab --version
```

## First run

Place the executable in a writable application directory, for example `/opt/homelab` or `~/homelab`. By default, application data lives in `data/` **beside the executable**, not in the shell's current working directory.

1. Initialize the administrator account:

   ```bash
   ./homelab init
   ```

   The default username is `admin`. To choose another:

   ```bash
   ./homelab init --username slava
   ```

   Enter and repeat a password when prompted. Input is hidden. Passwords must be 6–256 UTF-8 bytes. There is no default password and no password in the source code or binary.

2. Start the server:

   ```bash
   ./homelab serve
   ```

3. Open `http://127.0.0.1:8088` and sign in.

4. Add your subnets under **IP plan**, then add your devices. Choose **Connected to** in a device card to build the map. Record target firmware and a **Next check date** to create a reminder.

The initial inventory is empty. No demo devices are written to your database.

`init` refuses to overwrite an existing database. Stopping the server with Ctrl+C or SIGTERM shuts down gracefully.

## Files and data location

| Path | Purpose |
| --- | --- |
| `homelab` | Server, CLI, HTML, JavaScript, CSS, and icons |
| `data/homelab.redb` | Inventory, subnet definitions, administrator username, and password hash |

Use `--data-dir` to select another location. It is a global option and works before or after a subcommand:

```bash
./homelab --data-dir /var/lib/homelab init
./homelab --data-dir /var/lib/homelab serve
```

An explicitly relative `--data-dir` is resolved against the current working directory. Prefer an absolute path for services.

The application creates its data directory with mode `0700` and database files with mode `0600` on Linux. Run all maintenance commands as the same operating-system user that runs the service.

**NixOS:** never put writable data beside a package in `/nix/store`. Pass `--data-dir /var/lib/homelab`; see the included NixOS service example.

## Network access

The default listener is `127.0.0.1:8088`, available only on the server itself.

For LAN/VPN access, bind to the server's LAN address:

```bash
./homelab serve --listen 192.168.1.10:8088
```

Or listen on every IPv4 interface:

```bash
./homelab serve --listen 0.0.0.0:8088
```

Use your actual server IP. Open `http://SERVER-IP:8088` from your browser. Restrict port 8088 to your trusted LAN/VPN in your firewall. Do not forward it directly from the internet.

HTTP does not encrypt credentials or session cookies. A VPN encrypts its tunnel, but HTTP outside that tunnel is still HTTP. If your network is not fully trusted, put the app behind an HTTPS reverse proxy and start with `--secure-cookie`. This flag marks the cookie Secure; **it does not enable TLS**. The proxy must preserve the original `Host` header for same-origin checks and forward to a loopback-bound server. Do not use `--secure-cookie` with an ordinary remote HTTP URL: the browser will not send the cookie.

## Change the password

Stop the server first, then run:

```bash
./homelab passwd
```

Or, when using a custom directory:

```bash
./homelab --data-dir /var/lib/homelab passwd
```

Enter the new password twice and restart the server. The username is unchanged. All previous browser sessions are invalid after a server restart. If you forgot your password, this command resets it using local filesystem access; the old password is not required. Protect access to the host accordingly.

Never pass passwords as shell arguments or commit them to Git. The application deliberately has no `--password` command-line option.

## Back up and restore

Maintenance commands require the server to be stopped. redb holds an exclusive process lock, so another running instance or a maintenance command cannot open the same database.

### Back up

```bash
./homelab backup --output /safe/location/homelab-2026-09-08.redb
```

The destination's parent directory must already exist. The command refuses to overwrite any existing destination. It writes a consistent standalone redb database from the inventory snapshot, rather than copying an actively changing file.

A backup includes the administrator username and password hash. It is **not encrypted**. Keep it private and preferably store a copy on another device. Do not commit `data/`, backups, or database files to GitHub.

### Restore to a new data directory

```bash
./homelab --data-dir /var/lib/homelab-restored restore \
  --from /safe/location/homelab-2026-09-08.redb
```

### Replace an existing inventory

First back up the current database, then:

```bash
./homelab restore --from /safe/location/homelab-2026-09-08.redb --force
```

`--force` is required if the destination database exists. Replacement is transactional. Restore validates the snapshot and schema version before writing. Restoring also restores the backup's administrator password. Run `passwd` afterward if you want a new password.

Only restore backups you trust. Do not use raw file copies while the service is running.

## Run as a systemd service

A service example is included in [`deploy/homelab.service`](deploy/homelab.service). It runs as a dedicated user and stores data in `/var/lib/homelab`.

```bash
sudo useradd --system --home-dir /var/lib/homelab --shell /usr/sbin/nologin homelab
sudo install -m 0755 homelab /usr/local/bin/homelab
sudo install -d -o homelab -g homelab -m 0700 /var/lib/homelab
sudo -u homelab /usr/local/bin/homelab --data-dir /var/lib/homelab init
sudo install -m 0644 deploy/homelab.service /etc/systemd/system/homelab.service
sudo systemctl daemon-reload
sudo systemctl enable --now homelab
```

Adjust the service's `--listen` address if required. Review firewall access before enabling a LAN listener.

```bash
sudo systemctl status homelab
sudo journalctl -u homelab -n 100 --no-pager
```

For password changes or backups:

```bash
sudo systemctl stop homelab
sudo -u homelab /usr/local/bin/homelab --data-dir /var/lib/homelab passwd
sudo systemctl start homelab
```

For backups, choose a destination writable by `homelab`. The included [`deploy/nixos.nix`](deploy/nixos.nix) shows equivalent NixOS service configuration using a manually installed static binary at `/opt/homelab/homelab`. It is an example module, not a published nixpkgs package.

## Update the application

1. Download/build the new binary for the same CPU architecture.
2. Stop the server.
3. Back up your database using the old binary.
4. Replace only the executable. Preserve `data/` or your configured data directory.
5. Start the new binary with the same options.

The database currently uses application schema version 1. Unknown schema versions are rejected instead of silently overwritten. Future schema changes require explicit migration work; downgrades are not promised.

## Build from source

Required **on the build machine only**:

- Rust/Cargo (manifest minimum: 1.89; CI uses the pinned toolchain in `rust-toolchain.toml`).
- Node.js 22.13 or newer and npm for the React build.
- Git.

```bash
git clone https://github.com/sound-lime/homelab-manager.git
cd homelab-manager
npm ci --prefix web
npm run build --prefix web
cargo build --locked --release
```

The native executable is `target/release/homelab`. The web build must run before Cargo because its output is embedded at compile time. Rebuilding the UI causes Cargo to re-embed it on the next build.

For development, `cargo run --release -- <command>` uses this checkout's ignored `data/` directory. A built executable run normally uses `data/` beside the executable instead. In both cases, `--data-dir` overrides the default.

### Static Linux binaries, no Docker

```bash
rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl
cargo build --locked --release --target x86_64-unknown-linux-musl
cargo build --locked --release --target aarch64-unknown-linux-musl
```

`.cargo/config.toml` selects Rust's self-contained `rust-lld` linker for both targets. Current dependencies require no external C library. Builds are tested on Linux; other build-host platforms may need additional setup.

Outputs:

- `target/x86_64-unknown-linux-musl/release/homelab`
- `target/aarch64-unknown-linux-musl/release/homelab`

The musl targets produce portable, statically linked Linux binaries. The build host can be Linux, macOS, or Windows if Rust, `rustup`, Node.js, npm, and the required target toolchain are available. The target CPU architecture must match the destination machine.

To remove this project's compiled artifacts before rebuilding:

```bash
cargo clean
```

This removes `target/` but keeps Cargo's downloaded dependency cache. To remove the dependency cache too, use `cargo cache -a` if the `cargo-cache` utility is installed, or remove the platform-specific Cargo registry and git cache manually.

### Checks

```bash
npm test --prefix web
npm run build --prefix web
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

Tests cover authentication, CSRF protection, login throttling, device/subnet CRUD, persistence, duplicate IPs, overlapping subnets, parent cycles, database locking, password hashing, backup/restore, embedded assets, and `/28`, `/31`, `/32`, `/0` allocation calculations.

## Architecture

| Component | Implementation |
| --- | --- |
| HTTP server and API | Rust, axum, tokio |
| CLI and hidden password input | clap, rpassword |
| Durable storage | redb with ACID transactions |
| Password verification | Argon2id, random salt, PHC hash string |
| Web interface | React, TypeScript, Lucide icons, CSS |
| Web build and embedding | Vite, include_dir |

The frontend is static: no Next.js/Vinext server, Cloudflare binding, Sites runtime, or OpenAI authentication remains in the standalone application.

### Database layout

The redb `state` table contains an `inventory` key with a versioned JSON snapshot: credentials, device records, and subnet records. Every mutation reads and validates the snapshot inside one write transaction before committing it. This prevents duplicate-IP races and partial updates. This intentionally simple MVP layout rewrites the snapshot on each change and is designed for a small lab, not a high-volume time-series database. Limits are 10,000 devices and 1,000 subnets; practical performance depends on inventory shape and hardware.

Database operations and password hashing run off the async request executor. Device and subnet IDs are UUIDs. Credentials are never included in inventory API responses.

### Authentication

- Argon2id: 19 MiB memory, 2 iterations, parallelism 1; random salt per password.
- Cryptographically random session tokens; only token digests are stored in server memory.
- HttpOnly, SameSite=Strict cookies; optional Secure flag for HTTPS.
- Sessions expire 8 hours after login and disappear on restart. A maximum of 32 sessions is retained; logging in beyond this limit invalidates earlier sessions.
- State-changing authenticated requests require a per-session CSRF token and same-origin validation.
- Login attempts are limited globally to 5 per rolling minute, with at most one password verification in progress. Restarting resets this limiter. A visitor who can reach the login page can consume this small global quota, so the service belongs on a trusted LAN/VPN.
- No passwords or request bodies are logged.

## API overview

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/healthz` | Process health; does not expose inventory |
| POST | `/api/login` | Authenticate and set session cookie |
| GET | `/api/session` | Read username and CSRF token |
| POST | `/api/logout` | Invalidate current session |
| GET | `/api/inventory` | Read devices and subnets |
| POST | `/api/devices` | Create/update a device; empty ID creates a UUID |
| DELETE | `/api/devices/{id}` | Delete device and detach its children |
| POST | `/api/subnets` | Create/update a subnet |
| DELETE | `/api/subnets/{id}` | Delete an unassigned subnet |

Except for health, login, and static assets, routes require authentication. Mutations also require `X-CSRF-Token`. JSON request bodies are limited to 32 KiB. The UI preserves form input after a failed save and reports validation errors.

## Troubleshooting

| Problem | What to check |
| --- | --- |
| Database not found | Run `init`, or use the same `--data-dir` as before |
| Cannot open database | Stop other server/maintenance processes; check file ownership |
| Site only opens on the server | Set a LAN/VPN `--listen` address and check the firewall |
| Cannot save / invalid request token | Reload and sign in again; verify reverse proxy Host preservation |
| Login succeeds but returns to login | Do not enable `--secure-cookie` on a remote HTTP connection |
| Too many login attempts | Wait at least a minute before retrying |
| IP rejected | Check duplicates, selected CIDR, and reserved network/broadcast addresses |
| Subnet cannot be deleted | Clear or change its assignment in all device cards first |
| Cannot write beside a NixOS package | Use a writable absolute `--data-dir` outside `/nix/store` |
| Executable format error | Choose the correct CPU architecture and a 64-bit OS |

## Development scope

This repository is the standalone Rust implementation. The earlier hosted Sites prototype is a separate application and is not modified by these builds. Import from that prototype is not implemented in the MVP.
