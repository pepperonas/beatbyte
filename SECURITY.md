# Security Policy

## Supported Versions

BeatByte is in early development. Only the latest release receives security
fixes.

## Threat Model

BeatByte processes **untrusted input**: imported audio files and chart
files. The project treats the following as security-relevant:

- Chart parsing (JSON): must reject malformed input with useful errors,
  never crash, never follow path traversal in referenced files.
- Audio decoding: handled by well-maintained Rust decoders; decoder
  crashes on malformed files are treated as bugs.
- Save/config files: validated on load; corrupt files must not brick the
  game.

BeatByte never executes imported files and has no networking in the
shipped game.

## Reporting a Vulnerability

Please report vulnerabilities privately via
[GitHub Security Advisories](https://github.com/pepperonas/beatbyte/security/advisories/new)
rather than public issues. You can expect an initial response within a
week. Please include reproduction steps and, for malformed-file issues,
the offending file if possible.
