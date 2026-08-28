# macOS Gatekeeper and GitHub releases

Wikitool's macOS binaries are distributed as unsigned GitHub release assets, not as an
Apple-sanctioned application. Every macOS archive therefore contains `macos-release-trust.json`
with status `unsigned_github_release`. Gatekeeper will not silently trust those executables; a
checksum-bound, user-approved quarantine exception is required.

Gatekeeper can block a program before its first instruction runs, so Wikitool cannot repair its own
quarantine state. A packaged script must not attempt to weaken that boundary. The release checksum
proves that the downloaded bytes match the GitHub release; it does not supply an Apple identity.

## Start with external provenance

Obtain both the ZIP and `SHA256SUMS.txt` from the same GitHub release, then verify the archive before
extracting or approving an exception:

Compare `shasum -a 256 /path/to/wikitool-<version>-macos-<arch>.zip` with the exact matching line
in `SHA256SUMS.txt`.

Do not treat a checksum copied from inside the downloaded ZIP as independent evidence. After
extraction, inspect `macos-release-trust.json`.

After confirming the hash, extracting the archive, and obtaining the user's approval for those
exact bytes, an agent may remove quarantine only from each executable the user intends to run:

```bash
xattr -d com.apple.quarantine /exact/path/to/wikitool
xattr -d com.apple.quarantine /exact/path/to/contextmink/contextmink
xattr -d com.apple.quarantine /exact/path/to/papertiger/papertiger
# Only when the optional experimental campaign runner will be used:
xattr -d com.apple.quarantine /exact/path/to/papertiger/papertiger-mise
```

Do not use recursive `xattr -dr` against a download folder, disable Gatekeeper globally, or silently
apply the exception to future releases. Each new archive has different bytes and requires a new
checksum verification and decision.
