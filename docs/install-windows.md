# Installing SSHeesh on Windows

> SSHeesh is an early preview (0.1). Things will break; [tell us](https://github.com/footprintsntrails/ssheesh/issues/new/choose) when they do.

**Needs:** Windows 10 (version 1809 or newer) or Windows 11, 64-bit.

## 1. Download

Open the [latest release](https://github.com/footprintsntrails/ssheesh/releases/latest) and download **`SSHeesh_<version>_x64-setup.exe`**.

(There's also an `.msi` for company-managed PCs. Most people want the `setup.exe`.)

## 2. Install

Run the downloaded `setup.exe`. It installs for your user only, so it doesn't ask for admin rights.

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

SSHeesh checks for updates by itself and installs them in the background. When one is ready, a **Restart to update** button appears in the toolbar at the top.

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
