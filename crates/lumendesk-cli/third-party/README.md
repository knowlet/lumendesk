# Reviewed redistribution notice supplements

These files preserve third-party licensing material omitted from specific
published Cargo packages or missed by the original packager. They are not
LumenDesk-authored license grants. Review date: 2026-10-19; PR baseline:
`a90f4c2d247c8b0cedd337ac911c12903f6faca8`.

`overrides.json` records the exact package name, version, registry identifier,
declared license expression, published `.crate` SHA-256, original VCS revision
where available, notice text SHA-256 and primary source URL. `license_overrides.rs`
only returns an entry when that complete identity matches. Updating a crate or
changing its source requires a fresh review; a name-only or license-ID fallback
is deliberately not supported. No notice is fetched from the network while
packaging.

## Scope

The first 31 entries correspond to the empty notice arrays reproduced in a
275-dependency, all-target web scaffold inventory. A separate 32nd entry covers
`epaint_default_fonts 0.36.2`, an additional native-only finding. These counts do
not describe the linked code in one binary or the complete SDK's licensing
coverage. See `docs/REDISTRIBUTION.md` for target reachability, obligations and
remaining release review.

The original `.crate` archives were obtained from Cargo's registry cache and
hashed against the locked package checksums. Most omitted license files were
recovered from the exact upstream commit recorded in the published
`.cargo_vcs_info.json`. Older cesu8 and winapi target packages have no VCS record;
their published source headers explicitly offer Apache-2.0 or MIT. Their original
attribution headers are preserved and the Apache-2.0 alternative's complete
standard text is included. `selectors` declares MPL-2.0 in its versioned manifest
and source headers; its missing complete license was recovered from Mozilla's
version-2.0 text.

## Deliberately unresolved upstream statements

The pinned objc2 workspace `LICENSE.md` identifies possible Apple SDK-derived
binding redistribution implications. Its four MIT-only crates do not supply a
complete copyright-specific MIT notice. The supplemental
`LICENSE-MIT-STANDARD.txt` is SPDX's unmodified license text, including its literal
placeholders. It is reference material, **not an invented attribution or proof of
compliance**. Both issues stay in the generated dependency review fields for
affected selected packages. A non-empty notice list must not hide them.

## Components not represented by the crate's simple license identifier

`webview2-com-sys 0.39.1` carries Microsoft WebView2 SDK loader binaries in
addition to MIT-licensed Rust bindings. The exact upstream generator pins SDK
`1.0.3800.47`. All nine bundled files (three architectures; DLL, import library
and static library) were compared byte-for-byte with that official NuGet
archive. Its Microsoft BSD-style `LICENSE.txt` and third-party `NOTICE.txt` are
preserved in full. This is the SDK loader's licensing material; it does not
cover redistribution of a separately installed WebView2 Runtime.

The font crate embeds Ubuntu Light, Noto Emoji, Hack and emoji-icon-font. All
four accompanying notice files are preserved. A separate font attribution
notice preserves the exact copyright/license records from the font name tables;
its narrative labels are newly authored. Hack's own notice includes Bitstream
Vera terms in addition to MIT and public-domain attribution. The original font
bytes were not changed. A single crate SPDX expression does not replace these
file-specific notices.

## Updating these records

1. Resolve the exact target/features and retain the application lockfile.
2. Verify the published package archive against its locked checksum; compare
   source material to the archive before claiming a source URL represents the
   build inputs.
3. Obtain licensing files from the package itself or its exact recorded upstream
   revision. Preserve the bytes, notices and any unresolved upstream qualifications.
4. Record file and archive hashes and primary URLs in `overrides.json`, update the
   corresponding exact lookup entry, and run the digest and identity-boundary
   tests.
5. Inspect generated archives and their unresolved review fields. Source
   availability, actual linked/system components, assets, license choices and
   release approval still need review for the particular distribution.

No original toolkit implementation, tests, examples or assets were consulted to
produce these records. Only this repository and independent dependency and
license-owner sources were used.
