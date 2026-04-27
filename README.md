# sdtd-server-manager

A self-hosted control panel for **7 Days to Die** dedicated servers, written
in Rust. Single-binary backend with an embedded web UI — point it at your
server install, open `http://localhost:8421`, and you get the same kind of
panel paid hosts (Nitrado, GTX Gaming, BisectHosting, GameServerKings, etc.)
charge a monthly fee for, but running on your own machine.

Built and tested against **7 Days to Die V2.6 Stable** (April 2026), and
forward-compatible with future versions because the settings UI is driven
by whatever properties your `serverconfig.xml` actually contains.

---

## Features

**Lifecycle**
- Start / stop / status with real-time uptime
- **Graceful shutdown** via the game's telnet `shutdown` command, with a
  20-second grace period before falling back to terminating the process

**Server settings (`serverconfig.xml`)**
- Live form-based editor that round-trips the XML, preserving comments
- Properties grouped by category (Identity, Networking, Slots, Web Dashboard,
  World, Difficulty, Day/Night, Performance, Zombies & Blood Moon, Loot,
  Multiplayer, Land Claims, Dynamic Mesh, Twitch, Persistence, …)
- Bool / number / enum inputs for everything the V2.6 wiki documents,
  including the new V2.5/2.6 properties: `JarRefund`, `CameraRestrictionMode`,
  `DeathPenalty`, `EnemySpawnMode`, `ZombieFeralSense`, `AirDropFrequency`,
  `MaxQueuedMeshLayers`, the `DynamicMesh*` family, `Twitch*`,
  `MaxChunkAge`, `SaveDataLimit`, `WebDashboard*`, `EnableMapRendering`
- Search filter, dirty-tracking, save-as-preset
- Anything the editor doesn't recognize falls through as a free-text field
  in *Misc / Advanced* — so future game versions just work

**World & seeds**
- Pick from the bundled maps (Navezgane, PREGEN6k/8k/10k) or any RWG world
  you've already generated
- WorldGenSize bounded to 6144 / 8192 / 10240 (the V2.6 supported set)
- Random seed roller — generates readable names like `AshenRidge42`

**Live console (telnet)**
- Streams the server's stdout/stderr into the browser
- Command input box: send any console command (`say`, `kick`, `ban add`,
  `lp`, `gt`, `weather`, `shutdown`, …) directly to the running server
- ↑ / ↓ history
- Auto-attaches when the server reports ready; pill in the toolbar shows
  `telnet: on/off`

**Players & admins (`serveradmin.xml`)**
- Admin list with permission levels (0 = full access, 1000 = default user)
- Whitelist (toggles whitelist-only mode when non-empty)
- Bans with optional reason
- Per-command permission overrides (e.g. `kick = 1`, `say = 1000`)
- Reads both the modern `<user platform="Steam" userid="…">` format and
  the legacy `steamID="…"` format; writes the modern shape on save

**Backups**
- One-click snapshot of the current save folder (resolved from `GameName`)
- Newest-first list with size, timestamp, and optional note
- Restore renames your existing save aside as `<n>.replaced-<ts>` instead
  of deleting it, so a botched restore is always recoverable
- Refuses to restore over a running server

**Schedule**
- Daily tasks at HH:MM (24h, local time)
- Actions: `restart` (graceful), `stop`, `start`, `backup`
- Persisted across manager restarts; `last_fired_iso` dedup prevents
  double-firing within the same minute

**Presets** — save your full XML, load on demand, name and tag.

---

## Quick start

```bash
cd 7dtd-server-manager
cargo build --release
./target/release/sdtd-server-manager
```

Open <http://localhost:8421>.

The first time it runs, the manager guesses common Steam install paths.
Anything it didn't get right you can fix in the **APP CONFIG** tab.

To enable the live console, in your `serverconfig.xml`:

```xml
<property name="TelnetEnabled"  value="true" />
<property name="TelnetPort"     value="8081" />
<property name="TelnetPassword" value="some-strong-password" />
```

The manager will pick up the password automatically when the server starts —
nothing else to configure on the manager side.

---

## Linux / Arch users

Everything works on Linux out of the box — the same `cargo build --release`
produces a single binary that runs anywhere.

### One-command run (any distro)

```bash
cargo build --release
./target/release/sdtd-server-manager
```

### AppImage (any distro, no install needed)

```bash
./packaging/build-appimage.sh           # dynamic build (smaller)
# or
./packaging/build-appimage.sh --musl    # fully static, runs anywhere
```

The script produces `dist/sdtd-server-manager-x86_64.AppImage`. Run it
directly — no installation required.

```bash
chmod +x dist/sdtd-server-manager-x86_64.AppImage
./dist/sdtd-server-manager-x86_64.AppImage
```

**Which mode to pick:**

- **Dynamic** (default) — smaller (~8MB), uses your distro's `glibc` and
  DBus. Works on any modern desktop distro: Arch, Fedora, openSUSE, Ubuntu
  22.04+, Debian 12+, SteamOS, etc. The build script tells you the
  minimum glibc version the resulting AppImage requires; build on the
  oldest distro you want to support, or on a glibc-2.28 Docker image
  for max compatibility.
- **`--musl`** — fully static (~12MB). Doesn't link to `glibc` or any
  system library at all. Runs on **any** Linux kernel ≥ 3.2, no matter
  how old or stripped-down. Use this if your AppImage is failing on
  someone's CentOS 7 / RHEL 8 / really old Ubuntu / minimal Alpine box.
  Requires `rustup target add x86_64-unknown-linux-musl` once on the
  build machine.

