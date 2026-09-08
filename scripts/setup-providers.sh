#!/usr/bin/env bash
# Terminal fallback: install missing CLIs with their official native installers.
# Authentication remains in each provider's CLI; no credentials/config are copied.
set -euo pipefail
usage() { echo 'Usage: bash scripts/setup-providers.sh [claude|antigravity|both] [--yes]'; }
provider="${1:-both}"
case "$provider" in claude|antigravity|both) ;; -h|--help) usage; exit 0 ;; *) usage; exit 2 ;; esac
case "${2:-}" in ''|--yes) ;; *) usage; exit 2 ;; esac
if [[ "${2:-}" != --yes ]]; then
  read -r -p "Install missing $provider CLIs using official installers? [y/N] " hopper_answer
  [[ "$hopper_answer" == y || "$hopper_answer" == Y ]] || exit 0
fi
export PATH="$HOME/.local/bin:$PATH"
command -v curl >/dev/null || { echo 'curl is required. Install it using your OS package manager.' >&2; exit 1; }
hopper_installer=$(mktemp)
trap 'rm -f "$hopper_installer"' EXIT
install_provider() {
  local hopper_bin="$1" hopper_url="$2"
  shift 2
  if command -v "$hopper_bin" >/dev/null 2>&1; then
    echo "$hopper_bin is already installed."
    return
  fi
  curl --proto '=https' --proto-redir '=https' -fSL "$hopper_url" -o "$hopper_installer"
  bash "$hopper_installer" "$@"
  command -v "$hopper_bin" >/dev/null || { echo "$hopper_bin was not found after installation. Restart your terminal and check the provider's instructions." >&2; exit 1; }
}
if [[ "$provider" == claude || "$provider" == both ]]; then
  install_provider claude https://claude.ai/install.sh
  echo 'Sign in using: claude auth login'
fi
if [[ "$provider" == antigravity || "$provider" == both ]]; then
  install_provider agy https://antigravity.google/cli/install.sh --skip-aliases
  echo 'Sign in using: agy'
fi
echo 'Open Hopper → Settings → Codex → Connect your coding agents, then test each connection.'
