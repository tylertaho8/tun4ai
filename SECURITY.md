# Security

tun4ai installs a root launchd service and grants passwordless `sudo` for four helper scripts, so bugs here can matter.

## Reporting

Please report vulnerabilities privately through [GitHub Security Advisories](https://github.com/tylertaho8/tun4ai/security/advisories/new), not in public issues. Include the macOS version, the tun4ai commit and steps to reproduce. Expect a first reply within a week.

## In scope

- Anything that lets a non-admin user or another local process gain root through the helper, the sudoers entry or the launchd service.
- The root helper reading, following or executing user-controlled paths.
- Traffic or DNS leaving outside the tunnel while the firewall is on.
- The node UUID ending up somewhere readable by other users.

## Out of scope

- Vulnerabilities in sing-box itself; report those to [SagerNet/sing-box](https://github.com/SagerNet/sing-box).
- UDP and IPv6 not being tunneled; these are documented limitations.
- Attacks that already require root or an administrator password.
