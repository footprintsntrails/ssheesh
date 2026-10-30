# Installing SSHeesh on macOS

> SSHeesh is an early preview (0.1). Things will break; [tell us](https://github.com/footprintsntrails/ssheesh/issues/new/choose) when they do.

**Needs:** macOS 11 Big Sur or newer. Works on Apple Silicon (M1–M4) and Intel Macs, it's one download for both.

## 1. Download

Open the [latest release](https://github.com/footprintsntrails/ssheesh/releases/latest) and download **`SSHeesh_<version>_universal.dmg`**.

## 2. Install

1. Open the downloaded `.dmg`.
2. Drag **SSHeesh** into the **Applications** folder.
3. Eject the disk image.

## 3. First launch

SSHeesh isn't notarized by Apple yet (that needs a paid Apple developer account), so macOS blocks the first launch until you allow it. On macOS 15 Sequoia and newer, right-click → Open no longer gets around this, so use the steps below.

1. Open SSHeesh from Applications. macOS says “Apple could not verify SSHeesh is free of malware”. Click **Done**. Don't click Move to Trash.
2. Open **System Settings → Privacy & Security**.
3. Scroll down to the line about SSHeesh and click **Open Anyway**. The button only shows for about an hour after step 1, so if it's missing, repeat step 1.
4. Confirm with your password or Touch ID, then click **Open**.

You only do this once. Updates install without asking again.

<details>
<summary>macOS says the app “is damaged and can't be opened”</summary>

That's the same check, worded badly. Open **Terminal** and run:

```bash
xattr -dr com.apple.quarantine /Applications/SSHeesh.app
```

Then open SSHeesh again.
</details>

## 4. Updates

SSHeesh checks for updates by itself and installs them in the background. When one is ready, a **Restart to update** button appears in the toolbar at the top. Terminals reconnect after the restart.

Change this in **Settings → Updates** (⌘,): install automatically, ask first, or never check.

## Your servers

SSHeesh uses the `ssh` that comes with macOS, so everything in `~/.ssh/config` shows up in the sidebar and your keys just work.

For terminals that survive a dropped connection, install **tmux** on the server:

```bash
sudo apt install tmux      # Debian, Ubuntu
sudo dnf install tmux      # Fedora, RHEL
```

## Uninstall

Drag SSHeesh from Applications to the Trash. Settings live in `~/Library/Application Support/io.github.footprintsntrails.ssheesh`.
