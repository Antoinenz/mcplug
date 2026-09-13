# Identification and compatibility

## Which project is this jar?

`mcplug scan` walks `plugins/*.jar` (skipping `.paper-remapped`, `update`, `disabled`, `.mcplug`),
reads each jar's `plugin.yml` / `paper-plugin.yml` for its **name**, and computes SHA-1, SHA-256 and
SHA-512 in one pass. Then, for jars the lockfile doesn't already know by hash:

1. **Modrinth by hash** — one `POST /v2/version_files` with every SHA-512. Anything published on
   Modrinth comes back with its exact version. This covers most plugins.
2. **GeyserMC by hash** — Geyser and floodgate builds are matched by SHA-256 against the recent
   build list.
3. **Name search** on every enabled source. Hangar candidates are verified by comparing the
   installed jar's SHA-256 with each Hangar version's file hash; a match is `hash-confirmed`.
   GitHub search only understands `owner/repo`.

Results:

- hash-confirmed → recorded, done.
- exact name match on a single source, when `--accept-exact` was given → recorded with an
  "unknown" version; the first check resolves it.
- otherwise → `unidentified`, with ranked candidates. In the TUI press `i` to pick one (the pick is
  recorded without a version until the next check), or `m` to mark it `unmanaged`.

Identity is the descriptor name, not the filename: `Geyser-Spigot.jar` and
`Geyser-Spigot-2.11.2.jar` are the same plugin, and a renamed jar re-syncs by hash. Two jars with
the same name are reported as duplicates.

## Which version should it be on?

`check` asks each source for the plugin's versions (Modrinth in bulk via `/v2/version_files/update`),
then applies, in order:

1. **channel** — the plugin's minimum accepted channel (`release` by default; `beta` also accepts
   betas; `alpha` accepts everything). Hangar channels map by name; GitHub pre-releases are betas.
2. **ignored versions** — anything you pressed `x` on.
3. **compatibility** against the server's Minecraft version:

| result | meaning | auto-applied? |
|---|---|---|
| `exact` | the version lists the server's exact MC version | yes |
| `lenient` | same line (`26.x` for year versions, `1.21.x` for classic) and the plugin's `compat` allows it | yes |
| `same-line-only` | same line, but the plugin is `strict` | shown with `⇡`, manual only |
| `unknown` | the source has no game-version data (GitHub, GeyserMC) | shown, manual only |
| `incompatible` | different line | not offered (the version picker still lists it) |

Loader filtering happens at the source: Paper servers accept `paper`, `purpur`, `spigot`,
`bukkit` files, in that order of preference. Sources often publish one version number as several
files (one per loader); those siblings are never reported as updates.

Versions are ordered by their **publish date**, never by parsing version strings — `2.2.5-SNAPSHOT
(b140-8780fa4)`, `26.9.1`, `v5.5.71-bukkit` and `2.11.2-b1235` all coexist happily.

## Minecraft versions

Both schemes parse and compare numerically: `26.2 > 1.21.11`, `26.2.1 > 26.2`, `26.3-pre1 < 26.3`.
The "line" used for lenient matching is `major.minor` for `1.x` and the year for `26.x`, because a
`26.1 → 26.2` drop corresponds to what a `1.21.4 → 1.21.5` drop used to be.

## Pins and ignores

- `p` **pin**: never offered updates; the version picker still works for a deliberate change.
- `x` **ignore**: skips that one version id; the next release is offered again.
- per-plugin `compat`: `inherit` (server default, strict) · `strict` · `lenient` · `any`.
