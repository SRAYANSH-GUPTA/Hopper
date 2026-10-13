# Security and local data

## Application and browser boundaries

Hopper's trusted application webviews are `main` and `about`. Native application
commands require a trusted webview and application origin. Embedded browser tabs
receive only the conversation reply and file import commands, on the supported
assistant HTTPS origins. Arbitrary subdomains and nonstandard ports are excluded.
Browser navigation cannot open local files or internal application protocols.

Plugin permissions are scoped to individual application webviews. Local image
previews use Hopper's guarded asset handler: requests from browser tabs are
rejected before accessing disk, responses must be images, and reads are limited
to 64 MiB. The filesystem asset scope supports images from user-selected
workspaces and attachments; it does not grant browser tabs filesystem access.

The packaged renderer uses a content security policy that permits packaged
scripts, WebAssembly for syntax highlighting, inline styles, image previews, and
HTTPS connections. Remote scripts, inline scripts, frames, plugins, and form
submission are blocked. Development adds the inline scripts and WebSocket
connections required by Vite. Embedded websites run in separate native webviews
and use their own website policies.

Tauri must include the fix for
[GHSA-w28w-mhc8-qvjv](https://github.com/tauri-apps/tauri/security/advisories/GHSA-w28w-mhc8-qvjv).
Its channel isolation prevents an embedded website from retrieving another
webview's queued IPC response. Application command guards alone do not fix that
upstream vulnerability.

## Persistence and telemetry

Settings and workspace snapshots use synced temporary files and atomic
replacement. On Unix, newly written snapshots have owner-only permissions.
Unreadable or corrupt snapshots produce a recovery error instead of being
replaced with empty defaults. Preserve a copy before repairing a damaged JSON
file. Separate application and daemon processes do not have an interprocess
transaction coordinator; avoid independently editing the same data directory.

Telemetry is disabled unless the build supplies a nonempty `VITE_SENTRY_DSN`.
Distributors that enable it must use a destination they control and document
their data handling. Hopper does not enable Sentry's default PII collection.

## Remote daemon

TCP authentication controls access; it does not encrypt traffic. Use a trusted
encrypted tunnel such as Tailscale when connecting between machines. The daemon
limits connections, concurrent requests, authentication time, frame size, and
queued responses. Slow or malformed clients are disconnected. Remote backend
support and provider setup are described in [Provider setup](provider-setup.md).

## Release verification

The release workflow requires the automated CI gates, a project-controlled
updater key matching the bundled public key, valid artifact signatures, and all
expected platform artifacts. macOS distribution additionally requires Developer
ID signing and notarization. See [Release Build](../README.md#release-build).

Dependency auditing covers the complete npm and Cargo lockfiles. RustSec warnings
without a compatible upstream replacement remain visible in CI and are documented
in the README. An audit result does not replace native platform acceptance tests.
