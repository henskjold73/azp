# azp

Manage multiple Azure CLI logins side by side. Each profile is its own
`AZURE_CONFIG_DIR` under `~/.azure-profiles/<name>/` (mode `0700`).

Built for macOS and Linux (bash/zsh). Windows is not a target.

## Install

```sh
brew install henskjold73/tap/azp
```

Or with the install script, which downloads a prebuilt binary into `~/.cargo/bin`:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/henskjold73/azp/releases/latest/download/azp-installer.sh | sh
```

Or build from source with `cargo install --git https://github.com/henskjold73/azp`.

## Usage

```sh
azp add work                 # create profile and run `az login`
azp add client -- --tenant client.onmicrosoft.com   # extra args go to `az login`
azp list                     # profile, user, tenant, subscription, status (* = active)
azp use work                 # prints: export AZURE_CONFIG_DIR=...
azp shell work               # start $SHELL with the profile active
azp relogin work             # sign in again when tokens expire
azp remove work              # delete the profile (asks first)
```

Profile names may only contain `[a-z0-9_-]`. `azp list` reads user, tenant and
subscription from `azureProfile.json`, then runs `az account get-access-token`
in each profile (in parallel) to fill the STATUS column: `signed in`, `expired`
(run `azp relogin <name>`), or `signed out`. azp itself never reads token caches.

## Shell integration

`azp use` only prints an `export` line, so wrap it in a function to switch the
current shell. Add this to `~/.zshrc`:

```zsh
azu() { eval "$(azp use "$1")"; }
```

Then `azu work` switches this shell to the `work` profile.
