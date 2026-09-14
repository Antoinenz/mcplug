# Transactions, restarts, backups

Every change to a server's plugins is a transaction with an id like `20260913-084916`.

## Steps

| # | step | on failure |
|---|---|---|
| 0 | preflight: plugin dir writable, plan not empty, staging dir created | abort, nothing touched |
| 1 | mcbackup `checkpoint <slug> -m "before: …"` (if enabled and installed) | abort, nothing touched |
| 2 | download every file to `plugins/.mcplug/staging/<tx>/`, hashing while streaming | abort, staging deleted, journaled |
| 3 | verify: checksum from the source (sha512/sha256/sha1, whichever it publishes); open the jar; read its descriptor; the name must match the plugin being replaced | abort, staging deleted, journaled |
| 4 | write `rollback/<tx>/manifest.toml` — old file, new file, old lock entry, new lock entry, per item | — |
| 5 | swap, per item: old jar → `rollback/<tx>/`, new jar → `plugins/`, ownership matched to the old jar | reverse the completed swaps, journal `aborted` |
| 6 | mark the manifest complete, update `lock.toml`, journal `applied` | — |
| 7 | restart according to the policy | journal `restart: failed`; jars stay in place |
| 8 | mcbackup `backup <slug> -m "after: …"` | journal only |

Nothing on the live server changes before step 5, and step 5 only starts once every file has
passed step 3. Required dependencies (Modrinth/Hangar metadata) are added to the plan and swapped
first. Files from sources without checksums (GitHub) are refused unless `--allow-unverified` / the
TUI confirmation, and the daemon never applies them.

`MCPLUG_FAULT=hash` makes every checksum fail, for testing the abort path.

## Revert

`mcplug revert <server> [tx]` (default: the last applied transaction), or `r` in the journal screen:
new jars are removed, old jars restored from `rollback/<tx>/`, lock entries restored from the
manifest, journaled as `revert`. The last five rollback directories are kept. A manifest without
`completed = true` means a crash mid-swap; the transaction is safe to revert.

If the plugin-level revert isn't enough — the update corrupted plugin data, say — the journal shows
the mcbackup checkpoint id taken before the update: `mcbackup restore <slug> <id>`.

## Restart policies

| policy | behaviour |
|---|---|
| `now` | `say [mcplug] server restarts in 60s — <summary>` then at 30/10/5/3/2/1 s; restart; wait until the panel reports running **and** the server answers a list-ping (fully loaded) |
| `when-empty` | poll the player count every 30 s; at 0, a ≤15 s countdown and restart; after the max wait, restart anyway |
| `never` | leave the jars in place for the next restart |

Announcements need console access (MCSManager or RCON). With `kind = "command"` the restart still
happens, silently. Player counts come from a Minecraft server-list ping on `ping_port`, gated on the
panel saying the instance is running (several instances can share a port when only one runs at a
time).

## mcbackup

If [mcbackup](https://github.com/Antoinenz/mcbackup) is installed (`/usr/local/bin/mcbackup` or on
`PATH`), each transaction is bracketed by a pinned checkpoint before and a normal snapshot after a
successful restart, using the server's `backup_slug` (MCSManager nickname slugified — the same name
mcbackup derives). If mcbackup doesn't know the server, the checkpoint fails and the transaction is
aborted before anything changes; either add the server to mcbackup or turn the backup off for that
run (`--no-backup`, or `b` in the restart dialog).

## Server jar

`mcplug jar <server> --update` downloads the newest build of the current Minecraft version, verifies
it, keeps the old jar, and rewrites the start command through the panel (or tells you what to set
for manual servers). `--mc <version>` first prints every managed plugin's best available version
for the target and whether the start command's Java meets the build's minimum (`26.2` needs Java 25),
and only proceeds without `--check-only`. Both go through the same restart/backup flow.

## The journal

`plugins/.mcplug/journal.jsonl`, one JSON object per line: time, transaction id, action
(`update` `install` `revert` `aborted` `restart` `backup` `server-jar`), outcome, items, note.
`mcplug history <server>` prints it; the TUI's `l` shows it with revert on `r`.

## The in-game bridge

`McplugBridge` (in `companion/`, bundled into release binaries) runs a plain-JDK HTTP listener on
`127.0.0.1:<port>` (`plugins/McplugBridge/config.yml`, written by mcplug: `port`, `token`,
`daemon-url`). mcplug calls `GET /status` (players, TPS, MSPT, version), `POST /countdown`
(chat + title + action bar), `POST /broadcast`, `POST /notify` (ops + console), all with
`Authorization: Bearer <token>`. When the bridge is present the `Companion` control wraps the base
control (MCSManager/RCON/command), so restarts still go through the panel while messages and
readiness go through the plugin.

`/mcplug <status|check|update [plugin|all]|restart>` (permission `mcplug.admin`, default op) POSTs
to the daemon's `/v1/command` on `127.0.0.1:25581` with the same token. The daemon looks the token
up across the servers it knows and acts on that server only; unknown tokens get 401. `update`
plans and applies with restart-when-empty in the background and reports back through `/notify`;
`restart` runs the usual countdown. The daemon skips its own tick for a server while an in-game
command is working on it.
