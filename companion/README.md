# McplugBridge

A Paper plugin that lets [mcplug](https://github.com/Antoinenz/mcplug) talk to a running server.

- mcplug → server (`127.0.0.1:<port>`, bearer token): `GET /status` (players, TPS, MSPT, version),
  `POST /broadcast`, `POST /countdown` (chat + title + action bar), `POST /notify` (ops + console).
- server → mcplug daemon (`/v1/command`, same token): the in-game `/mcplug status|check|update|restart`
  command, **operators only** (`mcplug.admin`, default op). The daemon maps the token to this one
  server, so an operator can only act on the server they're on.

Install it from mcplug (server list → `b`), which writes `plugins/McplugBridge/config.yml` with a
unique port and token and restarts the server. Build by hand with `mvn -q package` (JDK 25, as Paper 26.x requires).
