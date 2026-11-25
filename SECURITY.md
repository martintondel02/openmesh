# Security Policy

## Supported versions

| Version | Supported |
| --- | --- |
| 0.1.x (alpha) | ✅ best effort |

OpenMesh is **alpha software and has not been independently audited**. The protocol design and
threat model are documented in [`docs/architecture/PROTOCOL.md`](docs/architecture/PROTOCOL.md)
§17 and [`docs/architecture/ARCHITECTURE.md`](docs/architecture/ARCHITECTURE.md) §6.

## Reporting a vulnerability

Please **do not open a public issue**. Use GitHub's private vulnerability reporting:
<https://github.com/martintondel02/openmesh/security/advisories/new>

Include: affected component/version, reproduction steps, impact, and any suggested fix. We aim
to acknowledge within 72 hours. Coordinated disclosure is appreciated; credit is given unless you
prefer otherwise.

## Known limitations (by design, tracked for OMP/2)

* No forward secrecy for direct messages yet (static ECDH).
* No traffic-analysis resistance; relays see pseudonymous endpoints and timing.
* Companion links (USB/BLE) are not mutually authenticated — physical access is the trust
  boundary.
* No key revocation or group membership revocation.
* Repeaters can drop, delay or replay envelopes (they cannot read or forge them).
