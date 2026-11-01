# Changelog

## 0.8.1 — quicker, and easier on your servers

- **A busy terminal doesn't slow the window down.** A `cat` of a big file or a long build log sent the window one message per read, tens of thousands a second. While output keeps coming it now arrives in small batches; after a pause it still comes at once, so typing feels the same.
- **The window doesn't wait on the keychain or the disk.** Adding, removing and listing servers, forgetting a password, trusting a server's key and Snap Layouts happen out of its way.
- **Server stats ask the server far less.** The look every two seconds at CPU, memory and network started ten programs on the server; now it starts none, and disks, the month's traffic and the server's name are read every 30 seconds. Stats stop asking a computer that can't give them (This computer on a Mac), ask less often while they fail, and wait while the server's terminals reconnect. The container list asks less while it fails, and not while the window is hidden.
- **Tunnels sit still.** Each one woke ten times a second to check on its ssh; now it waits for ssh to finish.
- **Files scrolls smoothly.** A long folder no longer draws the whole list again on every frame of a scroll, and its dates take far less work to write.
- **Terminals do less each time they draw.** Hidden secrets are looked for only in lines that changed, highlighting does less work per piece of output, and a terminal in another tab marks its links when you come back to it.
- A progress bar that reports the same value again no longer redraws the window.

## 0.8.0 — a calm new look, and a safer server in a click

