# Vendored Dependencies

`vendor/contextmink` is a source snapshot of the public Contextmink repository.
Wikitool release artifacts build this source for each release target so public
downloads already contain the matching Contextmink binaries.

- Upstream: https://github.com/remiliacorporation/contextmink
- Current tag: `v0.6.0`
- Current commit: `64b98cfe6e421b05fa87b3c472e5df20d929612c`
- Version pin: `config/contextmink.version`

Refresh procedure:

1. Check out the desired upstream Contextmink tag in a clean worktree.
2. Replace `vendor/contextmink` with that source tree, excluding `.git/` and
   build output such as `target/`.
3. Update `config/contextmink.version` to the same release version.
4. Verify `cargo pkgid --manifest-path vendor/contextmink/Cargo.toml` reports
   that version.
5. Run `cargo test --manifest-path vendor/contextmink/Cargo.toml --locked`.
6. Run `cargo test --workspace` and
   `cargo test -p wikitool --features maintainer`.
7. Build and dogfood at least the Windows release bundle, because it carries
   both `contextmink.exe` and `contextmink-bridge.exe`.

Do not edit vendored Contextmink files as ordinary wikitool changes. Make
Contextmink changes upstream first, tag a Contextmink release, then refresh this
snapshot.
