# macOS Gatekeeper and GitHub releases

A GitHub release is distribution outside the Mac App Store. Apple permits that software to be
Developer ID-signed and notarized, but doing so requires Apple Developer credentials. Wikitool's
release workflow therefore supports two explicit modes and places `macos-release-trust.json` in
every macOS archive:

- `developer_id_notarized` means every Mach-O in the archive was signed, the exact ZIP received an
  accepted Apple notary result, and CI passed code-signature and Gatekeeper assessment.
- `unsigned_github_release` means no Apple identity was available. Gatekeeper will not silently
  trust those executables; a checksum-bound, user-approved quarantine exception is required.

Gatekeeper can block a program before its first instruction runs, so Wikitool cannot repair its own
quarantine state. Ad-hoc signing, a self-authored installer, Homebrew metadata, or GitHub provenance
can improve integrity evidence but cannot make an unsigned executable Developer ID-trusted.

## Start with external provenance

Obtain both the ZIP and `SHA256SUMS.txt` from the same GitHub release, then verify the archive before
extracting or approving an exception:

```bash
shasum -a 256 -c SHA256SUMS.txt
```

Do not treat a checksum copied from inside the downloaded ZIP as independent evidence. After
extraction, inspect `macos-release-trust.json`.

## Notarized archive

For `developer_id_notarized`, inspect both native executables:

```bash
codesign --verify --strict --verbose=4 ./wikitool
spctl --assess --type execute --verbose=4 ./wikitool
codesign --verify --strict --verbose=4 ./contextmink/contextmink
spctl --assess --type execute --verbose=4 ./contextmink/contextmink
```

A failure is a release defect. Preserve the archive hash and assessment output, then obtain a
corrected release rather than weakening Gatekeeper. Apple does not support stapling a ticket
directly to a ZIP archive or standalone command-line binary; Gatekeeper resolves their notarization
tickets online.

## Unsigned GitHub archive

For `unsigned_github_release`, an agent must explain the loss of Apple identity verification and
obtain the user's approval for the exact checksum-verified archive. It may then remove quarantine
from only the two shipped executable paths:

```bash
xattr -d com.apple.quarantine /exact/path/to/wikitool
xattr -d com.apple.quarantine /exact/path/to/contextmink/contextmink
```

Do not use recursive `xattr -dr` against a download folder, disable Gatekeeper globally, or silently
apply the exception to future releases. Each new archive has different bytes and requires a new
checksum verification and decision.

Apple's primary references are [Signing Mac Software with Developer
ID](https://developer.apple.com/developer-id/), [Customizing the notarization
workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow),
and [Creating distribution-signed code for
macOS](https://developer.apple.com/documentation/xcode/creating-distribution-signed-code-for-the-mac).
