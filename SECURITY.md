# Security Policy

## Reporting a vulnerability

Please do not open a public issue for a security problem. Report it privately through GitHub's
"Report a vulnerability" button on the repository's Security tab. Include the version, the steps to reproduce, and
what an attacker could do with it.

You can expect an acknowledgement within a few days. We will keep you updated while we work on a fix, and credit you
in the release notes if you want.

## Scope

olsc runs local servers and edits system files (the hosts file, the certificate trust store). Reports
about the following are especially welcome:

- The elevated helper accepting commands it should refuse.
- Local certificate authority key handling.
- Downloads that skip checksum verification.
- Quick Apps or Quick Commands running something the user did not approve.
- Secrets appearing in logs, configs or exports.

## Windows Defender exclusions (2.12.0)

Settings → Performance can ask the elevated helper to exclude folders from Windows Defender's real-time scan, which
makes `npm install` and `composer install` faster. The helper has two commands for this, `defender-exclude-add` and
`defender-exclude-remove`, each taking exactly one folder. The helper refuses anything that is not an existing folder on
a local drive, anything with characters that could change the PowerShell command, `.`/`..` parts, drive roots, system
folders (Windows, Program Files, ProgramData), user profile roots and their `AppData`. OLS itself only adds folders it
manages (project root folders, the sites folder, its data folder), only after a click in the app, and only removes
exclusions it added. An excluded folder is not scanned in real time, so malware that lands there is not caught: only
exclude folders with code you trust. Reports of the helper accepting a path it should refuse are in scope.

## Pulling a server database (2.12.0)

Project -> Backups -> Pull from a server copies a staging or production database over SSH. OLS supports SSH key login only, keeps the database password in the Windows credential store (never in settings, logs, task output or a command line; it is written to the remote shell's stdin), validates every profile field before starting `ssh`, and imports the dump into a temporary database first, so your local database is only replaced after the data (and the optional anonymizing rules) were applied. The downloaded dump is deleted as soon as it is imported. A server database usually holds real people's data: use anonymizing whenever you can; without it you are copying personal data onto your computer and you are responsible for protecting it. Server profiles and pulls are deliberately not available through the HTTP API or MCP.

## Open a site on your phone (2.12.0)

Site settings -> Open on my phone gives a site a second name under sslip.io (a public DNS service that answers with your computer's LAN address) and can serve OLS's local certificate authority to a phone. The certificate page listens only on your private LAN address, on a random port, behind a random 128-bit token, for at most 10 minutes or 50 requests, and serves only the public certificate and an unsigned iOS profile; the CA's private key is never read by that code. Installing the certificate makes the phone trust every certificate OLS issues, so keep the computer safe and remove it from the phone after testing. OLS does not change the Windows firewall. These features are not available through the HTTP API or MCP.