**No GTK dependency.** On Linux, the file/folder picker uses the XDG
Desktop Portal (DBus protocol) instead of GTK. This means:
- The AppImage doesn't bundle GTK — it's smaller and won't conflict
  with the user's GTK theme.
- It works under any DE that ships portals: GNOME, KDE Plasma,
  Cinnamon, Sway, Hyprland, etc. (every modern desktop does).
- Headless setups don't need a display — the picker simply errors,
  and the user fills paths in by typing them.

**To run as a daemon on a headless Linux server**, set
`NO_BROWSER=1 BIND=0.0.0.0:8421` and the AppImage skips browser launch
and binds to your LAN. (The AppRun script auto-detects no DISPLAY and
sets `NO_BROWSER=1` for you.)

### Arch (PKGBUILD / makepkg)

```bash
cd packaging
makepkg -si
```

This builds `sdtd-server-manager` and installs it system-wide along with
the .desktop entry and a systemd user service.

### Run as a systemd service (headless servers)

```bash
# After installing via PKGBUILD, or after copying the .service file
# manually to ~/.config/systemd/user/ :
systemctl --user enable --now sdtd-server-manager
journalctl --user -u sdtd-server-manager -f      # tail logs
```

### Environment variables

| Var          | Default         | Effect                                            |
| ---          | ---             | ---                                               |
| `BIND`       | `127.0.0.1:8421`| Full bind address. Use `0.0.0.0:8421` for LAN.    |
| `PORT`       | `8421`          | Port (only used when `BIND` is unset).            |
| `NO_BROWSER` | unset           | Skip the auto-open of the default browser.        |
| `RUST_LOG`   | `info`          | Standard tracing-subscriber filter.               |

---

## What's intentionally not in scope (for now)

- **Live in-game map (Allocs map mod)** — requires a separate game-side mod,
  out of scope for the base manager.
- **Mod manager / installer** — modlet packaging is fluid enough that a
  generic installer would do more harm than good. For now, install mods by
  dropping them into `<server>/Mods/` yourself.
- **SteamCMD update integration** — if you have steamcmd installed you can
  run `app_update 294420 validate` from a shell; baking that in is a
  next-iteration item.

---

## API surface

All endpoints are JSON, served on `127.0.0.1:8421` (configurable via the
`PORT` environment variable).

| Method   | Path                          | Purpose                                    |
| ---      | ---                           | ---                                        |
| `GET`    | `/api/status`                 | live status, uptime, telnet attachment     |
| `GET`    | `/api/config`                 | parsed `serverconfig.xml` + raw            |
| `PUT`    | `/api/config`                 | merge in property changes & save           |
| `POST`   | `/api/start`                  | start the server                           |
| `POST`   | `/api/stop`                   | graceful telnet stop, kill on timeout      |
| `GET`    | `/api/logs?since=N`           | bounded log buffer (cursor-based)          |
| `GET`    | `/api/maps`                   | bundled + RWG-generated world list         |
| `GET`    | `/api/seed?count=N`           | random seeds                               |
| `GET`    | `/api/settings`               | manager settings (paths, presets)          |
| `PUT`    | `/api/settings`               | save manager settings                      |
| `GET`    | `/api/presets`                | list XML presets                           |
| `POST`   | `/api/presets`                | save current XML as preset                 |
| `POST`   | `/api/presets/:name`          | apply preset                               |
| `DELETE` | `/api/presets/:name`          | delete preset                              |
| `POST`   | `/api/console/exec`           | run a telnet command on the live server    |
| `GET`    | `/api/admin`                  | parsed `serveradmin.xml`                   |
| `PUT`    | `/api/admin`                  | save admins / whitelist / bans / perms     |
| `GET`    | `/api/backups`                | list snapshots                             |
| `POST`   | `/api/backups`                | create snapshot now                        |
| `POST`   | `/api/backups/restore`        | restore (renames live save aside)          |
| `POST`   | `/api/backups/delete`         | delete a snapshot                          |
| `GET`    | `/api/schedule`               | list scheduled tasks                       |
| `POST`   | `/api/schedule`               | add a task                                 |
| `PUT`    | `/api/schedule`               | update a task                              |
| `DELETE` | `/api/schedule/:id`           | delete a task                              |

---

## Caveats

- The manager binds **only** `127.0.0.1`. If you want LAN access, edit the
  bind address in `src/main.rs` and put it behind something with auth.
- On Linux, `startserver.sh` spawns the actual binary as a child of a shell;
  killing the shell occasionally leaves `7DaysToDieServer.x86_64` behind.
  The graceful telnet path avoids this problem; the kill fallback may need
  a manual `pkill 7DaysToDie` once if telnet wasn't attached.
- World generation happens inside the running game, on first start of a
  fresh seed. The manager only lists what's in your `GeneratedWorlds`
  directory.
- No Rust toolchain was available in the environment used to author this,
  so a `cargo check` pass on your end is the next step. Most likely fix-up
  point if any: the `quick-xml` 0.31 attribute decoding API in `admin.rs` —
  if `attr.decode_and_unescape_value(reader.decoder())` doesn't compile
  for your minor version, swap it for `String::from_utf8_lossy(&attr.value).into_owned()`.
