# `secrets` — Secret file detection

Concerns managed in this module:

- **External tool integration** — wraps the `betterleaks` secret scanner
- **JSON output parsing** — deserialises betterleaks findings
- **Deduplication & sorting** — normalises the report
- **Scanning API** — scan a single directory or a set of paths

## Files

  | File                     | Concern                                                    |
  | ------------------------ | ---------------------------------------------------------- |
  | [`mod.rs.md`](mod.rs.md) | Secret scanning, betterleaks wrapper, `BetterleaksFinding` |
