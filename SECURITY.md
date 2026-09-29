# Security Policy

## Reporting a vulnerability

Please do not open a public issue for a security problem. Report it privately through GitHub's
"Report a vulnerability" button on the repository's Security tab. Include the version, the steps to reproduce, and
what an attacker could do with it.

You can expect an acknowledgement within a few days. We will keep you updated while we work on a fix, and credit you
in the release notes if you want.

## Scope

OLS runs local servers and edits system files (the hosts file, the certificate trust store). Reports
about the following are especially welcome:

- The elevated helper accepting commands it should refuse.
- Local certificate authority key handling.
- Downloads that skip checksum verification.
- Quick Apps or Quick Commands running something the user did not approve.
- Secrets appearing in logs, configs or exports.
