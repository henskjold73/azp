# azp

Manage multiple Azure CLI logins side by side. Each profile is its own
`AZURE_CONFIG_DIR` under `~/.azure-profiles/<name>/` (mode `0700`).

Built for macOS and Linux (bash/zsh). Windows is not a target.

```sh
cargo install --path .

azp add work                 # create profile and run `az login`
azp add client -- --tenant client.onmicrosoft.com   # extra args go to `az login`
azp list                     # profile, user, tenant, default subscription (* = active)
azp use work                 # prints: export AZURE_CONFIG_DIR=...
azp shell work               # start $SHELL with the profile active
azp relogin work             # sign in again when tokens expire
azp remove work              # delete the profile (asks first)
```

Profile names may only contain `[a-z0-9_-]`. `azp list` only reads
`azureProfile.json`, so it never calls `az` and never touches token caches.

## Shell integration

`azp use` only prints an `export` line, so wrap it in a function to switch the
current shell. Add this to `~/.zshrc`:

```zsh
azu() { eval "$(azp use "$1")"; }
```

Then `azu work` switches this shell to the `work` profile.
