# macOS Gatekeeper and release trust

Official macOS release archives must be signed with a Developer ID Application certificate and
accepted by Apple's notary service. The release workflow signs every Mach-O executable in the
bundle, including separately versioned companion tools such as Contextmink, before it recreates and
submits the exact ZIP that is published. A macOS release fails closed when signing credentials are
not configured or Apple does not accept the submission.

Gatekeeper can block a program before its first instruction runs. Wikitool therefore cannot safely
remove its own quarantine attribute or otherwise repair an unsigned release at runtime. Apple also
does not support stapling a ticket directly to a ZIP archive or standalone command-line binary;
Gatekeeper resolves the notarization tickets for the signed files online.

## Verify an official archive

First verify the downloaded ZIP against the release's `SHA256SUMS.txt`. After extraction, inspect
both native executables:

```bash
codesign --verify --strict --verbose=4 ./wikitool
spctl --assess --type execute --verbose=4 ./wikitool
codesign --verify --strict --verbose=4 ./contextmink/contextmink
spctl --assess --type execute --verbose=4 ./contextmink/contextmink
```

An official archive that fails either check is a release defect. Preserve the archive hash and
assessment output, then obtain a corrected release; do not teach an agent to conceal the failure.

## Explicit development-build exception

For an intentionally unsigned local or development build, an agent may remove quarantine only
after the user identifies the exact artifact and authorizes the exception. Verify its checksum or
build provenance first, then target only the executable that was approved:

```bash
xattr -d com.apple.quarantine /exact/path/to/wikitool
xattr -d com.apple.quarantine /exact/path/to/contextmink/contextmink
```

Do not use recursive `xattr -dr` against a download folder, disable Gatekeeper globally, or treat
quarantine removal as the installation path for an official release.

Apple's current primary references are [Signing Mac Software with Developer
ID](https://developer.apple.com/developer-id/), [Customizing the notarization
workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow),
and [Creating distribution-signed code for
macOS](https://developer.apple.com/documentation/xcode/creating-distribution-signed-code-for-the-mac).
