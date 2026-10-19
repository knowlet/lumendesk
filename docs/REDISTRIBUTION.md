# Redistribution review

Review date: 2026-10-19. Baseline: PR #1 commit
`a90f4c2d247c8b0cedd337ac911c12903f6faca8`.

This is an engineering review of the archive contents and their declared
third-party terms. It is not a grant of rights from dependencies' authors or a
legal clearance. Maintainer approval of the PR and approval of a particular
release remain separate decisions.

## What is actually distributed

The committed `crates/lumendesk-cli/verification/package-archive-metadata.json`
describes a **web-mode** generated application, once packaged as source and once
as a Linux binary. Both historical records report 275 dependency entries and 31
entries with no copied notice files. These are not counts for a native build,
the complete native-plus-web SDK, or every supported platform.

The historical archive files, their `DEPENDENCIES.json` files, and the generated
application's `Cargo.lock` are ignored build outputs, not files committed with
that record. The aggregate count cannot establish the names, versions, licensing
decisions, or exact contents of those historical archives on its own. A newly
generated inventory must be identified as new evidence and must not replace the
historical checksums without regenerating and inspecting the actual artifacts.

| Output | Material copied by the baseline packager | Review consequence |
|---|---|---|
| Source archive | Application files; local LumenDesk/core Rust sources; SDK assets and license texts; registry dependency metadata and selected notice files. Registry sources are not vendored. | A registry dependency reference is not itself redistribution of all of that registry crate's source. The SDK files actually copied still need their own notices. Any separately vendored source bundle requires its own complete review. |
| Binary archive | Release executable and selected application/dependency notices. Cargo code and embedded data may be linked into that executable. | Obligations depend on code/data actually distributed, including generated code and statically linked libraries. A Cargo graph is useful evidence but is not a list of linked object files. |
| Linux runtime prerequisites | The recorded executable dynamically needs WebKitGTK, JavaScriptCoreGTK, GTK/GDK, GLib/GIO/GObject, Cairo, GdkPixbuf, libsoup, D-Bus, libc and libgcc. These shared libraries are not bundled. | Installing system libraries separately avoids redistributing their files in this archive; it does not establish that the application's obligations for linking to a library have been met. |
| Other platforms/features | The metadata graph includes target-specific dependencies; the native renderer activates additional crates and fonts. | Review the exact feature/target build. Linux evidence cannot clear macOS frameworks, the Windows WebView2 loader/runtime, or native font assets. |

## Obligations that a notice count does not prove

An empty `notices` array is a **review finding**, not proof that a dependency has
no license or that infringement occurred. Conversely, one copied `LICENSE` file
does not prove that all third-party notices, embedded data/font terms, or
source-availability requirements have been satisfied.

