## What's new

{{changes}}

## Coming from 0.10

SSHeesh 0.10 updates itself to {{version}} as it always has: "Restart to update", and you're on {{version}}. Your servers, saved passwords, settings and the terminals running on your servers come along. Local terminals, and terminals on servers without tmux, close with the update, so finish what's running there first.

On Windows the version you had stays in the Start menu as "SSHeesh (previous version)" for 30 days.

## Download

| System | Download |
| --- | --- |
| **macOS** 11+, Apple Silicon and Intel | [**SSHeesh_{{version}}_universal.dmg**](https://github.com/footprintsntrails/ssheesh/releases/download/v{{version}}/SSHeesh_{{version}}_universal.dmg) |
| **Windows** 10 and 11 | The [**installer**](https://github.com/footprintsntrails/ssheesh/releases/download/v{{version}}/SSHeesh_{{version}}_x64-setup.exe), for you only, no administrator needed · or [**SSHeesh.exe**](https://github.com/footprintsntrails/ssheesh/releases/download/v{{version}}/SSHeesh.exe), nothing to install |
| **Linux** x64 | [**ssheesh-{{version}}-linux-x64.tar.gz**](https://github.com/footprintsntrails/ssheesh/releases/download/v{{version}}/ssheesh-{{version}}-linux-x64.tar.gz): one program, `tar xzf` and run `./ssheesh`. It needs Vulkan (Mesa's driver works) |

- **macOS:** the app isn't notarized by Apple: run `xattr -dr com.apple.quarantine /Applications/SSHeesh.app` once, or follow the [step-by-step guide](https://github.com/footprintsntrails/ssheesh/blob/main/docs/install-macos.md).
- **Windows:** if SmartScreen asks, click More info → Run anyway. [Step-by-step guide](https://github.com/footprintsntrails/ssheesh/blob/main/docs/install-windows.md)

`latest.json`, `update-v1.json`, the `.sig` files, `ssheesh-{{version}}-linux-x64` and `ssheesh-{{version}}-windows-x64.zip` are what the app uses to update itself. You don't need to download them.

After that, SSHeesh updates itself.
