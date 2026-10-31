# Installing SSHeesh on Windows

> SSHeesh is an early preview (0.3). Things will break; [tell us](https://github.com/footprintsntrails/ssheesh/issues/new/choose) when they do.

**Needs:** Windows 10 (version 1809 or newer) or Windows 11, 64-bit.

## 1. Download

Open the [latest release](https://github.com/footprintsntrails/ssheesh/releases/latest). There are two ways to get SSHeesh; pick one:

- **`SSHeesh.exe`**: the whole app in one file. Nothing to install: put it anywhere (the desktop, a USB stick) and double-click it. It updates itself too: the new version takes the file's place, keeping its name, so shortcuts keep working. Keep it in a folder you can write to (not Program Files).
- **`SSHeesh_<version>_x64-setup.exe`**: installs SSHeesh for your user (no admin rights needed), adds it to the Start menu, and updates itself in the background.

Either way, your servers and settings live in `%APPDATA%\io.github.footprintsntrails.ssheesh`, and remembered passwords in Windows Credential Manager. You can switch between the two, or move the portable file around, without losing anything.

The portable file needs Microsoft Edge WebView2, which Windows 10 and 11 already have. If it won't start on an older, untouched PC, use the installer: it adds WebView2 for you.

## 2. First launch

### “Windows protected your PC”

SSHeesh isn't code-signed yet (that needs a paid certificate), so SmartScreen shows a blue warning the first time.

1. Click **More info**.
2. Click **Run anyway**.

You only do this once; updates install without the warning.

## 3. Make sure Windows has `ssh`

SSHeesh uses the OpenSSH client that ships with Windows. It's on by default in Windows 11 and recent Windows 10. To check, open **PowerShell** and run:

```powershell
ssh -V
```

If it says `ssh` isn't recognized, turn it on in **Settings → System → Optional features → Add a feature → OpenSSH Client**, or run this in an **administrator** PowerShell:

```powershell
Add-WindowsCapability -Online -Name OpenSSH.Client~~~~0.0.1.0
```

## 4. Updates

The installed version checks for updates by itself and installs them in the background. When one is ready, a **Restart to update** button appears in the toolbar at the top.

Change this in **Settings → Updates** (Ctrl+,): install automatically, ask first, or never check.

## Your servers

Hosts in `C:\Users\<you>\.ssh\config` show up in the sidebar automatically, and your keys in `.ssh` are used.

For terminals that survive a dropped connection, install **tmux** on the server:

```bash
sudo apt install tmux
```

### Known limitation on Windows

Windows' built-in `ssh` can't share one connection between tabs. The Files and Stats views therefore sign in on their own. With SSH keys this just works. If a server only accepts a password, open a terminal to it first.

## Uninstall

**Settings → Apps → Installed apps → SSHeesh → Uninstall.**
