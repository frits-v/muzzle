# Security Policy

## Supported Versions

| Version | Supported          |
|---------|--------------------|
| 0.2.x   | :white_check_mark: |
| < 0.2   | :x:                |

## Reporting a Vulnerability

**Do not open a public issue for security vulnerabilities.**

Instead, use [GitHub Security Advisories](https://github.com/frits-v/muzzle/security/advisories/new)
to report vulnerabilities privately. This ensures the issue is triaged and fixed
before public disclosure.

### What to include

- Description of the vulnerability
- Steps to reproduce (or a proof-of-concept)
- Affected versions
- Potential impact

### Response timeline

- **Acknowledgement**: within 48 hours
- **Initial assessment**: within 7 days
- **Fix or mitigation**: best effort, typically within 30 days

### Scope

Muzzle enforces workspace sandboxing and git safety for AI coding sessions.
Security-relevant findings include:

- Sandbox escapes (path traversal, symlink following, worktree breakouts)
- Git safety bypasses (force-push, main branch writes, tag deletion)
- Panic-induced fail-open conditions (hooks must deny on crash)
- Session isolation failures (cross-session state leaks)

### Recognition

Contributors who report valid security issues will be credited in the
release notes (unless they prefer to remain anonymous).
