# Internals

Single crate: `src/lib.rs` is the library (everything but argument parsing and terminal setup),
`src/main.rs` is the clap CLI, `src/tui/` the ratatui front end. The daemon and the TUI drive the
same `ops` functions the CLI does.

```
src/
├── util/mcversion.rs      McVersion parse/compare/line (both version schemes)
├── util/slug.rs           slugify — identical to mcbackup's
├── config/                model (Config, ManualServer, ControlConfig, Policy), paths, secrets
├── server/                discovery (MCSManager + manual), start_command, detect (jar → platform/version), java, perms, mcsm (panel API)
├── platform/              Platform trait; bukkit.rs (plugin.yml / paper-plugin.yml, loaders, ignored dirs)
├── plugins/               scan.rs (walk + descriptors), hash.rs (sha1/256/512 in one pass)
├── lockfile/              lock.toml model + atomic save
├── sources/               Source trait, model (ProjectRef, ResolvedVersion, Compat), http (paced client), modrinth, hangar, github, geysermc, identify (pipeline), resolve (check)
├── transaction/           plan (requests → items + deps), apply (download/verify/swap/lock/journal), journal
├── control/               ServerControl trait; mcsm, rcon, command/none; ping (server-list ping); restart (policies)
├── backup/                mcbackup detection + checkpoint/backup
├── serverjar/             ServerJarProvider trait; paper (Fill v3), purpur (v2); ops (status, compat table, java check, install)
├── daemon/                tick loop, policy evaluation, state file
├── ops.rs                 scan_server, check_server, apply_plan (backup → apply → restart → backup)
├── jobs.rs                JobRunner: futures → messages
├── cli/                   one file per subcommand
└── tui/                   app (event loop), state, flow (update/install/revert/versions/search/journal/jar), screens/
```

## Discovery

`server::discover(config)` reads `InstanceConfig/*.json` (skipping `ignore` and `type = universal`),
locates `server.properties` in `cwd` or one level down, takes the jar from the start command
(`-jar <file>`) or the only jar in the directory, and inspects it: `version.json` → Minecraft
version and Java minimum; `META-INF/MANIFEST.MF` `Main-Class` and `META-INF/versions.list` → brand
(Paperclip with `purpur-*` / `folia-*` inside → Purpur / Folia; `com.velocitypowered` → Velocity;
`net.fabricmc` → Fabric; `org.bukkit.craftbukkit` → Spigot; `net.minecraft` → vanilla). Only the
zip directory and three tiny entries are read — discovery of seven servers takes ~0.3 s. Hashing
the server jar happens on demand for build identification.

Writability is probed by creating `plugins/.mcplug/`; read-only servers are shown and never
mutated.

## Sources

| source | identify | search | versions | download verification |
|---|---|---|---|---|
| Modrinth v2 | `POST /version_files` (sha512, bulk) | `GET /search` with loader + `project_type:plugin` facets | `GET /project/{id}/version?loaders=` ; bulk latest via `POST /version_files/update` | sha512 |
| Hangar v1 | — (confirms name hits by `fileInfo.sha256Hash`) | `GET /projects?q=` filtered by platform | `GET /projects/{slug}/versions?platform=PAPER` (`platformDependencies` = MC versions) | sha256 (hangarcdn) / none (external) |
| GitHub | — | `owner/repo` only | `GET /repos/{o}/{r}/releases`, jar assets filtered by glob | none → `unverified` |
| GeyserMC v2 | `builds` sha256 | name contains geyser/floodgate | `projects/{p}/versions/latest/builds` | sha256 |

Modrinth v3 is used only for `GET /user`, `GET /user/{id}/collections`, `GET /projects?ids=`.
Every client shares one reqwest instance with the `Antoinenz/mcplug/<version> (contact)` User-Agent
and a per-source minimum spacing between requests (Modrinth 200/min, Hangar 120/min, GitHub
30/min); 429 and 5xx are retried with backoff.

Adding a source: implement `Source` (`parse_url`, `identify_by_hashes`, `search`, `project`,
`versions`, `http`, `as_any`) and add a `SourceRef` variant for the lockfile.

## Platforms

`Platform` knows the plugin directory, how to read a descriptor, the Modrinth loader list, the Hangar
platform name, ignored subdirectories and how to judge a descriptor's `api-version`. `bukkit.rs`
covers Paper, Purpur, Folia, Pufferfish and Spigot. Multi-platform jars (ViaVersion, Plan ship
`velocity-plugin.json` and `fabric.mod.json` too) are read with the *server's* platform, so the
right descriptor wins. Velocity and Fabric are detected but not yet managed — a `Platform`
implementation each is the work.

## TUI

Elm-ish: `State` + a `Screen` enum; `render()` is a pure function of the state; key presses mutate
state or spawn a job; jobs send `Msg`s through an unbounded channel; the loop is a `tokio::select!`
over crossterm's `EventStream`, the message channel and a 250 ms tick. No `.await` on network
inside the loop, so the UI stays responsive while five sources are being queried. Popups render over
the plugin table. Progress lines for the same download replace each other.

## Daemon

`mcplug daemon` holds a lock file (one per machine), ticks every 60 s, rediscovers servers each tick,
and for each due server: scan → check → select by `auto_apply` → (defer in quiet hours) → build plan
→ `apply_plan` with the policy's restart. `scheduled` restarts are stored in the state file and
executed on a later tick. The TUI status bar reads the heartbeat from the state file — there is no
IPC.

## Tests and fault injection

`cargo test` covers version parsing/ordering/lines, start-command parsing, jar brand detection,
descriptor parsing, lockfile round-trip and URL parsing. `MCPLUG_FAULT=hash` fails every checksum
so the abort path can be exercised against a real server directory.
