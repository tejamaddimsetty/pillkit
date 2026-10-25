# Security

barkit draws inside the operating system's own chrome, so we treat security
reports as the highest priority.

## Reporting

Email security@barkit.dev (codename address; update before launch) with steps to
reproduce. Please don't open a public issue. We aim to reply within 2 working
days and to fix critical issues within 14 days.

## What's in scope

- Getting a host to show content that wasn't signed by the server
- Getting a host to open a URL outside the app's allowlist
- One app reading or controlling another app's items through the local socket
- Licence or feed forgery, key leakage, privilege escalation by the host
- Portal issues: auth bypass, CSRF, cross-account access

## Known limits in v0.1

- The Windows named pipe uses the default security descriptor (same user,
  administrators and SYSTEM have full access; others read-only). v0.2 will set
  an explicit per-user DACL.
- Any process running as the same user can talk to the host, as with any
  per-user service. It still can't show anything without a valid licence for
  the app it registers as, and it can't bypass the URL allowlist.
- The portal has no login rate limiting yet; put it behind a proxy that does.
