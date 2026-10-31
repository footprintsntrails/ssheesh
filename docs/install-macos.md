# Installing SSHeesh on macOS

> SSHeesh is an early preview (0.3). Things will break; [tell us](https://github.com/footprintsntrails/ssheesh/issues/new/choose) when they do.

**Needs:** macOS 11 Big Sur or newer. Works on Apple Silicon (M1–M4) and Intel Macs, it's one download for both.

## The easy way: one command

Open **Terminal** (press ⌘Space, type `Terminal`, press Return), paste this line and press Return:

```bash
curl -fsSL https://raw.githubusercontent.com/footprintsntrails/ssheesh/main/scripts/install-macos.sh | sh
```

It downloads the latest SSHeesh from this page, puts it in **Applications** and opens it. macOS doesn't ask you to confirm anything, because the app didn't come through a browser. Run the same line again later to update by hand, though SSHeesh also updates itself.

## Or download it yourself

1. Open the [latest release](https://github.com/footprintsntrails/ssheesh/releases/latest) and download **`SSHeesh_<version>_universal.dmg`**.
2. Open the downloaded `.dmg` and drag **SSHeesh** into the **Applications** folder.
3. Eject the disk image.

### First launch

SSHeesh isn't notarized by Apple yet (that needs a paid Apple developer account), so macOS blocks an app that came from a browser until you allow it. The quickest fix is this line in **Terminal**:

```bash
xattr -dr com.apple.quarantine /Applications/SSHeesh.app
```

Then open SSHeesh as usual. If you'd rather click through it, on macOS 15 Sequoia and newer right-click → Open no longer works, so:

1. Open SSHeesh from Applications. macOS says “Apple could not verify SSHeesh is free of malware”. Click **Done**. Don't click Move to Trash.
2. Open **System Settings → Privacy & Security**.
3. Scroll down to the line about SSHeesh and click **Open Anyway**. The button only shows for about an hour after step 1, so if it's missing, repeat step 1.
4. Confirm with your password or Touch ID, then click **Open**.

You only do this once. Updates install without asking again.

<details>
<summary>macOS says the app “is damaged and can't be opened”</summary>

That's the same check, worded badly. Run the `xattr` line above, then open SSHeesh again.
</details>

## Updates

SSHeesh checks for updates by itself and installs them in the background. When one is ready, a **Restart to update** button appears in the toolbar at the top. Terminals reconnect after the restart.

Change this in **Settings → Updates** (⌘,): install automatically, ask first, or never check.

**Saved server passwords** live in your Mac's Keychain. The first time a new SSHeesh reads them, macOS asks whether to allow it: click **Always Allow**. From 0.6.0 every version is signed with the same certificate, so macOS knows an update is the same app and doesn't ask again. (Versions before 0.6.0 looked like a new app to macOS after every update, so it asked each time.)

## Your servers

SSHeesh uses the `ssh` that comes with macOS, so everything in `~/.ssh/config` shows up in the sidebar and your keys just work.

For terminals that survive a dropped connection, install **tmux** on the server:

```bash
sudo apt install tmux      # Debian, Ubuntu
sudo dnf install tmux      # Fedora, RHEL
```

## Uninstall

Drag SSHeesh from Applications to the Trash. Settings live in `~/Library/Application Support/io.github.footprintsntrails.ssheesh`.
