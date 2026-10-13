# Provider setup

Hopper displays **Connect your coding agents** on first launch. It is also available under **Settings → Codex**. Setup runs on the machine executing the agents: the desktop for local mode, or the connected daemon host for remote mode. Claude Code and Antigravity conversations require local mode. Remote conversations currently support Codex only; selecting another provider produces an actionable error. Installing a provider on a daemon host does not enable remote conversation support for it.

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

Antigravity permission bypass is **off by default**. Existing explicitly saved preferences are preserved. Enabling **Allow Antigravity to run tools without asking** adds `--dangerously-skip-permissions` to every agy run. With it turned off, agy's headless mode auto-denies tools that need permission; Hopper then shows an error naming the blocked permission and an approval toast whose **Allow & retry** reruns that turn once with permissions allowed (never automatically, even in full-access mode).

Claude image attachments use stream-json input. Attach up to eight PNG, JPEG, GIF, or WebP images, each at most 5 MiB and totaling at most 20 MiB per message. Unsupported or unreadable attachments fail before a turn starts.

Claude's `AskUserQuestion` calls are shown as an inline question card instead of a permission toast; the answers return to the running turn as the tool's `updatedInput`. agy cannot pause for answers in headless mode, so its questions appear in the same card and the answer is sent as the next message in that conversation.

## Claude slash commands

With Claude Code selected, type `/` in the composer to open the command menu. Use the arrow keys to choose an entry, then Tab or Enter to insert it. Send the completed command to run it. Installed skills and plugin commands appear alongside Hopper's built-in controls.

| Command | Behavior |
| --- | --- |
| `/help` | Browse the commands available in Hopper. |
| `/usage` | Show this conversation's reported token counts and request a separate Claude CLI usage report. Account plan limits depend on what the installed CLI exposes; Hopper does not infer costs or remaining quota. |
| `/plugins` | List installed plugins and install, enable, disable, or uninstall a plugin by its `name@marketplace` identifier. |
| `/skills` | Search installed skills and custom commands, then insert one into the composer. |
| `/model [model]` | Choose a model for upcoming messages. An exact model name selects it directly. |
| `/permissions` | Choose read-only, approval-based, or full-access tool permissions for upcoming messages. |
| `/plan [on\|off\|prompt]` | Toggle plan mode, set it explicitly, or enable it and leave a planning prompt ready to send. Plan mode takes precedence over the selected tool permissions. |
| `/status` | Show the selected model, permissions, plan mode, and installed command count. |
| `/new`, `/clear` | Start a fresh conversation and preserve the previous conversation in history. |
| `/compact [instructions]`, `/context`, `/mcp`, `/agents` | Run the installed Claude CLI's supported headless command in the current conversation. Compacting uses Claude and may consume account usage. |

Aliases: `/plugin` opens `/plugins`; `/models` opens `/model`; `/quota` and `/cost` open `/usage`. Plain messages such as “usage of this function” remain normal conversation text. Namespaced commands such as `/reviewer:check` keep their full command name.

The plugin manager uses Claude's dedicated plugin CLI. Plugin management requires a CLI version with JSON plugin output (2.1.268 or newer). Scope can be user, project, or local; a workspace is required for project and local changes. Entries belonging to another project are read-only. Uninstalling requires confirmation and preserves plugin data. Plugin changes refresh Hopper's command menu and apply to subsequent Claude processes; they do not modify a running turn. Install plugins from marketplaces you trust; installations needing an interactive confirmation must be completed in Claude Code's terminal.

Terminal-only Claude menus are represented by Hopper controls where supported. Before forwarding a provider slash command, Hopper checks the installed CLI's headless command registry. Unsupported commands return an error instead of becoming an ordinary model prompt. Custom skills can still run tools and consume account usage. See [Claude's headless command behavior](https://code.claude.com/docs/en/agent-sdk/slash-commands) and [plugin CLI reference](https://code.claude.com/docs/en/plugins/cli-reference).

## Conversation recovery

Local Claude and Antigravity conversations are saved separately as `claude-conversations.json` and `antigravity-conversations.json` in Hopper's app data directory. Each file preserves workspace and thread identity, provider session IDs, names, and completed conversation items. An interrupted turn is shown as interrupted after restart. A streaming item that had not completed before a crash may be missing. Local image attachments still need their source files; pasted image data is stored with the conversation.

Writes use private atomic snapshots. Only one Hopper app instance can hold a provider history file open; a second instance receives a lock error instead of overwriting newer history. Corrupt or incompatible files produce an error containing their path and are never silently replaced. Each provider history file has a 128 MiB limit; reaching it reports a save error and blocks further turns until the file is backed up and reduced. No conversation is automatically pruned. Disk failures retain pending updates in memory and retry when another turn is requested after access is restored. Back up the files before manual repairs. Threads without a saved provider session cannot recover context that was never persisted; Hopper requires a new conversation instead of silently resuming with no context.

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
