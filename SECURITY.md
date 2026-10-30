# Security

SSHeesh holds the keys to people's servers, so security reports get priority.

## Reporting a problem

Please don't open a public issue for a vulnerability. Use GitHub's private reporting instead: **Security → Report a vulnerability** on this repository. Include what you found, how to reproduce it, and which version you used (Settings → About).

You'll get a reply within a few days.

## How SSHeesh handles your data

- Connections go through your system's `ssh`. SSHeesh doesn't store your private keys or passwords.
- The server list you add in the app is saved on your computer only.
- There's no telemetry and no account.
- Updates are signed, and the app refuses an update that isn't signed with the project's key.

## Supported versions

Only the latest release gets fixes.
