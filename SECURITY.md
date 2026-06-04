# Security Policy

## Supported Use

SP42 is currently an alpha project under active development. The repository is public, but production deployment guidance and long-term support guarantees are not in place yet.

## Reporting a Vulnerability

Please do not open a public issue for sensitive security problems.

Use GitHub private vulnerability reporting for this repository:

https://github.com/schiste/SP42/security/advisories/new

If that channel is unavailable, contact a project maintainer privately before
disclosing details publicly.

## Sensitive Material

Never publish:

- Wikimedia access tokens
- local `.env.wikimedia.local` contents
- personal developer credentials
- raw authentication headers or cookies

## Scope Notes

The local single-user auth bridge is intended for development use only. It should not be treated as a production authentication system.

Production deployments, release signing, and Wikimedia Cloud VPS access are
maintainer-controlled. Do not request or share secrets in public issues or pull
requests.