- **Security check-up and Free up disk space (once "What's using the disk"), at the foot of the sidebar.** Two buttons over Settings look at the server you have open; with several open, you pick one. They've left the tab and stats menus, where they were one more thing to read past.
- **Run, instead of Paste into terminal.** A check-up fix or a cleanup runs on the server when you press Run, without a terminal, after one question, and says how it went (its output, or the error); the check comes back up to date. One that needs you (sudo asks for your password, or a package manager asks whether to go ahead) opens in a new terminal instead, with what to type in plain words. A recipe's steps run in the server's terminal, so what they print is there to compare.
- **SSHeesh Default, the new look:** Claude's warm dark with Claude Code's green, yellow and red, and the >_< logo's green as the accent. Midnight is still in Settings → Appearance. If you kept the look SSHeesh came with (Midnight in blue), you're moved over once; a theme or accent you picked stays. Claude Code's orange is in the accents too, as Terracotta.
- **Win+V pastes into terminals.** Windows' clipboard history (and the emoji panel) typed nothing, because the terminal thought the Win key was still held down.
- **Icons from one set.** Search, the two new buttons, Settings and Add server are drawn like the server icons beside them: in lines, or with the pixel icons, in pixels, the logo too (as the terminal's welcome draws it).
- **The green is softer**, in the logo, the welcome, status colors and the app icon.
- **Quieter terminals.** The buttons that start an agent show up when you point at the pane's title, instead of sitting over every terminal in four colors.
- **Pasting can't run things by itself.** A paste loses the control characters a terminal would take as keys (Esc, Ctrl+U, a lone Enter), so text can't end the shell's paste guard early and run a line.

### Fixed
- **Grey blocks in Mac terminals stay away.** When macOS took the GPU back (after sleep, or with many terminals open), a terminal went on drawing without it for good, and WebKit's grey blocks came back. Now the GPU comes back, and a terminal out of sight for half a minute gives its GPU back so the ones on screen keep theirs.
- **No more rainbow over agents and full-screen programs.** Error, warning and address highlighting stays out of the way while Claude Code, Codex, OpenCode, oh-my-pi, vim, htop or less runs: they redraw lines in pieces, and the highlight landed on half an address one time and not the next.
- **No stray dotted lines.** A path's dotted underline goes as soon as its line is drawn again, instead of staying where the text was while an agent's spinner keeps output coming.
- "Open new terminal here" and starting an agent work in Windows' PowerShell, which has no `&&`.
- **Dialogs keep the keyboard.** Shortcuts no longer open tabs behind a dialog, and Tab and Esc stay in it, instead of typing into the terminal behind.
- **Menus work from the keyboard:** arrows move through them, and Esc closes the menu without clearing the files you picked.
- Esc in an icon or color picker closes just the picker, not Add server (with what you typed) or Settings.
- The tab you're on stays in sight in a long row of tabs, which fades where more are off to the side.
- A recipe stepped aside sits in the top bar beside the tabs, instead of over them.
- Add server brings the connection test's answer into view.
- Settings' close button no longer covers what's under it as you scroll.
- Archive is in the menu of several picked files too.
- Filtering a long folder starts at the top of what it found, at once.
- Keeping an edited file open after "Discard your changes?" leaves the list on that file, and the question names it.
- The server card names the disk it means ("Disk /mnt/backups"), and doesn't say "no terminal" for one that just hasn't connected yet.
- Dialogs fit a short window, with their buttons in sight.
- Text on the accent color stays readable whatever the accent.

### Security
Severity is rated with how likely an attack is; in brackets, what it would be on impact alone.

- **A server file name could place a download outside Downloads on Windows.** A Linux file named like `x\..\..\AppData\…\Startup\evil.bat` landed in the Startup folder and ran at the next sign-in. Names are now made into one plain Windows name. Severity: Low (High without likelihood).
- **A folder name could run a command when you opened a terminal in it.** "Open terminal here", "Open in main terminal", starting an agent and dropping a file on this computer's terminal typed the name with only quotes around it; Ctrl+U and Enter inside a name acted as keys. Such names are now refused with a note, and the quoting also holds in fish and PowerShell. Severity: Low (High without likelihood).
- **Server output could set your clipboard to a command that ran when pasted.** Programs may copy (OSC 52); what they copy and every paste now lose those keys. Severity: Medium (High without likelihood).
- **Copying between servers with Replace took everything the first server sent.** A compromised server could add, say, `.ssh/authorized_keys` next to the folder you copied. Only what you picked arrives now. Severity: Low (High without likelihood).
- **A saved password could be given to a jump host.** Behind ProxyJump, the hop's own login got it. It's now typed only for the server it's saved for, and not at all behind a hop with an ssh older than OpenSSH 8.4, which can't show who's asking. Severity: Low (Medium without likelihood).
- **A container's label could make up a container whose name was a command,** typed by the "pick a container" snippets. Names docker and podman can't give are dropped. Severity: Low (High without likelihood).
- **The Windows app could start an `ssh.exe` or `tar.exe` from its own folder,** which for the portable app is often Downloads. It now looks only where Windows keeps programs. Severity: Low (High without likelihood).
- **On Linux without a Downloads folder, downloads went straight into your home folder,** where a server's `.bashrc` would be live. They go to ~/Downloads now. Severity: Low (High without likelihood).
- **A server's key was fetched through a file in the shared /tmp** (Linux), where another user on the computer could slip in a key of their own. It's fetched in your own ~/.sshee now, only lines for that server are kept, and nothing is offered to trust without a fingerprint to compare. Severity: Low (Medium without likelihood).
- **An upload into a folder others can write to (/tmp) could go into a file someone else made** under its temporary name. Its temporary file is always new now. Severity: Low (Medium without likelihood).
- An archive whose name has a `:` in it is no longer taken by GNU tar as a file on another computer. Severity: Low (Low without likelihood).

## 0.7.3 — the Windows app updates more surely

- **The Windows app (SSHeesh.exe) updates itself more surely.** It gives your antivirus a moment to look at the new version before swapping it in, and tries again while something else holds the file. If Windows Security takes the new version away (SSHeesh isn't signed yet), it says so and where to allow it, instead of "the file can't be found". If the running copy can't be replaced, the new one is left next to it as `SSHeesh-<version>.exe`, to open by hand. Each failure now says which step it was. The update to this version still runs the old way, so if it fails, download it from the release page once.

## 0.7.2 — at the right line

- **A path with lines opens at them.** Click `notes.md:242-296` (or `main.rs:12`, `main.rs:12:5`, GitHub's `#L10-L20`) and the file opens beside the terminal scrolled to those lines, with a band marking them and "lines 242–296" over the file. Formatted Markdown marks the paragraphs, headings and code those lines make up; Source marks the lines themselves. Before, the line number was dropped and the file opened at the top.
- **Old temporary files can always be cleaned up** from "What's using the disk": the Temporary files card shows whenever anything in /tmp or /var/tmp is over a day old, with how much it frees, instead of only from 50 MB up. It starts at files a week old when there are any, else a day.

## 0.7.1 — a click opens it

- **A click on a path or link in the terminal opens it**, no Ctrl or ⌘ needed: the file beside the terminal, a link in your browser. A double-click or a drag across a path still just selects it, and Ctrl+click (⌘-click) still opens at once.
- **This computer in the Pixels style** is a small old desktop, so it's plainly pixels like the rest; the laptop it was looked just like the line one. The laptop is still in the icon picker, now with pixel corners.

## 0.7.0 — files that don't fall over, secrets that stay hidden

- **Uploads and downloads go on after a dropped connection.** When the Wi-Fi drops or the laptop sleeps, the card in the corner says it's waiting, and once the server answers again it carries on by itself: a file from the very byte it stopped at, a folder (and a copy between two servers) from the start. It waits up to ten minutes without getting anywhere, and Stop works all the while. A file that changed on the server meanwhile is fetched again from the start, so nothing comes out mixed.
- **Secrets in the terminal show as dots.** GitHub, GitLab, OpenAI, Anthropic, Stripe, Slack, AWS, Google and Telegram bot tokens, JWTs, private keys, and whatever follows `PASSWORD=`, `api_key:` and the like, or sits as a password in a link. The name stays readable, ordinary words like `max_tokens: 4096` stay as they are, and the dots are there from the first moment the text is, never a flash later. Click one to see it; in streamer mode they stay hidden, clicked or not. The "While you were away" card hides them too. Settings → Terminal → Hide secrets.
- **Two styles for server icons: Lines and Pixels.** Clean bold lines, or pixels like the >_< logo, for all your servers at once: switch in the icon picker (right-click a server → Icon and color) or in Settings → Layout → Server icons. There are 30 icons now, among them AI, Git, mail, monitoring, backups, VPN, chat, a shop and smart home, and any color of your own beside the six ready ones. This computer is drawn in the same style.
- **Bigger icons in the slim sidebar** when it shows no load, a little smaller when it does. The load beside the icons is now off for everybody; Settings → Layout → Server load brings it back.
- **Compact mode hides what you pick:** the sidebar, the tab bar, or both (both also hides the vitals, as before). The shortcut brings back the one you used last, and the tab bar slides in over your work, not over the sidebar.
- **A calmer top bar.** The Files, Tools and side-by-side buttons are gone from it: Files and a server's tools are in the menu under its name and on right-click, side by side is Ctrl+Shift+S or dragging a tab onto the page. Settings is a bigger gear at the bottom of the sidebar, and in the top bar only while the sidebar is hidden.

### Fixed
- "Main menu" in the top bar stays on one line beside many tabs.

## 0.6.8 — every server's tabs, side by side

- **One row of tabs for every server.** The tabs along the top are all your servers' tabs, as in a browser, and each says whose it is once more than one server has tabs open: “Files (web-1)”, “Terminal 2 (Claude Code, web-1)”. A thin line stands between one server's tabs and the next. Clicking a tab goes to its server, Ctrl+Tab goes through all of them, you can drag any of them anywhere, and the row stays on the main menu. Closing a server's last tab goes to the tab beside it instead of the main menu.
- **Two servers side by side.** Drag a tab of another server onto the page and it shows beside the one you're on: its Files beside this one's, its terminal beside this one's. Each half says whose it is, and the toolbar follows the half you're in. As with any pair, another tab shows on its own and either of the pair brings both back; × sends it back to its own server, still open.
- **Copy files from one server to another.** Drag files from one server's Files onto another's beside it, into the folder open there or onto a folder, or right-click them and pick the server and the folder (Copy to another server…). Folders too. They go straight through SSHeesh, so the servers don't need to reach each other and nothing waits on your disk; progress and Stop are on a card in the corner, nothing is replaced without asking, and a stopped copy leaves nothing behind. From This computer's files it's an upload.
- **A download's note opens its folder** with the file picked out: click “Downloaded …”.

### Fixed
- **No more grey blocks and smeared colors in Mac terminals.** Macs now draw terminals with the GPU, as Windows does; the other way left stray color blocks behind in the Mac's WebKit. It's switched on once for Macs that had it off by default (Settings → Terminal → Draw with the GPU).
- The server's name at the top left keeps its room when there are many tabs.

## 0.6.7 — the welcome, with any hosting

- **SSHeesh's welcome shows even when your hosting adds a script of its own** to the server's message. A script that adds nothing to what you see no longer makes the server look like it has a welcome of its own; one that does still counts. Backup copies of scripts (`.dpkg-old` and the like) are left out, as the server leaves them out too, and the standard scripts of Azure, minimal Ubuntu and Debian are known.
- **A firmware update waiting** (fwupd) is kept with the other notices, instead of making the server look like it has a welcome of its own. Notices about an old release, the hardware support stack, a disk check at the next reboot and updates that couldn't install on their own stay too.

## 0.6.6 — the welcome on Ubuntu

- **SSHeesh's welcome shows on Ubuntu servers too.** Ubuntu's own message (its news, help links and system summary) is the distribution's, not the server's, so SSHeesh's welcome takes its place; before, Ubuntu's news made it look like the server had a welcome of its own. A server has its own when there's text in /etc/motd or a script someone added to /etc/update-motd.d. The notices stay, in their paragraphs: updates, a security update, a new release, ESM.

## 0.6.5 — tabs on top, a welcome, your own colors

- **A welcome for your servers.** A new terminal shows the server's own welcome if it has one, and SSHeesh's if it doesn't: the logo beside the system, uptime, load, memory and disk, containers and updates waiting. The server's notices (updates, a restart needed) always stay. Settings → Terminal → Welcome: SSHeesh's every time, only the first time, the server's own, or nothing; pick what it shows and add a line of your own, with a live preview.
- **Tabs are along the top by default**, as in a browser, and only there: they no longer show under the server at the same time. Under each server is still a choice in Settings → Layout. The server list says "Needs you" when an agent on that server waits for an answer.
- **The server list is a slim column of icons by default**, so your work gets the room; point at an icon for the server's name, and it stays quiet: no load meters unless you turn them on (Settings → Layout → Server load). Settings → Layout → Server list → Names brings the wider list back.
- **× never closes a tab while panes are side by side**, in either layout: it takes the pane off the screen and the tab stays, to close from the tab list. A pane on its own closes with × as before.
- **Your own colors.** Pick any accent color from a palette, and give the terminal your own background and text color ("Your own" under Settings → Terminal).
- **The interface size slider no longer jumps**: the new size applies when you let go.
- **Clearer settings**: groups like Highlighting have a heading of their own instead of looking like one more setting.
- The tour draws tabs where yours are.
- **Faster connects** where the system tries Kerberos first (Debian and Ubuntu do): SSHeesh skips it.

### Fixed
- **Paths are underlined in tmux sessions too**, where agents usually run; before, only plain shells got the marks.
- **Ctrl+click on a file** opens the file beside the terminal every time; when the two were being put side by side, it sometimes showed only its folder.
- **Ctrl+click on a path that's gone** (a log that was just deleted, say) says so clearly, and a search that takes a moment says it's looking.
- **Ctrl+click finds** paths with brackets in them (`app/[id]/page.tsx`), and gives up after a few seconds instead of searching a big home folder for long.
- **Side by side, one tab at a time:** closing one of three panes keeps the other two; switching the layout in Settings keeps the tab you're on visible; dropping a tab on the middle of a pane swaps it into the pair; Split adds a pane; Ctrl+Tab goes through all tabs.
- **Your own terminal colors** apply to open terminals right away.
- **Dialogs opened from Settings** (What's new, choosing ~/.ssh/config servers) show on top instead of behind it, and Esc closes only the dialog on top.
- **Streamer mode** also hides names and addresses in the ~/.ssh/config list, the command bar's folders, notifications, tooltips and container errors.
- **Two servers with the same name** no longer mix up in the command bar.
- **The welcome for returning users** no longer adds every server from ~/.ssh/config, and "Show the welcome again" shows the whole welcome.
- **Ctrl+Shift+Z is redo** in text fields again.
- **Adding a server typed as `1.2.3.4:2222`** keeps the port, and port 0 is refused.
- **The portable app** no longer opens the download page by itself every few hours when an automatic update fails.
- **Pasting a lot of text** into a terminal whose program isn't reading can no longer freeze SSHeesh.
- **1Password's SSH agent** works on a Mac (its path has a space).
- **A dropped connection** is never mistaken for a refused login because the server's last output said "Permission denied".
- **Closing a tab you hadn't opened since launch** ends its session on the server too.
- **Agent buttons** open a new terminal when SSHeesh can't tell whether a program is running, instead of typing into it.
- **"Needs you"** no longer fires for prompts that report their folder (oh-my-posh, Windows Terminal setups).
- **Linux: no crash** when a saved password is read from the keychain, which could break the server list until a restart.
- **Tunnels** start once, from the main window, and a removed server's tunnels stop.
- **"Paste into terminal"** never goes into a container's shell or logs.
- **Security check-up:** private addresses (10.x, 192.168.x, Docker, Tailscale) aren't called open to the internet, and it doesn't offer to turn off password login while SSHeesh itself logs in with a password.
- **Network traffic** is no longer counted several times over bridges, bonds, VLANs and Proxmox devices.
- **Snippets** leave Go templates like `{{end}}` alone, and Paste puts a multi-line snippet in as one paste instead of running each line.
- **Files:** a link to a folder downloads as the folder, not just the link. Uploading several files asks once about every file that would be replaced, and replacing keeps the old file's permissions. Unsaved edits ask before a folder change or another file closes them. Files that aren't UTF-8 open read-only instead of being saved back changed, Windows line endings stay as they were, and a dropped connection can't leave a file cut off. A folder that fails to open says so instead of showing the previous one. Opening a folder from a terminal link clears the old selection and filter. "New file" won't empty a file that already has that name, and copying a folder into itself is refused.

## 0.6.3 — one tab at a time, for real

- **Side by side is a pair, as in a browser.** With one tab at a time, a tab you drop onto the page (or Ctrl+Shift+D) makes a pair with the one you're on. A new tab, or a click on a tab outside the pair, shows that tab on its own; a click on either tab of the pair brings both back.
- **Closing side by side never closes a tab.** A pane's × takes it out of the pair and the tab keeps running. Closing a tab is in ⋯, the tab list and Ctrl+W.
- **Files always opens, in one place.** The Files button, the server's menu, a tab's menu, "Browse files in this folder", bookmarks and the command bar all go to the server's one Files tab, at the folder you asked for, instead of opening a second one or squeezing it in beside the others.
- **Fixed: Files not showing although its tab was picked.** Double-clicking a pane's title bar quietly made it fill the window, and later that pane covered everything else. With one tab at a time, double-click does nothing, and going to another tab always shows it.
- **New tabs show up in the list** after you drag a tab onto the page; before, they stayed missing until a restart.
- **Ctrl+click finds paths an agent prints from its own folder**, like `out/reports/summary.txt`: SSHeesh looks in the terminal's folder and the ones above it, in the folders your programs run in, and in your home folder.
- **Paths and links in a terminal are faintly underlined**, so you can tell they open with Ctrl+click (⌘-click on a Mac); pointing at one says so.
- **The Main menu button looks like a button** in the sidebar: it sits on a card of its own.

## 0.6.2 — no more stray numbers on Windows

- **No more "61;6;7;21;22;23;24;28;32;42c" or "32;29;29M" at the prompt on Windows.** Windows passes a terminal's replies and mouse reports on to ssh in pieces, and tmux on the server waited only 10 ms for the rest, so over Wi-Fi it sometimes gave up halfway and the rest showed up as text. With SSHeesh on Windows it now waits 100 ms; Esc still feels instant.
- **Clicks and drags during a reconnect no longer turn into text** in the next shell. When the connection drops, the terminal stops sending mouse and focus reports until tmux (or the program) asks for them again.

## 0.6.1 — tabs that belong to their server

- **Tabs look like they belong to their server.** Each server's tabs sit under a quiet "Tabs" heading and hang from a line that starts at the server's icon; the tab you're on is marked on that line. Servers are a little bigger and tabs a little smaller, so the list reads at a glance.
- **A terminal with an agent keeps its name**: "Terminal 2 (Claude Code)", "Terminal (Codex)", "Terminal (OMP)" for oh-my-pi. "Needs you" gets a line of its own, so the name isn't cut off.
- **New terminals open on the server you're on.** Splitting (Ctrl+Shift+D and Ctrl+Shift+E, ⌘D and ⇧⌘D on a Mac) opens a terminal on the same server right away instead of asking where to. On the main menu, Ctrl+Shift+T (⌘T) opens one on the server you were on last; before, it did nothing.
- **"Show me" no longer closes the hello.** It plays the feature right there, so you can look at each one and then choose. "Your own colors and keys" opens the page to set them up, with a way back.
- **The tour looks like SSHeesh**, with made-up but real-looking files and code. In "Find anything" the command bar no longer slides off the edge of the picture.

## 0.6.0 — a calm window for your servers

SSHeesh now works like a browser for your servers: each server is like a site, with its tabs right under it. The first time you open this version, a short hello shows what changed, and every choice you made before stays.

### Tabs under their server
- **Every server's tabs sit under it in the sidebar**: terminals, files, stats and AI agents. Only one server is on screen at a time, and clicking a server takes you back to the tab you were on there. Prefer a strip along the top? Settings → Layout.
- **Agents show up by name**, on every server: a tab running Claude Code, Codex, OpenCode or oh-my-pi is called after it, and says **Needs you** when it waits for an answer.
- **Drag a tab onto the page** to see two at once, dropping it where you want it. Drag a pane's title bar back onto the list, or click its "Back to its own tab" button, to make it a tab again. **Ctrl+Shift+S** (⇧⌘S) switches between one tab at a time and side by side; right-click a tab to show it beside or below the one you're on.
- **Reopen the tab you closed** with Ctrl+Shift+Z (⇧⌘T), or from the main menu.
- **Shift+click a server** to open it in a window of its own.
- One tab at a time is the new starting point for new users; the hello offers side by side.

### The main menu
- **The logo at the top of the sidebar opens the main menu**, a start page like a browser's new tab: a search field, your servers as tiles, the agents at work on any of them, and the tabs you closed lately.
- **Server icons say what a server is for**: the first time it connects, SSHeesh sees whether it serves a website, runs a database or hosts Docker containers, and picks the icon. Every server also gets a color of its own, so they're told apart at a glance. Pick another icon, or **your own picture**, from the server's menu.

### Make it yours
- **A new hello.** Your servers first, then one page to make SSHeesh yours with a live preview of the app beside it, then an optional one-minute tour of the best tricks. "Hide names" is there from the first second, in case you're sharing your screen.
- **Edit any theme down to every color**: start from a built-in theme, then change colors with a picker, hex values, a palette and transparency. SSHeesh warns when text gets hard to read. Share a theme as text, and import one.
- **Your own keyboard shortcuts.** Settings → Keyboard lists them all; click one and press new keys. Keys the terminal needs can't be taken.
- **Tips** appear the first time you meet a feature, one at a time, and never take your keystrokes. Settings → About turns them off, shows the welcome again or starts the tour.

### Files
- **Markdown files open formatted** in the preview, with a Preview / Source switch. Links open in your browser; images aren't loaded.
- **Bookmarks, places and recent folders sit behind one button** instead of a column.
- **Ctrl+click a path in a terminal** now always opens it beside the terminal, also when you work with one tab at a time.

### Quieter and clearer
- **One top row** with the server's name and address (click it for the server's tools), Files, Tools, side by side and Settings. "Connected" no longer shows all the time; only trouble does.
- **On Windows, SSHeesh draws its own title bar** and gives the space to your work. Hovering maximize opens Snap Layouts. Settings → Layout → "Use the Windows title bar" brings back the system's.
- **Menus and dialogs are solid**, never see-through over your text. A theme of your own can make them translucent.
- **Text on accent buttons is dark**, and readable (7:1 instead of 2.7:1). Light theme status colors are darker, so warnings read as small text too. Orange and pink are gone from the accents: they looked like warnings.
- **An optional red or yellow line** along the top of the window for servers labeled Production or Staging.
- **The command bar moved to Ctrl+Shift+L** (Ctrl+Shift+K still works), and to ⌘L on a Mac, where **⌘K now clears the terminal**, as in other Mac terminals. Snippets say which server they run on.
- **Shortcuts work on any keyboard layout**, Russian or German included.

### Faster
- **Folders open in about one network round trip.** Every request to a server costs about 20 ms less, and background updates (stats, agents, containers) no longer hold up the folder you're opening: on Windows that could take most of a second.
- **Folders you just saw open instantly** and refresh in the background, and a folder starts loading when you point at it.
- **Big folders sort about 20 times faster**, and typing in the filter only filters.
- **SSHeesh starts faster**: Settings, the hello, the tour and the Markdown preview load the first time you open them, and saving your tabs waits for a quiet moment instead of writing on every change.
- Each server keeps its own layout, so arranging, filling the window or "Terminal and files side by side" on one server leaves the others as they were.

### Also in this version
- **The app icon fills its tile on Windows and Linux**, as other apps' icons do, instead of looking a size smaller in the taskbar. macOS keeps the icon drawn on Apple's grid, which leaves the margin the Dock expects.
- **The portable Windows app is just `SSHeesh.exe`** on the release page. A copy you already have keeps its name when it updates.
- **On a Mac, saved passwords stop asking for your Mac's password after every update.** Every macOS build is now signed with the same certificate, so macOS knows an update is the same app. Updating to 0.6.0 asks one last time: click **Always Allow**.
- **Clean up temporary files** in "What's using the disk": pick how old they must be (a day, a week or a month) and see what that frees. Only files in /tmp and /var/tmp that nothing has opened or changed for that long; sockets like tmux's, services' private folders and display lock files stay.

## 0.5.1 — real tabs

- **Tabs you can move and close.** In the Tabs layout every terminal and file manager of the server is a tab: drag one to move it, close it with its × or a middle click, or right-click for "Close other tabs" and "Close tabs to the right". The + next to them opens a new terminal or file manager.
- **Ctrl+Tab and Ctrl+Shift+Tab** go to the next and previous tab (side by side: the next pane). **⌘T / Ctrl+Shift+T** opens a new terminal.
- **A tab keeps its name.** Closing "Terminal 2" no longer renames "Terminal 3"; the next new terminal takes the free number.
- **Stats stay at the bottom.** They're no longer a tab or a button at the top, and a server's workspace no longer opens a Stats pane: the vitals bar shows them, and clicking it opens the full view (click again to go back, in the Tabs layout).
- In the Tabs layout, a pane's title bar no longer offers to split or fill the window.

## 0.5.0 — commands and tunnels

### Snippets and a sharper command palette
- **Snippets: commands you keep and run again.** Leave blanks in them, like `{{service}}` or `{{lines:100}}`, and SSHeesh asks for them each time; `{{container}}` lets you pick one of the server's containers. They run in the terminal of the server you're looking at. A few useful ones are there to start with.
- **Narrow the palette down** (⌘K / Ctrl+Shift+K) by typing `srv:` for servers, `cmd:` for snippets, recipes and server tools, `file:` for bookmarked and recent folders, or `set:` for settings.

### Recipes that explain every command
- **Eight step-by-step guides**: log in with a key and turn passwords off, turn on the firewall, stop password guessing with fail2ban, install security updates automatically, use your own user instead of root, install Docker, add swap, and put a web app behind HTTPS with Caddy.
- **Every command comes with a sentence saying what it does.** Commands go into the terminal without pressing Enter, so you run each one yourself. Steps that could lock you out say so first, check that you can still log in, and say how to undo them.
- Commands for Debian and Ubuntu or for Fedora and RHEL, picked from what the server runs. Findings in the security check-up link to the recipe that fixes them.

### The server's web apps in your browser, and saved tunnels
- **Web apps on this server**: SSHeesh finds what the server serves (Grafana, a dev server, a Docker app) with its page title. Click Open and it's in your browser at `localhost`, through your SSH login, without opening anything to the internet.
- **Saved tunnels** bring a server's port (or a SOCKS proxy) to this computer, with a switch for each. They reconnect on their own when the connection drops, and the ones that were on start again when SSHeesh opens.

### Terminal
- **Ctrl+click a file path in the output** (⌘-click on a Mac) to open it in the file manager beside the terminal, with the file picked out and shown. Paths like `src/main.rs:12` and `./notes.md` work too, from the folder the terminal is in. Ctrl+click a web link to open it in your browser.
- **A scrollbar**, also for the history of server terminals, which lives in tmux on the server. Drag it, click above or below it, or press Shift+Page Up and Shift+Page Down. Typing while you're scrolled back takes you back to the prompt.
- **The server's welcome message** (motd) shows when a new session starts, as it does when you log in with ssh.
- **Resizing no longer makes the text jump.** The terminal takes its new size once you stop dragging, instead of redrawing on the server at every step.
- On Windows, connecting no longer types `61;6;7;21;22;23;24;28;32;42c` into the shell.

### Tabs or side by side
- **Terminal, Files and Stats switch on and off** at the top: with only Terminal on, the terminal fills the window; with Terminal and Files on, they share it. Switching one off keeps it running.
- **Or use them as tabs**: one fills the window at a time, and a second terminal gets a tab of its own. Pick it in the welcome or in Settings → Layout.

### File manager
- **Folders open much faster on Windows**, and big folders faster everywhere. SSHeesh stays signed in to the server between clicks instead of logging in again for every folder (Windows can't share one connection the way macOS and Linux do). Folders you've seen show up at once while they refresh, the folder under the pointer loads before you click it, and a folder with thousands of files only draws what's on screen.
- **Click any folder in the address bar** to go there; click the current folder or the space after it to type a path.
- **A `..` row** at the top of every folder goes up one.
- **Back and Forward** with the mouse's side buttons, Alt+← and Alt+→, and a Forward button.
- **Bookmarks on the left or the right**: Settings → Layout.

### Fixed
- "What's using the disk" promised far more from removing unused Docker images than it frees, and kept promising it after you'd removed them: Docker counts layers those images share with ones in use. It now counts only what the cleanup really removes, and leaves the card out when no image is unused.
- Drop-down lists on Windows showed white text on a white background.
- Picking an icon for a new server opens the icons under its name instead of a pop-up over the colors.

## 0.4.2 — the portable app updates itself

- **The portable Windows app updates itself**, like the installed one: it downloads the new version, checks its signature, takes its place, and asks you to restart. Your servers and settings stay put. This works from 0.4.2 on, so getting 0.4.2 itself is one last manual download.
- The security check-up and "What's using the disk" windows have a close button.

## 0.4.0 — know your server

### A security check-up
- **One click checks a server's security** without changing anything: password login, root login, ports open to the internet, the firewall, Docker ports that slip past ufw, automatic updates, protection against password guessing, and updates waiting for a restart.
- **Packages with known holes.** The check-up lists installed packages that have security fixes waiting, with their CVE numbers when the system knows them, and flags ssh, OpenSSL, sudo and the kernel. It asks the server's own package manager, so your package list goes nowhere.
- **Each finding says why it matters** in plain words, problems first. Where there's a fix, it's a command you can copy or paste into the server's terminal, and nothing runs until you press Enter.
- Find it in Stats, by right-clicking a server, or by searching for "security" (⌘K / Ctrl+Shift+K).

### What's using the disk
- **How full each disk is, and the biggest folders.** Click a folder to look inside, or open it in Files.
- **Docker's images, containers, volumes and build cache**, plus the system journal and package caches.
- **Safe cleanups** that say what they remove and roughly how much they free, as commands you copy or paste into the terminal. SSHeesh never deletes anything on its own, and doesn't offer to delete Docker volumes, which hold data.

### Start an AI agent where you are
- **The logos of the agents a server has** (Claude Code, Codex, OpenCode, oh-my-pi) sit in the terminal's title bar. Click one to start it in the folder you're in. If the terminal is busy with something else, the agent starts in a new terminal beside it, in the same folder.

### Terminals made for AI coding agents
- **Claude Code, OpenCode and other full-screen programs get full color.** Servers now know your terminal shows millions of colors, so programs stop falling back to 256.
- **Esc works right away**, also on servers with an older tmux, which waited half a second for more keys. Stopping an agent feels instant.
- **Shift+Enter starts a new line** in Claude Code, OpenCode and Codex instead of sending your message. In a plain shell it still runs the command, like Enter.
- **Icons in oh-my-posh and Starship prompts show up** instead of empty boxes: SSHeesh brings the Nerd Fonts symbols along.
- **Emoji take two cells**, as the server counts them, so the cursor no longer drifts after one.
- **Draw with the GPU** in Settings → Terminal: smoother for programs that redraw a lot, and box lines join up. On by default on Windows.

### Copy and paste
- **Selecting text copies it**, in server terminals too. So do the copy commands of Claude Code, OpenCode and vim. Turn it off in Settings → Terminal.
- **Ctrl+C copies when text is selected**, as in Windows Terminal; with nothing selected it stops the running command, as always. Ctrl+Shift+C and Ctrl+Shift+V always copy and paste.
- **Pasting on Windows.** Ctrl+V pastes, and so does clipboard history (Win+V), which presses Ctrl+V for you. Before, it sent a control key to the server instead.
- A server can put text on your clipboard but never read it.

### See uploads and downloads
- **A card in the corner for every upload and download**: how much is done out of how much, the speed, and an X to stop it. Stopping leaves nothing half-done behind, on the server or on your computer.
- An upload that gets cut off no longer leaves a broken file in place of the one that was there.
- The drop-to-upload window closes as soon as you press Upload; the card shows how it goes.

### Fixed
- A file preview no longer spills out of a narrow Files pane: it takes the whole pane until there's room for both. The wide bookmarks column waits for room too, and the status bar stays on one line.
- The vitals bar keeps its buttons in a narrow window, and toasts sit above it instead of covering it.
- Settings' close button stays put while you scroll.
- In compact mode the toolbar waits a moment before sliding in, so reaching for a pane's buttons doesn't cover them. Panels that slide in have an edge line.
- A server's card in the icon rail no longer says it isn't open when Files or Stats is.
- The Compact mode switch in the welcome no longer gets squeezed by its description, and the welcome's choice buttons don't stretch across.
- The Add server window shows that there's more below.

## 0.3.5 — live servers

### See how your servers are doing
- **Small CPU and memory meters under every open server** in the sidebar. They come from the same numbers as the vitals bar, and servers you haven't opened aren't asked for anything.
- **A warning line when a disk or memory is nearly full**, like "Disk 91% full", so you notice before something breaks.
- **When the sidebar is folded into icons**, each open server shows its load on its icon. Pick the style in Settings → Layout → Server load, or in the welcome's Layout step: Frame (the default), Bars, Pulse, Count, or Off.
- **Point at a server icon** for a card with its address, how many panes are open, CPU, memory, disk and uptime.

### Fixed
- **"Remember password" works after a restart.** The password went into your keychain, but SSHeesh forgot it had saved it and asked again next time. Servers you added before this update find their saved password on the next connect, so there's no need to add them again.

## 0.3.4 — drop files anywhere

- **Drop files on a server's terminal to upload them.** SSHeesh asks where to put them, starting with the folder that terminal is in, and you can change it before uploading.
- **Drop files on a file manager** and they go straight into the folder it has open, on a server or on this computer.
- Dropping files on this computer's terminal types their paths, like any terminal app.
- While you drag, the pane under your files lights up and says what will happen.
- The **Root** place is now called **Disk**, so it doesn't read like the root user's folder. Home's tooltip shows its real path (`/root` when you sign in as root).
- ssh's notes about its shared connection ("Shared connection to … closed.") no longer show up in terminals or error messages.

## 0.3.3 — set it up your way

### A guided setup
- **The welcome walks you through making SSHeesh yours.** Colors and accent; the terminal's color scheme, font, text size and cursor; the layout (server list as names or icons, which side the sidebar sits on, the bookmarks column, vitals, compact mode); and how it behaves (reconnecting, "Needs you" alerts, highlighting, the logo). A live preview of the app shows every choice as you make it. Skip any step, or the whole thing.

### What's new, right in the app
- **After an update, SSHeesh shows what changed**, including every version you skipped. See it again any time in Settings → About, or search for "What's new".

## 0.3.2 — friendlier and tidier

### A proper hello
- **A short welcome on first launch.** Pick a theme and an accent, choose which servers from `~/.ssh/config` to bring along, and three tips worth knowing. Every step can be skipped, and everything in it can be changed later in Settings.

### Fits any size
- **Pane title bars keep their Close button**, however many panes you open. In narrow panes the other buttons fold into a ⋯ menu, and labels shorten before buttons disappear.

### A tidier file manager
- **Take places out of the column.** Hold a place, bookmark or recent folder and drag it out of the column to remove it. Right-click the column to tick which places show, or bring them all back.
- **A New menu** (the + button): a new file, a new folder, or an archive of what you selected (or of everything here) as a `.tar.gz`. Right-click any file or folder to archive it too.
- **A toolbar that fits.** In a narrow file manager the address bar keeps its room; the filter, bookmark, upload and refresh buttons step aside into the ⋯ menu, which always has them all.
- "Show hidden files" moved from the toolbar to the ⋯ menu and the right-click menu.

### Also
- **The logo moved to the bottom of the sidebar**, bigger, with the version next to it. On a Mac it no longer sits under the window buttons. Click it for About.
- **A calmer top bar.** The layouts and vitals buttons are gone: layouts live in Settings and the command palette, and the vitals bar has its own close button (⌘J or Ctrl+Shift+J brings it back).

## 0.3.0 — make it yours

A big one. SSHeesh is about a terminal you can shape to the way you work, and this release adds a lot of that, plus a much friendlier way to add servers, and guard rails for the servers that matter.

### Make it yours
- **Server icons and colors.** Pick an icon and a color for every server, when you add it or with a right-click.
- **A narrow sidebar.** Drag the sidebar's edge in and the server list folds into a rail of icons. Drag it out again for names.
- **Compact mode** (⇧⌘M or Ctrl+Shift+M). The sidebar, toolbar and vitals step aside; point at the window's edge and they slide back in.
- **Light by day, dark by night.** Let the theme follow your system, with a light and a dark theme of your choice.
- **Interface size**, separate from the terminal's font size.
- **Fade the panes you're not using**, so it's obvious where you're typing.
- **Search in Settings.** Type "font" or "reconnect" and jump straight to it.
- **A logo that winks.** The `>_<` mark now sits in the sidebar and on empty panes, and blinks at you now and then. Prefer it still, or gone? Settings → Appearance.
- Flip through themes with the arrow keys, and turn animations off if you like things calm.

### Easier every day
- **A new Add a server window.** Paste anything: `root@1.2.3.4:2222` or a whole `ssh -p 2222 -i key user@host` command, and the fields fill in. Your keys in `~/.ssh` and key managers like 1Password and Bitwarden are listed for you.
- **Test before you add.** One button checks that the server answers, shows its fingerprint the first time, signs in, and tells you whether `tmux` is installed (with the command to install it if not).
- **Passwords.** Sign in with a password, and choose whether SSHeesh remembers it. Remembered passwords live in your system keychain (Keychain, Credential Manager or Secret Service), never in SSHeesh's own files.
- **Highlighting in terminal output.** Errors in red, warnings in yellow, and your own words or regular expressions in the color you pick.
- **"Needs you" alerts.** When an AI coding agent or any program in a terminal waits for an answer, or a long run goes quiet, the pane and the sidebar say so, and you get a system notification if SSHeesh is in the background.
- **While you were away.** After a dropped connection, a card shows what the server printed in the meantime, with errors counted and marked.
- **You choose which ~/.ssh/config servers to add.** On first launch SSHeesh asks instead of adding all of them, and new ones later get a quiet note. Change your mind in Settings → Layout.
- **Remove or hide servers.** Right-click a server you added to remove it, or one from `~/.ssh/config` to hide it.
- **A portable Windows app.** One `.exe` to run without installing. Your servers and settings stay in `%APPDATA%` either way.
- **The window comes back the way you left it**, size and place, and folded panels unfold to the width they had.
- **Open in main terminal.** Right-click a folder in the file manager to `cd` the terminal you're already using there.
- New terminals opened next to files or stats go on the left.
- The bookmarks column in the file manager is narrow by default and looks cleaner.
- Empty spots explain what to do next, like how to install Docker on a server that doesn't have it.

### Safer
- **Prod, Staging and Dev labels.** A colored tag and frame, everywhere the server shows up.
- **A clear question for new servers.** The first connection shows the server's fingerprint in a card instead of ssh's yes/no prompt. If a server's identity ever changes, a red warning shows the old and new fingerprints and doesn't connect unless you say so.
- **Streamer mode** (⇧⌘H or Ctrl+Shift+H). Hides server names, addresses, IPs and user names everywhere, terminal output included, for streams and screen shares.
- **Privacy page** in Settings: everything SSHeesh connects to, which is your servers and GitHub for updates. No account, no analytics.
- When the connection drops, SSHeesh now asks before reconnecting by default. Settings → Terminal switches it back to automatic.

### Coming next
- 0.3.2 tidies up the file manager and adds a first-launch welcome. See the [roadmap](README.md#roadmap) for what follows.

### Fixes
- The sidebar and the bookmarks column can be dragged narrower again; the drag used to get stuck.
- The macOS install guide covers macOS 15 and newer, where right-click → Open no longer works, and there's a one-line Terminal installer that skips the security prompt.

## 0.1.0 — first preview

The first public build. It's early and rough: expect bugs, and please [report them](https://github.com/footprintsntrails/ssheesh/issues/new/choose).

### Terminals
- Sessions survive network drops. On servers with `tmux`, reconnecting lands you back in the same shell with everything still running.
- Choose what happens when the connection drops: reconnect automatically, show a Reconnect button, or stay quiet.
- The status face tells you how a session is doing: `>_` connected, `>_<` reconnecting, `x_x` couldn't connect.
- Nine color schemes, font, size, line height, cursor and padding.

### Containers
- Expand a server in the sidebar to see its Docker and Podman containers, grouped by Compose project.
- Open a shell inside a container, follow its logs, start, stop or restart it.

### Files
- Browse a server's files with an address bar you can type into, places, bookmarks and recent folders.
- Preview and edit text files, upload by dragging files in, download files and whole folders.

### Stats
- A one-line vitals bar (CPU, memory, network, disk) and a detailed view with load, traffic and every disk. Nothing to install on the server.

### Workspace
- Split panes, drag them around by their title bar, resize any edge, fill the window with one pane.
- Command palette, keyboard shortcuts, sidebar on either side.
- Four app themes, accent colors, spacing and corner settings.
- Updates install themselves; you choose automatic, ask first, or never.
