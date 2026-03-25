# Security Policy

## Supported Use

SP42 is currently an alpha project under active development. The repository is public, but production deployment guidance and long-term support guarantees are not in place yet.

## Reporting a Vulnerability

Please do not open a public issue for sensitive security problems.

Instead, report security concerns privately to the project maintainer through an agreed private channel. If no dedicated channel exists yet, contact the maintainer directly before disclosing details publicly.

## Sensitive Material

Never publish:

- Wikimedia access tokens
- local `.env.wikimedia.local` contents
- personal developer credentials
- raw authentication headers or cookies

## Scope Notes

The local single-user auth bridge is intended for development use only. It should not be treated as a production authentication system.
