# Configuration

mcplug keeps three kinds of state:

| what | where | notes |
|---|---|---|
| config | `~/.config/mcplug/config.toml` | servers outside a panel, sources, policy |
| secrets | `~/.config/mcplug/secrets.toml` | tokens; mode 0600, refused if wider; written by `mcplug auth` |
| per-server state | `<server>/plugins/.mcplug/` | `lock.toml`, `journal.jsonl`, `rollback/`, `staging/` — travels with the server |
| daemon state | `~/.local/state/mcplug/daemon.json` (root: `/var/lib/mcplug/`) | heartbeat, last check per server, pending scheduled restarts |

Config directory resolution: `--config <dir>` → `$MCPLUG_CONFIG_DIR` → `/etc/mcplug` when root and it exists →
the invoking user's home when run via `sudo` → the platform config dir. So `sudo mcplug` uses *your* config,
and a daemon installed by the installer reads `/etc/mcplug`.

## config.toml

```toml
version = 1
contact = "you@example.com"        # goes into the User-Agent, as Modrinth asks

[mcsmanager]
enabled = true
url = "http://127.0.0.1:23333"
instance_config_dir = "/opt/mcsmanager/daemon/data/InstanceConfig"
ignore = ["__MCSM_GLOBAL_INSTANCE__"]      # nicknames or instance uuids to skip
api_key_env_file = "/opt/mcbackup/env"     # fallback: read MCSM_APIKEY from here if secrets.toml has none

[[servers]]                                # manual servers (any number)
id = "survival"
name = "Survival"
path = "/srv/mc/survival"
start_command = "java -Xmx4G -jar paper-26.2-123.jar nogui"   # optional; used to find the jar and java
ping_port = 25565                          # for player counts (server-list ping)
backup_slug = "survival"                   # mcbackup source name; default slugify(name)

[servers.control]
kind = "rcon"                              # rcon | command | none
host = "127.0.0.1"
port = 25575
password_ref = "survival"                  # mcplug auth rcon/survival <password>
restart_command = "systemctl restart mc-survival"   # needed to start again after `stop`
# kind = "command": only restart_command; no console, so no countdown announcements
# kind = "none":    updates are staged, you restart

[sources]
modrinth = true
hangar = true
github = true
geysermc = true

[backup]
mcbackup = "auto"                          # auto | off | /path/to/mcbackup

[policy]                                   # daemon behaviour, see below
```

### Servers

MCSManager instances are discovered from the daemon's `InstanceConfig/*.json`: nickname, working
directory, start command, ping port. If the server files sit one level below the instance directory
(a zip import that kept its top folder), mcplug finds `server.properties` there and notes it.

The server **id** is the nickname slugified the same way mcbackup does (`Labubu SMP` → `labubu-smp`),
so the two tools always agree on a server's name. Commands accept the id, an unambiguous prefix, or
the MCSManager uuid.

### Policy

```toml
[policy]
check_interval = "6h"            # per server
auto_apply = "same-mc-release"   # none | same-mc-release | all
restart = "when-empty"           # now | when-empty | scheduled | never
restart_at = "04:30"             # local time, for "scheduled"
countdown = 60                   # seconds of in-chat warning
quiet_hours = ["22:00-08:00"]    # no applying or restarting inside these local-time windows
geyser_auto = false              # Geyser/floodgate builds almost daily; opt in
backup = true                    # mcbackup checkpoint before / snapshot after automatic updates
server_jar_builds = false        # reserved: keep the server jar on the newest build

[policy.per_server."labubu-smp"] # any key above, per server id
restart = "scheduled"
[policy.per_server.creative]
enabled = false
```

`auto_apply` scopes:

- `none` — check and record only; the TUI shows what's available.
- `same-mc-release` — releases whose source lists the server's exact Minecraft version, verified by checksum. Never betas, never GitHub files without checksums, never Geyser unless `geyser_auto`.
- `all` — additionally accept versions that only match the server's version *line* (`26.x`, `1.21.x`) and each plugin's own channel setting.

`restart`:

- `now` — countdown in chat, restart, wait until the server answers pings again.
- `when-empty` — poll the player count every 30 s; restart after a short countdown once it hits 0 (falls back to a restart after 4 h).
- `scheduled` — apply the jars, restart at `restart_at`.
- `never` — apply the jars; whoever restarts next loads them.

## secrets.toml

Written by `mcplug auth <what> <value>`:

| key | used for |
|---|---|
| `modrinth_token` | Modrinth PAT with `COLLECTION_READ` + `USER_READ`: browse your collections in the install screen |
| `github_token` | raises GitHub's unauthenticated 60 requests/hour |
| `mcsm_api_key` | MCSManager panel API key (or `api_key_env_file` fallback) |
| `[rcon] <server-id> = "…"` | RCON password for a manual server |

## The lockfile

`plugins/.mcplug/lock.toml` records, per plugin: descriptor name (the identity), current file and
hashes, the source reference (`modrinth` project/version ids, `hangar` slug/version, `github`
owner/repo/tag/asset glob, `geysermc` project/build), channel, `pinned`, `ignored_versions`,
`compat` (`inherit` | `strict` | `lenient` | `any`), and `installed_at`. Two special sources:
`unidentified` (still looking for candidates) and `unmanaged` (leave it alone).

Editing it by hand is fine — `compat = "lenient"` on a plugin whose author is slow to update the
version list is the most common reason to.