| Declared terms | Engineering requirement when the covered material is redistributed | Primary terms |
|---|---|---|
| MIT | Preserve the actual copyright and permission notice with copies or substantial portions; a string containing `MIT` is not a substitute. | [SPDX MIT text](https://spdx.org/licenses/MIT.html) |
| Apache-2.0 | Supply the license; retain applicable notices; identify changed files where required; if the covered upstream distribution includes a NOTICE, preserve its relevant attribution content in a permitted location. Apache does not require inventing a NOTICE that upstream never supplied. | [Apache License 2.0, section 4](https://www.apache.org/licenses/LICENSE-2.0) |
| BSD-2-Clause / BSD-3-Clause | Retain the applicable copyright, conditions and disclaimer, including in accompanying materials for binary distributions; honor the additional non-endorsement clause where present. | [BSD-2-Clause](https://spdx.org/licenses/BSD-2-Clause.html), [BSD-3-Clause](https://spdx.org/licenses/BSD-3-Clause.html) |
| Unicode-3.0 | Include the copyright and permission notice with the covered data/software or associated documentation. Data tables can have their own obligations even when the surrounding code is permissive. | [Unicode License v3](https://spdx.org/licenses/Unicode-3.0.html) |
| MPL-2.0 | Preserve covered-source notices and make covered source available under MPL. For an executable, explicitly tell recipients how to obtain that source, including when the upstream source was unmodified. MPL does not require relicensing unrelated, separately authored application files. | [Mozilla MPL sections 3.1–3.4](https://www.mozilla.org/en-US/MPL/2.0/), [Mozilla FAQ Q8 and Q11](https://www.mozilla.org/en-US/MPL/2.0/FAQ/) |
| `A OR B` | Select and meet an available alternative, checking any separately licensed included material; do not read `OR` as requiring every alternative. | The applicable versioned upstream license and included notices. |
| `A AND B` / custom terms | Meet all applicable terms or resolve the expression/file-specific terms before release. | The applicable versioned upstream licenses and included notices. |

The baseline inventory records a generic registry identifier, not a versioned
source-availability notice. In particular, a linked MPL dependency is not cleared
merely by including its license text. A release needs an explicit path or URL to
the exact covered source, including any local changes, plus preservation of its
terms. The presence of a public upstream repository is not an adequate substitute
for telling recipients where to obtain the distributed version's covered source.

## Independent reconstruction of the 275 / 31 inventory

A new standalone web application was resolved with Cargo 1.99.0, using
`default-features = false, features = ["webview"]`, seeded with the baseline
workspace lockfile and resolved **offline**. This reproduced **275 dependency
entries and exactly 31 empty notice arrays** under the baseline filename rules.
The saved [reconstruction lockfile](../crates/lumendesk-cli/verification/redistribution-web.Cargo.lock)
and [audit JSON](../crates/lumendesk-cli/verification/redistribution-audit.json)
record the identities, expected archive hashes, baseline notice paths and target
reachability. This is a new inspection of version-pinned packages; it does not
claim to reproduce the historical `.tar.gz` files' SHA-256 values.

Target-specific `cargo metadata --filter-platform` graphs were traversed from
the application through normal/build edges. These include proc-macros and build
helpers; they are a conservative build-input set, not proof that each crate's
object code is present after linking.

| Web target | Normal/build dependencies | Baseline empty-notice entries in that graph | Reachable MPL-only packages |
|---|---:|---|---|
| `x86_64-unknown-linux-gnu` | 145 | `dlopen2 0.8.2`, `dlopen2_derive 0.4.3` | `option-ext 0.2.0` |
| `aarch64-apple-darwin` | 87 | Nine objc2-family entries, identified below | `option-ext 0.2.0` |
| `x86_64-pc-windows-msvc` | 96 | `webview2-com 0.39.1`, `webview2-com-macros 0.8.1`, `webview2-com-sys 0.39.1` | None |

In particular, the all-target graph's `selectors`, `cssparser`,
`cssparser-macros` and `dtoa-short` are **not reachable in these three desktop web
build graphs**. The Windows GNU import-library crates are not evidence of code
in the MSVC build. Apple bindings and the Microsoft loader are not reasons to
reject a Linux archive in which they are not distributed.

## Each of the 31 empty-notice entries

“Other graph entry” means absent from the three desktop graphs above; it is not
an assertion that the crate is unused on every possible Cargo target. License
requirements apply when its covered material is actually distributed. The
source archive does not vendor any of these registry crates.

The following remedies preserve the original terms; they do not relicense the
upstream works:

- **O-MIT:** Preserve the exact objc2 workspace license explanation and the
  unmodified linked MIT standard text. **Still unresolved:** no authoritative
  copyright-specific notice is provided; the standard text's literal
  placeholders are not an invented attribution. Review the upstream Apple SDK
  qualification for the target build.
- **O-APACHE:** The crate offers three alternatives. Supply the complete
  Apache-2.0 text and preserve the exact upstream objc2 qualification. Review
  applicable Apple SDK terms; the license identifier alone does not resolve the
  qualification.
- **CESU:** The published headers explicitly offer Apache-2.0 or MIT despite the
  legacy slash expression. Supply Apache-2.0, preserve the authors' original
  attribution headers and the already-published `COPYRIGHT-RUST.txt`. The latter
  is a broad historical Rust notice, not proof that every library it mentions is
  present in this small crate.
- **MIT-FILE:** Copy the actual upstream copyright and full MIT permission text
  from the published archive's exact recorded upstream commit.
- **JNI:** Preserve both original MIT/Apache alternatives from the exact
  upstream commit; selecting one alternative is sufficient for the crate code.
- **NDK:** Supply the Apache-2.0 alternative from the exact upstream commit.
  This covers the reviewed Rust crate grant, not a blanket grant to redistribute
  all Android SDK/NDK files.
- **MPL:** Supply Mozilla's complete MPL 2.0 text. If covered code is included in
  a binary, include precise source-availability instructions. The same obligation
  also applies to reachable `option-ext`, whose existing LICENSE was not missing.
- **WEBVIEW-RUST:** Copy the actual upstream MIT notice. For `webview2-com-sys`,
  also include **Microsoft SDK LICENSE and NOTICE**, described below.
- **WINAPI:** The archive's original source headers explicitly offer
  Apache-2.0 or MIT. Supply Apache-2.0 and preserve the original headers. These
  are Windows GNU import-library crates, outside the tested MSVC graph.

| Dependency | Exact version | Declared license | Desktop web graph | Required notice/remedy and primary evidence |
|---|---|---|---|---|
| `block2` | `0.6.2` | `MIT` | macOS | **O-MIT**; [versioned source](https://github.com/madsmtm/objc2/blob/b4167b582b2f75f9a1be75495c41b765344fd03c/LICENSE.md) |
| `cesu8` | `1.1.0` | `Apache-2.0/MIT` | Other graph entry | **CESU**; [versioned source](https://static.crates.io/crates/cesu8/cesu8-1.1.0.crate) |
| `dispatch2` | `0.3.1` | `Zlib OR Apache-2.0 OR MIT` | macOS | **O-APACHE**; [versioned source](https://github.com/madsmtm/objc2/blob/8852b424193ca41602281b3d7540d7c8ed51e49a/LICENSE.md) |
| `dlopen2` | `0.8.2` | `MIT` | Linux | **MIT-FILE**; [versioned source](https://raw.githubusercontent.com/OpenByteDev/dlopen2/cc80e4a0a90d499b677fdf7743699b4b3a43a989/LICENSE) |
| `dlopen2_derive` | `0.4.3` | `MIT` | Linux; proc-macro | **MIT-FILE**; [versioned source](https://raw.githubusercontent.com/OpenByteDev/dlopen2/cc80e4a0a90d499b677fdf7743699b4b3a43a989/LICENSE) |
| `jni-sys-macros` | `0.4.1` | `MIT OR Apache-2.0` | Other graph entry | **JNI**; [versioned source](https://raw.githubusercontent.com/jni-rs/jni-sys/64d77b7a5f119d7b55b4e2c169a4668067ff59e6/LICENSE-MIT) |
| `ndk` | `0.9.0` | `MIT OR Apache-2.0` | Other graph entry | **NDK**; [versioned source](https://raw.githubusercontent.com/rust-mobile/ndk/49bbbba16c58ff63cb8a0ad0eca5a9fb7ecaec25/LICENSE-APACHE) |
| `ndk-sys` | `0.6.0+11769913` | `MIT OR Apache-2.0` | Other graph entry | **NDK**; [versioned source](https://raw.githubusercontent.com/rust-mobile/ndk/49bbbba16c58ff63cb8a0ad0eca5a9fb7ecaec25/LICENSE-APACHE) |
| `objc2` | `0.6.5` | `MIT` | macOS | **O-MIT**; [versioned source](https://github.com/madsmtm/objc2/blob/d7d2fa23ceaa5e6096c923b081040e5d81b3b9df/LICENSE.md) |
| `objc2-app-kit` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | macOS | **O-APACHE**; [versioned source](https://github.com/madsmtm/objc2/blob/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md) |
| `objc2-cloud-kit` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | Other graph entry | **O-APACHE**; [versioned source](https://github.com/madsmtm/objc2/blob/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md) |
| `objc2-core-data` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | Other graph entry | **O-APACHE**; [versioned source](https://github.com/madsmtm/objc2/blob/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md) |
| `objc2-core-foundation` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | macOS | **O-APACHE**; [versioned source](https://github.com/madsmtm/objc2/blob/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md) |
| `objc2-core-graphics` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | Other graph entry | **O-APACHE**; [versioned source](https://github.com/madsmtm/objc2/blob/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md) |
| `objc2-core-image` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | Other graph entry | **O-APACHE**; [versioned source](https://github.com/madsmtm/objc2/blob/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md) |
| `objc2-core-location` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | Other graph entry | **O-APACHE**; [versioned source](https://github.com/madsmtm/objc2/blob/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md) |
| `objc2-core-text` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | Other graph entry | **O-APACHE**; [versioned source](https://github.com/madsmtm/objc2/blob/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md) |
| `objc2-encode` | `4.1.0` | `MIT` | macOS | **O-MIT**; [versioned source](https://github.com/madsmtm/objc2/blob/8d214f5477365ffcbcbb7de058c86ed9a518efb7/LICENSE.md) |
| `objc2-exception-helper` | `0.1.1` | `Zlib OR Apache-2.0 OR MIT` | macOS | **O-APACHE**; [versioned source](https://github.com/madsmtm/objc2/blob/8d214f5477365ffcbcbb7de058c86ed9a518efb7/LICENSE.md) |
| `objc2-foundation` | `0.3.2` | `MIT` | macOS | **O-MIT**; [versioned source](https://github.com/madsmtm/objc2/blob/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md) |
| `objc2-io-surface` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | Other graph entry | **O-APACHE**; [versioned source](https://github.com/madsmtm/objc2/blob/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md) |
| `objc2-quartz-core` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | Other graph entry | **O-APACHE**; [versioned source](https://github.com/madsmtm/objc2/blob/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md) |
| `objc2-ui-kit` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | Other graph entry | **O-APACHE**; [versioned source](https://github.com/madsmtm/objc2/blob/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md) |
| `objc2-user-notifications` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | Other graph entry | **O-APACHE**; [versioned source](https://github.com/madsmtm/objc2/blob/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md) |
| `objc2-web-kit` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | macOS | **O-APACHE**; [versioned source](https://github.com/madsmtm/objc2/blob/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md) |
| `selectors` | `0.38.0` | `MPL-2.0` | Other graph entry | **MPL**; [versioned source](https://github.com/servo/stylo/blob/572ecba2d1600e7c3d490586692a209faf703baa/selectors/Cargo.toml) |
| `webview2-com` | `0.39.1` | `MIT` | Windows MSVC | **WEBVIEW-RUST**; [versioned source](https://raw.githubusercontent.com/wravery/webview2-rs/edc2caf886175ccaebe86078c9cfe1ae2a187328/LICENSE) |
| `webview2-com-macros` | `0.8.1` | `MIT` | Windows MSVC; proc-macro | **WEBVIEW-RUST**; [versioned source](https://raw.githubusercontent.com/wravery/webview2-rs/dffa41a8a46d3f5565eefbff2de57d38d399f158/LICENSE) |
| `webview2-com-sys` | `0.39.1` | `MIT` | Windows MSVC | **WEBVIEW-RUST + SDK**; [versioned source](https://raw.githubusercontent.com/wravery/webview2-rs/edc2caf886175ccaebe86078c9cfe1ae2a187328/LICENSE) |
| `winapi-i686-pc-windows-gnu` | `0.4.0` | `MIT/Apache-2.0` | Other graph entry | **WINAPI**; [versioned source](https://static.crates.io/crates/winapi-i686-pc-windows-gnu/winapi-i686-pc-windows-gnu-0.4.0.crate) |
| `winapi-x86_64-pc-windows-gnu` | `0.4.0` | `MIT/Apache-2.0` | Other graph entry | **WINAPI**; [versioned source](https://static.crates.io/crates/winapi-x86_64-pc-windows-gnu/winapi-x86_64-pc-windows-gnu-0.4.0.crate) |

The reviewed [supplement manifest](../crates/lumendesk-cli/third-party/overrides.json)
contains each `.crate` checksum, source revision, notice URL and notice-text
checksum. Supplements are bound to the complete package identity and declared
license expression. An update, alternate registry, changed checksum or changed
license expression cannot silently inherit another package's reviewed notice.
No invented copyright holder or template substitution is used.

### Inspection of the corrected source archive

The [new archive inspection record](../crates/lumendesk-cli/verification/redistribution-rebuilt-source.json)
records a source archive produced during this review. It contains **275
dependencies, zero empty notice lists and 547 payload records**. Every payload
length and SHA-256 matched its manifest. The archive SHA-256 is
`1acfa401e5d607a48e3d5f97924b6e5b49c717d8dc707bcf4b1182e9fc59d55b`.

For **35** exact registry packages, the packager verified the cached `.crate`
against the locked checksum and compared the local source files with that
archive before applying curated notices or publishing a verified source URL.
`DEPENDENCY-SOURCES.txt` identifies those exact unmodified sources, including
the five MPL packages in the all-target graph. Expected source URLs for other
packages are explicitly marked as unverified. A missing or altered cache cannot
silently gain an authoritative source-availability statement.

The same archive retains **19 unresolved objc2-family review entries**, including
additional attribution qualifications for the four MIT-only entries, and both
`license_review_required` and `redistribution_review_required` remain true.
Thus the zero-empty-list count records successful notice collection; it does
not close those review issues or convert the source bundle into a cleared
desktop binary release. No GUI application was executed during this inspection.

### Inspection of a corrected Linux web binary archive

The final reviewed SDK was also compiled and packaged as an actual
`x86_64-unknown-linux-gnu` web application. Its [binary inspection record](../crates/lumendesk-cli/verification/redistribution-rebuilt-linux-binary.json)
contains **145 dependency identities, zero empty notice arrays and 255 payload
records**, with all byte lengths and SHA-256 values valid. The archive SHA-256 is
`5c023321bb8c305949c27a546ffa265139bc73d874ba6f4f7f4d26af2be0a812`.

The host-filtered build graph includes `dlopen2`, `dlopen2_derive` and the
MPL-only `option-ext`; all three registry sources passed archive and local-tree
verification. `DEPENDENCY-SOURCES.txt` includes the versioned `option-ext 0.2.0`
source location and locked hash. Apple and Windows packages are absent from this
Linux inventory. There are no dependency-specific unresolved flags in this
fixture, but the manifest retains both review requirements. That result does not
inspect or clear the application's own rights or the system/toolchain obligations
listed below.

ELF inspection confirms that WebKitGTK, GTK, GLib and related runtime libraries
remain dynamically required. The application was built and inspected without
launching its GUI. This new Linux result does not establish macOS/Windows binary
packaging, clean-machine installation, or real WebView acceptance.

### Windows: the SDK loader has its own omitted notices

The exact `webview2-com-sys 0.39.1` source uses `WebView2LoaderStatic.lib` for
MSVC. Its [upstream generator at the published revision](https://github.com/wravery/webview2-rs/blob/edc2caf886175ccaebe86078c9cfe1ae2a187328/crates/update-bindings/src/main.rs)
pins Microsoft.Web.WebView2 SDK **1.0.3800.47**. The
[official NuGet archive](https://api.nuget.org/v3-flatcontainer/microsoft.web.webview2/1.0.3800.47/microsoft.web.webview2.1.0.3800.47.nupkg)
was downloaded and all **nine** DLL/static/import-library files across
arm64/x64/x86 were compared byte-for-byte with the crate; every file matched.
The SDK archive SHA-256 is
`56c9f26bdd07916a2d1949fb58a5c7e434dfa1173577dca879206050c4e718db`.

That SDK ships a Microsoft BSD-3-Clause-style license plus third-party notices
for ANTLR/StringTemplate. Neither file was in the crate's published package.
The supplement preserves both files **in addition to** Bill Avery's Rust MIT
notice. Thus merely finding the Rust project's MIT license would have missed a
real binary redistribution requirement. This inspection covers the SDK loader;
the separately installed WebView2 Runtime is not bundled and is a different
redistribution decision.

### Extra obligations outside the 31-item count

The baseline root filename matcher omitted `COPYRIGHT` and `UNLICENSE`. This
hid `COPYRIGHT-RUST.txt` in cesu8, but also omitted copyright files in GTK-family
crates and other dependencies that already had a non-empty LICENSE list. It
also missed nested notices such as `libdbus-sys/vendor/dbus/COPYING` and
`vendor/dbus/AUTHORS`. A zero empty-notice count would not have detected this
loss. Unrecognized filenames must still be inspected with care: `copying.rs`
is Objective-C copying API code, and `libwinapi_oemlicense.a` is a binary import
library, not a license notice.

The additional native-only `epaint_default_fonts 0.36.2` supplement preserves
all four original font notice files: `fonts/OFL.txt`, `fonts/UFL.txt`,
`fonts/emoji-icon-font-mit-license.txt` and `fonts/Hack-Regular.txt`, together
with the exact egui code license texts at its recorded source revision
[`49682f8baa058bf49e011035cfbd6e825f88a5ef`](https://github.com/emilk/egui/tree/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/epaint_default_fonts).
It also preserves the original font name-table copyright/license records and
hashes for Ubuntu Light, Noto Emoji, Hack and emoji-icon-font. Hack's notice
includes **Bitstream Vera** terms, which the crate-level SPDX expression does
not enumerate. The fonts remain unmodified. Font embedding in a compiled app
still distributes the font bytes; treating them as an implementation detail
would not remove those obligations. The [OFL license owner](https://openfontlicense.org/)
and [Ubuntu font license](https://ubuntu.com/legal/font-licence) describe their
respective conditions.

An additional real native **source** archive was inspected separately. Its
all-target inventory has **395 dependencies and 42 entries still without notice
files**; the exact names, versions and declared terms are retained in the
[native scope record](../crates/lumendesk-cli/verification/redistribution-native-scope.json).
The font crate's seven supplemental files are present and its registry source
was verified. The remaining entries include egui/eframe and AccessKit families,
older objc2 versions, GL tooling and other native-specific packages. They are
outside the original 275/31 web inventory; reviewed notices must not be reused
across versions by name alone. These metadata gaps do not prove that all 42 are
linked into any one native binary, and the inspected source archive does not
vendor their crate code. They do establish that the complete native stack has
not passed release licensing review.

## What can and cannot be concluded about redistribution

The reconstructed Linux web build's two empty-notice entries have identifiable
upstream MIT notices and are addressed by the pinned supplements. The Microsoft
loader's additional notices have also been identified and preserved. Those are
concrete notice defects with reproducible fixes. They do not establish that an
arbitrary application produced by the packager is legally ready to release.

For an actual distribution, unresolved work includes:

1. **Selected target and actual payload.** Review the exact application's own
   license, embedded assets, generated material and linked dependencies. The
   275-entry all-target graph is not a substitute for this selection. Other
   versions/features can introduce additional gaps; the native font correction
   is not a claim to clear the complete native dependency graph.
2. **MPL source availability.** For Linux/macOS web builds, make the exact
   `option-ext 0.2.0` covered source available and tell recipients how to obtain
   it. A verified versioned `.crate` URL and hash can document the unmodified
   source; a mutable local registry tree cannot silently be represented as that
   upstream source. Distribute any actual covered modifications under MPL.
3. **Apple-specific qualification.** For affected Apple builds, the upstream
   qualification and four MIT attribution gaps remain explicit review items.
   The included standard MIT reference text is not a replacement for missing
   author-specific material. These issues do not apply to an unrelated Linux
   executable just because the packages appear in the all-target inventory.
4. **System and toolchain components.** The Cargo inventory omits system
   libraries and Rust toolchain runtime components. Separately installed
   GTK/WebKit libraries are not copied by the archive; nevertheless LGPL-linked
   program conditions, including applicable license/use notices, replacement
   and debugging-related reverse-engineering rights, still need review under
   [LGPL 2.1 section 6](https://spdx.org/licenses/LGPL-2.1-only.html).
   The baseline runtime notes alone are not evidence that these requirements
   were satisfied. Bundling the shared libraries would add their own exact
   source/notice obligations.
5. **Release-specific approval.** The maintainer must review the unresolved
   entries and the final artifact. Supplementing filenames, checksum validation
   and successful builds must never change `license_review_required` into a
   compliance certificate.

## Release evidence and approval

For each release archive, retain its exact SHA-256, manifest, dependency inventory,
Cargo lockfile, feature/target configuration, and the notice/source-availability
bundle that travels with it. Inventory failures must remain visible until they
are resolved against exact upstream material or documented as not distributed.

The PR's cross-platform CI checks compilation, linking and display-free tests.
It does not inspect the legal sufficiency of a release, run real Wry IPC, verify
WebView security boundaries, or complete the GUI acceptance cases recorded in
`ACCEPTANCE.md`. `CONTRIBUTING.md` still requires explicit maintainer approval and
squash merging. Green CI alone does not authorize merging or distribution.
