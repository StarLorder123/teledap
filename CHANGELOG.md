# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- Initial implementation of TeleDAP MCP debug server
- `auto_launch` now supports `mode` parameter (`"local"` / `"remote"`) for host binary debugging without hardware

### Fixed

- Fix `send_request` hanging on `launch` due to matching `msg.seq` (adapter-assigned) instead of `msg.request_seq` (original request seq). DAP responses carry independent sequence counters; events interleaved between responses caused seq divergence and indefinite wait.

[0.1.0]: https://github.com/StarLorder123/teledap/releases/tag/v0.1.0
