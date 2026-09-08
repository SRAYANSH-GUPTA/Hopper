# Provider setup

Hopper displays **Connect your coding agents** on first launch. It is also available under **Settings → Codex**. Setup runs on the machine executing the agents: the desktop for local mode, or the connected daemon host for remote mode.

## New installations

1. Select Claude Code, Antigravity, or both.
2. Click **Install** for missing providers. Hopper downloads and runs the provider's official native installer. Existing installations are reused.
3. Click **Sign in**. Complete authentication in the provider's terminal/browser window, then return to Hopper. Each user supplies their own account and provider access.
4. Click **Test connection** for each selected provider. This sends a short prompt in an empty temporary directory and may consume provider credits. Installation or detected sign-in alone does not mark a connection verified.
5. Click **Start using Hopper** after the selected providers pass. The first selected provider becomes the active local provider.

**Set up later** dismisses first-launch setup. Open Settings to resume. **Save setup preferences** saves provider choices and the permission preference; it does not run a connection check. A saved setup flag records onboarding dismissal/completion, not a permanent authentication guarantee.

## Installation and authentication

Official sources:

- [Claude Code installation and sign-in](https://code.claude.com/docs/en/quickstart)
- [Antigravity installation and sign-in](https://antigravity.google/docs/cli/install/)

Windows uses PowerShell installers. macOS and Linux use Bash installers. Linux installation requires Bash; terminal sign-in supports x-terminal-emulator, GNOME Terminal, Konsole, Kitty, Alacritty, and xterm. If no supported terminal is found, the error includes the command to run manually. On a remote host, sign in directly on that host; the client never launches a local login for a remote provider.

Setup discovers native executable locations and the current PATH. Unix login-shell lookup handles shell-managed installs. CLI installation is checked again afterward; a successful installer exit alone is insufficient.

Packaged Hopper users do not need Node, Rust, CMake, or LLVM to build Hopper. Those are development/build dependencies. The provider installers and system integration may have their own platform requirements.

## Configuration

Hopper stores setup preferences in the operating system's configuration directory, under `hopper/provider-setup.json`. The previous file is backed up to `provider-setup.json.bak` when saving. Unknown keys are preserved; malformed JSON produces an error instead of being overwritten.

Hopper does not copy account tokens, personal settings, shell profiles, or project paths from the developer's machine. Official installers may update PATH; Antigravity installation preserves existing shell aliases.

Claude approvals use a session-only HTTP PreToolUse hook supplied through `--settings`, without Python, curl, or writes to `~/.claude/settings.json`. The hook calls a loopback endpoint authenticated with a per-process token. Legacy Hopper shell hooks remain inactive because their environment variables are removed from the child process. Existing user configuration files remain intact.

Antigravity permission bypass is **off by default**. The explicit **Allow Antigravity to run tools without asking** setting adds `--dangerously-skip-permissions` only after it is saved. With bypass off, use provider-defined permissions; tool requests requiring interactive approval may stop in headless mode. Hopper does not yet bridge Antigravity's interactive permission prompts.

## Terminal fallback

macOS/Linux:

```bash
bash scripts/setup-providers.sh both
```

Windows PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/setup-providers.ps1 -Provider both
```

Use `claude` or `antigravity` to install only one. `--yes` (Bash) or `-Yes` (PowerShell) explicitly skips the installation confirmation. Scripts install missing CLIs and print login instructions. They do not sign in, copy credentials, enable permission bypass, or mark Hopper setup complete.

## Backend contracts

- `provider_setup_status`: installation metadata, detected Claude authentication, host platform, and saved setup preferences.
- `provider_setup_action`: accepts `provider: "claude" | "antigravity"` and `action: "install" | "login" | "verify"`.
- `provider_setup_save`: accepts the `preferences` object.

All three commands route through `shared/provider_setup_core.rs` in the app and daemon. Installs, login launches, verification, and preference saves are serialized per process. Mutating setup calls are not retried automatically after a remote disconnect.

Setup parity does not imply remote provider execution parity: the daemon's existing chat provider support still determines which engines can run remotely.
