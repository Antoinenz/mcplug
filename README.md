<div align="center">

# mcplug

**A terminal plugin manager for Minecraft servers. Finds what you have, tells you what's outdated, updates it safely, restarts when nobody's on, and knows how to undo.**

Point it at a server directory — or let it discover every [MCSManager](https://mcsmanager.com) instance on the host — and it identifies the installed plugins by hashing the jars against Modrinth, Hangar and GeyserMC. From there: one key to check for updates, one to review and apply them, pick any version you like, and every change is a transaction with a rollback. A daemon can do the routine ones for you, on your terms.

</div>

## Features

- **Zero setup for MCSManager hosts** — reads the panel's instance files, detects the server software and Minecraft version from the jar, talks to the panel API for restarts and console
- **Hash-based identification** — every jar is looked up by SHA-512 on Modrinth (bulk), SHA-256 on GeyserMC, and confirmed by hash on Hangar; unknown jars get ranked candidates to choose from, or a "leave it alone" flag
- **Four sources** — Modrinth (search, collections, dependencies), Hangar, GitHub releases, GeyserMC builds for Geyser/floodgate
- **Compatibility you can override** — strict by default (version must list your Minecraft version), lenient per plugin when you know better; year-based `26.x` and classic `1.21.x` versions both understood
- **Any version, not just the latest** — version picker with changelog, pin a plugin, ignore a broken release, choose your channel (release / beta / alpha)
- **Transactions** — download to staging, verify checksum and plugin descriptor, swap all-or-nothing with a rollback manifest written first, journal every step; `revert` puts it back
- **Restart coordination** — in-chat countdown, restart now, when the server is empty, or never (stage for the next restart); MCSManager, RCON or a plain command
- **[mcbackup](https://github.com/Antoinenz/mcbackup) aware** — a pinned checkpoint before, a snapshot after, so a bad update can be undone at the world level too
- **Server jar too** — Paper/Purpur builds identified by hash, same-version build updates, and Minecraft upgrades that first show you which plugins have a compatible version and whether your Java is new enough
- **Daemon** — periodic checks, auto-apply scopes (none / same-Minecraft-version releases / everything), scheduled restarts, quiet hours
- **In-game bridge** — one key installs the bundled McplugBridge plugin: titled restart countdowns, ops-only notices, live player/TPS data, and `/mcplug status|check|update|restart` for operators, scoped to that one server
- **Scriptable** — every TUI action is also a CLI command with `--json` output and meaningful exit codes

## Installation

```sh
curl -fsSL https://raw.githubusercontent.com/Antoinenz/mcplug/main/dist/install.sh | sh
```

Prebuilt binaries for Linux (x86_64, aarch64), macOS and Windows are on the [Releases](https://github.com/Antoinenz/mcplug/releases/latest) page. Or `cargo install --git https://github.com/Antoinenz/mcplug`.

## Setup

MCSManager instances are found automatically. For the panel API (restarts, console, start-command updates), store an API key once:

```sh
mcplug auth mcsm <panel API key>       # panel → user menu → API key
mcplug auth modrinth <PAT>             # optional: browse your Modrinth collections (COLLECTION_READ + USER_READ)
mcplug auth github <token>             # optional: raises GitHub's 60 req/h limit
```

If mcbackup is installed and already has the MCSManager key, mcplug picks it up from there.

Servers outside a panel go in `~/.config/mcplug/config.toml`:

```toml
[[servers]]
id = "survival"
name = "Survival"
path = "/srv/mc/survival"
start_command = "java -Xmx4G -jar paper-26.2-123.jar nogui"
ping_port = 25565
[servers.control]
kind = "rcon"                          # or "command" / "none"
port = 25575
password_ref = "survival"              # mcplug auth rcon/survival <password>
restart_command = "systemctl restart mc-survival"
```

Files under MCSManager are usually root-owned: run `sudo mcplug`. It reads your own config, not root's.

## Usage

```
mcplug                     open the TUI
mcplug servers             detected servers: platform, version, build, java, status
mcplug scan <server>       identify plugins, write plugins/.mcplug/lock.toml
mcplug check [server]      what's outdated (exit code 2 if anything is)
mcplug update <server> [plugin…] [--version ID] [--restart now|when-empty|never]
mcplug install <server> <modrinth URL | slug | owner/repo | hangar:slug>
mcplug revert <server> [tx]
mcplug history <server>
mcplug jar <server> [--update] [--mc 26.3 [--check-only]]
mcplug daemon
```

The TUI does the scanning and checking by itself; you read the screen and press Enter:

```
servers   Enter open        b install the in-game bridge        r re-check
plugins   Enter actions for a plugin (update, choose a version, pin, ignore, identify…)
          u update everything     a add a plugin (search / URL / Ctrl-L collections)
```

```
$ sudo mcplug check labubu-smp
Labubu SMP (paper 26.2): 2 updates, 14 up to date, 0 pinned, 1 unmanaged
  ↑ ClearLaggEnhanced      26.8.0            → 26.9.1            modrinth  2026-09-13
  ↑ Geyser-Spigot          2.11.2-b1235      → 2.11.2-b1240      modrinth  2026-09-13

$ sudo mcplug update labubu-smp ClearLaggEnhanced --restart when-empty
  ClearLaggEnhanced   26.8.0 → 26.9.1   modrinth   ClearLaggEnhanced-26.9.1.jar
  restart: when empty (via mcsmanager)  ·  backup: mcbackup checkpoint before + snapshot after
apply? [y/N] y
· mcbackup checkpoint labubu-smp
· downloading ClearLaggEnhanced 26.9.1
· swapping jars
· restart: waiting for 2 player(s) to leave
· restart: restarting
· restart: server is up
· mcbackup backup labubu-smp
applied transaction 20260913-084916 (1 plugin). `mcplug revert labubu-smp 20260913-084916` undoes it.
```

## The daemon

`mcplug daemon` (or the systemd unit the installer drops in) checks every server on an interval and acts according to `[policy]`:

```toml
[policy]
check_interval = "6h"
auto_apply = "same-mc-release"   # none | same-mc-release | all
restart = "when-empty"           # now | when-empty | scheduled | never
restart_at = "04:30"             # for "scheduled"
quiet_hours = ["22:00-08:00"]
geyser_auto = false              # Geyser builds almost daily; opt in
backup = true                    # mcbackup around every automatic update

[policy.per_server.creative]
auto_apply = "none"
```

It never auto-applies pre-releases, files without checksums, or versions that don't declare your Minecraft version. Everything it does lands in the server's journal, visible in the TUI (`l`).

## In-game

Press `b` on a server (or `mcplug bridge <server> --install`). mcplug writes a config with a unique
localhost port and token, drops the bundled `McplugBridge` jar into `plugins/`, and restarts when the
server is empty. From then on restart warnings show as titles and action-bar text, and operators get:

```
/mcplug status            what mcplug knows about this server, pending updates
/mcplug check             look for updates now
/mcplug update [plugin]   apply updates; the server restarts once nobody is online
/mcplug restart           restart now with a countdown
```

The command talks to the mcplug daemon on `127.0.0.1:25581` with the server's own token, so an
operator can only ever act on the server they're standing in. Permission `mcplug.admin`, default op.

## Documentation

| url | description |
|---|---|
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | every config and policy key, secrets, manual servers, the lockfile |
| [docs/IDENTIFICATION.md](docs/IDENTIFICATION.md) | how jars are matched to projects, compatibility modes, channels, pins |
| [docs/TRANSACTIONS.md](docs/TRANSACTIONS.md) | the update transaction step by step, rollback, journal, restart policies, mcbackup |
| [docs/INTERNALS.md](docs/INTERNALS.md) | module map, source APIs used, TUI architecture, adding a platform or source |

## License

[MIT](LICENSE)
