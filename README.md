# nixion

`nixion` is a Rust TUI for managing flakes, NixOS configurations, user profile packages, and system generations from the terminal.

It focuses on the operational workflow around a NixOS machine: inspect the current system, search and install user packages, work with a nearby flake, and switch or clean up system generations without leaving the terminal.

## Features

- List installed packages from `nix profile`
- Search packages in `nixpkgs`
- Install packages into the current user profile
- Remove packages from the current user profile
- Detect a nearby flake automatically, with fallback to `/etc/nixos`
- List `nixosConfigurations` hosts from the selected flake
- Run `nix flake update`, `nix flake check`, and `nix fmt`
- Run `nixos-rebuild switch`, `test`, or `boot` for the selected host
- Show the flake root, metadata, inputs, and discovered `.nix` files
- List NixOS system generations
- Activate, schedule for boot, or delete a selected generation
- Test a selected generation before switching to it
- Remove old generations from the generations tab
- Distinguish boot and running generations in the UI
- Show absolute and relative generation age in the list and detail panel
- Filter and page through large generation histories
- Open a rollback dialog for the proposed target generation
- Preview which generations cleanup will keep or delete

## Requirements

- Nix with the modern CLI enabled (`nix profile`, `nix search`, `nix flake`)
- NixOS for generation management
- A flake-based NixOS configuration for host rebuild actions
- `sudo` access for generation actions

## Development

With flakes:

```bash
nix develop
cargo run
```

Without flakes:

```bash
nix shell nixpkgs#cargo nixpkgs#rustc nixpkgs#rustfmt --command cargo run
```

Run tests:

```bash
cargo test
```

## Controls

- `Left` / `Right`: switch tabs
- `j` / `k` or `Down` / `Up`: move selection
- `r`: refresh current tab
- `u`: update flake inputs on the flake tab
- `c`: run `nix flake check` on the flake tab
- `f`: run `nix fmt` on the flake tab
- `w`: run `nixos-rebuild switch` for the selected flake host
- `t`: run `nixos-rebuild test` for the selected flake host
- `b`: set the selected flake host for next boot on the flake tab
- `/`: enter search input on the search tab, or filter generations on the generations tab
- `Enter`: run search, or open the rollback dialog on the generations tab
- `PageUp` / `PageDown`: jump through long generation lists
- `Home` / `End`: jump to the first or last visible generation
- `i`: install selected search result
- `d`: remove selected installed package
- `s`: activate selected generation now
- `t`: test selected generation without making it persistent
- `b`: set selected generation for next boot on the generations tab
- `p`: jump to the rollback target generation on the generations tab
- `x`: delete selected generation
- `o`: delete old generations on the generations tab
- `q`: quit

## Behavior Notes

- Package actions target the current user's `nix profile`, not declarative system packages in `configuration.nix`.
- Flake detection prefers a flake with `nixosConfigurations`; otherwise it falls back to the first usable flake it finds.
- Host rebuild actions call `sudo nixos-rebuild <action> --flake <path>#<host>`.
- Generation activation calls the selected generation's `switch-to-configuration` script via `sudo`.
- Generation actions that change or delete system state require `y` / `n` confirmation in the TUI.
- The generations tab shows the boot target from `/nix/var/nix/profiles/system` separately from the currently running system at `/run/current-system`.
- Cleanup of old generations is only enabled when the running system already matches the boot generation.
- The rollback dialog always targets the next older generation after the current boot entry.

## License

MIT. See [`LICENSE`](LICENSE).
