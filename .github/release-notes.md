> [!WARNING]
> **Early preview.** This is a very early build of SSHeesh. Expect rough edges, missing pieces and bugs. Please [report what breaks](https://github.com/footprintsntrails/ssheesh/issues/new/choose).

## What's new

{{changes}}

## Download

| System | Download |
| --- | --- |
| **macOS** 11+, Apple Silicon and Intel | [**SSHeesh_{{version}}_universal.dmg**](https://github.com/footprintsntrails/ssheesh/releases/download/v{{version}}/SSHeesh_{{version}}_universal.dmg) |
| **Windows** 10 and 11 | [**SSHeesh.exe**](https://github.com/footprintsntrails/ssheesh/releases/download/v{{version}}/SSHeesh.exe), nothing to install · or the [installer](https://github.com/footprintsntrails/ssheesh/releases/download/v{{version}}/SSHeesh_{{version}}_x64-setup.exe), which also updates itself |
| **Linux** | [**SSHeesh_{{version}}_amd64.AppImage**](https://github.com/footprintsntrails/ssheesh/releases/download/v{{version}}/SSHeesh_{{version}}_amd64.AppImage) |

The app isn't signed by Apple or Microsoft yet, so the first launch needs a few extra clicks:

- **macOS:** skip all of that with one line in Terminal, which installs the latest version and opens it:
  ```bash
  curl -fsSL https://raw.githubusercontent.com/footprintsntrails/ssheesh/main/scripts/install-macos.sh | sh
  ```
  Downloaded the `.dmg` instead? Run `xattr -dr com.apple.quarantine /Applications/SSHeesh.app` once, or follow the [step-by-step guide](https://github.com/footprintsntrails/ssheesh/blob/main/docs/install-macos.md).
- **Windows:** click More info → Run anyway. [Step-by-step guide](https://github.com/footprintsntrails/ssheesh/blob/main/docs/install-windows.md)

- **Linux:** the AppImage is the whole app in one file. Make it runnable once (right-click → Properties → Allow executing as program, or `chmod +x SSHeesh_*.AppImage`), then double-click it.

`latest.json` and the `.tar.gz` below are what the app uses to update itself. You don't need to download them.

Your servers and settings live in your user folder (`%APPDATA%` on Windows, `~/Library/Application Support` on a Mac), not next to the app, so you can move or replace the app freely.

After that, SSHeesh updates itself.
