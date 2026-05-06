# 7 Days to Die Mod Manager

A simple Rust desktop app for installing and managing 7 Days to Die mods from ZIP files.

## Features

- Auto-detects common Steam installs of `7 Days To Die`
- Lets you choose the game folder manually
- Installs mod ZIP files into the game's `Mods` folder
- Lists enabled and disabled mods
- Disables mods by moving them to `DisabledMods` beside the `Mods` folder
- Enables disabled mods by moving them back
- Removes selected mods with the `Remove Mod` button
- Protects `0_TFP_Harmony` from deletion
- Remembers the selected game folder

## Run

```powershell
cargo run
```

## Build

```powershell
cargo build --release
```

The release executable will be created at:

```text
target\release\seven_days_mod_manager.exe
```

## Notes

The app expects a mod ZIP to contain either:

- a single mod folder, such as `ExampleMod/ModInfo.xml`
- mod files directly at the ZIP root, such as `ModInfo.xml`

Disabled mods are moved out of the game's `Mods` folder so the game does not load them.

`0_TFP_Harmony` is treated as a protected game mod and cannot be deleted from the app.
