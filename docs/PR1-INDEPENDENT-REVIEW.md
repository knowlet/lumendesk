# PR #1 independent review — 2026-10-19

## Decision and scope

The original PR had actionable correctness and packaging defects even though its
three build jobs passed. The changes accompanying this report fix the reproduced
defects and add regression coverage. This report is evidence for maintainer review;
it is not maintainer approval, a completed GUI acceptance record, or permission to
redistribute an arbitrary generated application.

- PR: [knowlet/lumendesk #1](https://github.com/knowlet/lumendesk/pull/1).
- Reviewed base: `288e79fda31580bdcb969f1951f21c2372ee5ef6`.
- Reviewed original head: `a90f4c2d247c8b0cedd337ac911c12903f6faca8`.
- Original scope: 57 changed files, 5,602 additions and 255 deletions.
- Original exact-head CI: [39188660594](https://github.com/knowlet/lumendesk/actions/runs/39188660594),
  with `linux-contracts`, `desktop-macos-15` and `desktop-windows-2022` successful.
- Sourcery's 150,000-character review limit and Codex's exhausted review quota
  establish incomplete automated review, not a new code defect or a passing review.

The review examined the Rust bridge, stream queue and host lifecycle together with
the JavaScript runtime; archive input selection, metadata, path and handle checks,
manifest rewriting and output integrity; and the pinned web dependency inventory.
Separate reviewers handled these areas and cross-checked the resulting fixes.
Evidence comes from this repository, regression execution, official dependency
sources, and version-pinned license material. No original toolkit implementation,
examples, tests or assets were consulted.

Local verification used Ubuntu 24.04, Rust/Cargo 1.99.0, Node 24.19.0, the locked
TypeScript 7.0.2 tools, and GTK 3/WebKitGTK 4.1 development libraries. No desktop
window was created. Node VM fixtures do not authenticate a real WebView frame or
demonstrate an actual engine navigation race.

## Findings and fixes

Priorities below describe engineering urgency for this pre-release milestone;
they are not CVSS scores or claims of a demonstrated remote exploit.

| ID | Priority | Confirmed problem | Remediation and regression evidence |
|---|---|---|---|
| R1 | P2 | Request IDs were allocated before argument serialization. A `toJSON` that submits another call/stream sends IDs in the order `2, 1`, so Rust rejects the outer request as reused. AbortSignal listener registration or its `aborted` getter can also submit nested work. | Serialize fields once, finish signal interactions and recheck session/capacity, then allocate the ID immediately before transport submission. Eight call/stream reentrancy cases exercise argument, listener and getter paths. The original serialization cases failed with reversed IDs. |
| R2 | P2 | Immediate IPC cancellation reached the core, but a full host command queue could drop the queued Cancel command. Host polling without read credit or an attached receiver did not notice cancellation, leaving a UI stream slot until idle expiry. | `Bridge::is_request_cancelled` observes admitted cancellation and pre-cancel tombstones without consuming either data or the tombstone. Host polling reclaims the slot without requiring credit or queue admission. The regression covers admitted and pre-admission cancellation and verifies a late queued job remains cancelled. |
| R3 | P2 | Successful results were size-bounded but terminal errors were not. A large backend error could be cloned into stream completion slots; an admission error could also expand an otherwise size-valid request. | Apply the encoded result/error limit before delivery or terminal cloning. A 384-byte Unicode error under a 128-byte limit becomes `PayloadTooLarge`, while accepted stream items retain order. A separate regression covers expansion of an unknown-method admission error. |
| R4 | P2 | The host embedded serialized JSON directly as JavaScript object literals. A `"__proto__"` key then changed the received object's prototype semantics and disappeared as an own data property. | Pass safely quoted JSON strings through `JSON.parse` for responses, stream frames and event payloads. Rust-generated scripts and Node share a fixture containing nested `__proto__`, quotes, a newline and Unicode. Original delivery lost the own properties. This finding concerns data/prototype semantics, not demonstrated global prototype pollution or script injection. |
| P1 | P2 | Packaging parsed `lumendesk.toml` before validating input file types. A static FIFO at that path blocked the CLI; the original-version regression required termination after three seconds. | Collect and bound selected regular inputs before configuration parsing or Cargo invocation; parse the captured configuration bytes. The FIFO regression now fails promptly without creating an archive. |
| P2 | P2 | Archive validation accepted double quotes, `COM¹`/`LPT²` device-name variants and case-colliding directory components. Complete-path comparison missed `web/A.rs` against `web/a.rs/child.rs`. | Reject portable invalid/reserved names and compare every shared component plus file/directory prefixes. Test case and canonical Unicode aliases using a conservative NFC/uppercase key. Original path/name fixtures failed their rejection assertions. |
| P3 | P2 | The documented source-hardlink rejection only checked link counts under `cfg(unix)`. Windows had no equivalent link-count check. | Inspect the opened Windows handle with `GetFileInformationByHandle`; add a hardlink integration regression on every supported desktop platform. This is a confirmed code omission; original-head Windows execution was not available locally. Normal Cargo hardlinked executable outputs remain an explicit exception and become regular archive bytes. |
| L1 | Release review | Root-only notice discovery missed real `COPYRIGHT`, `UNLICENSE`, nested vendored notices and embedded font terms. Several published crates omitted their workspace license files; the Windows loader had additional notices outside the Rust crate's license. | Bounded recursive notice discovery, exact identity/checksum-bound supplements, preserved source provenance, and explicit unresolved qualifications. The complete 31-entry ledger and Microsoft/font inspection are in [REDISTRIBUTION.md](REDISTRIBUTION.md). |

### Related hardening with narrower evidence

Host-to-page deliveries now include their original session token, checked when the
JavaScript callback executes. A shared fixture demonstrates that a delayed script
cannot settle a replacement page's reused request ID or emit its event. The original
runtime accepted that script in a VM. Actual Wry navigation timing still needs
engine testing, so this is a demonstrated protocol-contract issue and mitigation,
not a claim that a real engine exploit was reproduced.

Package reads validate the actual opened handle, using `O_NOFOLLOW | O_NONBLOCK`
on Unix and `FILE_FLAG_OPEN_REPARSE_POINT` with constrained sharing on Windows.
Reparse-point rejection and Windows junction tests strengthen coverage. They do
not establish that the original Rust metadata checks permitted a junction escape.
The existing requirement not to mutate an input tree concurrently remains:
ancestor-directory replacement isolation and malicious build-script containment
are not properties of this snapshot utility.

Checksums cover every payload entry and the tests inspect entry type, normalized
metadata, byte length and SHA-256. These are integrity checks, not signatures or
authenticated build provenance. No symlink or hardlink archive entries are emitted.
Case/Unicode normalization deliberately does not claim to model every filesystem.

## Stream and resource invariants retained

The review exercised or traced the existing single-read credit, bounded Rust
queue, accepted-item-before-terminal ordering, blocked producer cancellation,
retained sink sealing, session invalidation, window closure, idle expiry, and
separate worker/queue limits. No additional reproducible violation in those
reviewed paths remained after the fixes.

Cancellation releases frontend/host ownership promptly. A non-cooperative Rust
handler still holds its worker permit until it actually exits; cancellation is
not forced thread termination. The cap therefore remains meaningful even when
UI slots have already been reclaimed. Completion after the last stream item
requires another `next()` under the documented pull contract; `.result` alone
does not grant read credit or drain queued data.

The configured payload bound covers encoded request/value/item/error payloads.
Host wire envelopes and escaped JavaScript strings have additional overhead.
The documentation now distinguishes these quantities instead of promising that
an entire outgoing envelope fits the payload threshold.

## Packaging and license evidence

The historical archive metadata contains aggregate counts, not its original
275-entry `DEPENDENCIES.json` or generated application lockfile. A separately
identified, offline reconstruction seeded by the reviewed lockfile reproduced
the 275 dependencies and 31 empty notice lists. It does not claim to reproduce
the historical archive's bytes or checksums.

The corrected source archive's inventory contains 275 dependencies and no empty
notice arrays. The complete evidence, payload hashes, archive identity and retained
qualifications are in the [source inspection record](../crates/lumendesk-cli/verification/redistribution-rebuilt-source.json).
Nineteen objc2-family entries still carry explicit review qualifications, including
four incomplete holder-specific MIT notices. Their upstream Apple SDK qualification
was preserved; neither attribution nor a grant of rights was invented.

Before using a curated supplement or a verified source URL, the packager hashes
the exact cached `.crate` against Cargo.lock and compares its regular files with
the local registry source tree. Changed, missing and added files cannot silently
receive an unmodified-upstream source claim. During cross-review of this new
verification code, a `.cargo-ok` directory could initially hide extra files. A
failing regression led to allowing only a checked, bounded regular extraction
marker. That remediation defect was fixed before integration and is distinct
from the original PR findings.

`DEPENDENCY-SOURCES.txt` lists verified versioned registry source locations and
hashes. Expected but unverified locations and their verification status remain
in `DEPENDENCIES.json`.
For Linux/macOS web builds, the reachable MPL-only package requiring attention is
`option-ext 0.2.0`. The all-target graph also contains MPL crates absent from the
selected desktop web closures. Source mode does not vendor registry source code;
binary mode uses the host target's normal/build closure, including build helpers
conservatively and excluding development-only and unreachable nodes.

The additional native source inventory is a different scope: 395 dependencies,
with 42 still-empty notice lists recorded in the [native inspection](../crates/lumendesk-cli/verification/redistribution-native-scope.json).
The corrected native font bundle does not clear those additional versions and
packages. The original 31 web entries have all been investigated; a full native
release review remains incomplete.

No notice count clears application-owned assets/licenses, Apple-specific rights,
system/toolchain components or applicable LGPL linking conditions. Both
`license_review_required` and `redistribution_review_required` remain true.
Creating an internal review archive is permitted without implying release approval.

## Verification record

The complete local `sh scripts/verify.sh` run passed:

- `cargo fmt --all -- --check`.
- `cargo test --workspace --all-features --locked`: 122 unit/integration tests
  and two doctests passed; one environment-dependent real web archive test was
  intentionally separate.
- `cargo clippy --workspace --all-features --all-targets --locked -- -D warnings`.
- `cargo build -p lumendesk --all-features --examples --locked`.
- `node --test tests/*.test.mjs`: 135 tests passed, including 131 runtime tests.
- Strict TypeScript positive and negative fixtures: valid uses passed; invalid
  method/event/stream/type cases failed as expected.

The development-process integration test passed with normal process permissions,
including its real socket release/replacement assertion. An earlier restricted
run's `EPERM` was an execution-environment limitation, not treated as a source
regression.

The ignored `real_web_source_archive_builds_and_retains_sdk_test_fixtures` test
was then run explicitly and passed. It builds the relocated real web scaffold
with the bundled SDK and runs that SDK's tests, including the session-delivery
fixture. Its 157.80-second local run created no graphical window.

A fresh source archive and an actual Linux web binary archive were also produced
from the final reviewed SDK and inspected independently:

| Artifact | Dependencies | Empty notices | Verified source trees | Payload records with valid size/SHA-256 |
|---|---:|---:|---:|---:|
| Web source | 275 | 0 | 35 | 547 |
| Linux web binary | 145 | 0 | 3 | 255 |

The [binary inspection record](../crates/lumendesk-cli/verification/redistribution-rebuilt-linux-binary.json)
contains the exact artifact/executable hashes, all 145 dependency identities,
the verified `option-ext 0.2.0` MPL source notice, and its ELF shared-library
requirements. Its archive SHA-256 is
`5c023321bb8c305949c27a546ffa265139bc73d874ba6f4f7f4d26af2be0a812`.
The three source checks are `dlopen2`, `dlopen2_derive` and `option-ext`.
Apple and Windows dependencies are absent from this selected Linux inventory.
No dependency-specific unresolved flag remains in this Linux fixture; the
application/system/toolchain release questions remain outside that result,
and both manifest review requirements remain true. The actual GUI binary was
built and inspected, not executed. The ordinary integration suite separately
executes its extracted display-free binary fixture.

The CI workflow now also runs CLI library and portable-archive integration tests
on macOS and Windows. This adds native filesystem execution for hardlinks, junctions
and canonical path handling to those jobs; it still does not create GUI windows.
Every pushed fix requires its own successful current-commit workflow. The historical
`a90f4c2` result must not be reused as proof for a changed head. Consult the
[PR checks](https://github.com/knowlet/lumendesk/pull/1/checks) for the exact final
revision and three job conclusions.

## Existing merge conditions and outstanding acceptance

The branch retains the Conventional Commit PR title and the existing requirement
to squash only after explicit maintainer approval. No merge or auto-merge was
performed. The PR was open and non-draft when independently inspected; the body
still requested draft status until current CI and maintainer review were complete.
The inspection found no maintainer `APPROVED` review. Readiness status alone is
not equivalent to that approval.

After the final revision's three jobs succeed, these fixes can be assessed as the
documented source milestone. The remaining maintainer decision must explicitly
acknowledge the documented scope and release limitations. Successful CI does not
satisfy all existing merge conditions on its own.

The original [acceptance matrix](ACCEPTANCE.md) retains its scoped statuses.
Real Wry round trips, iframe/origin/navigation security boundaries, visible native
and web UI input, screen readers/IME/focus, real multiwindow close/event behavior,
offline GUI asset loading, installation on clean machines, persisted geometry,
desktop integrations and credible GUI performance measurements are not established.
In particular, the Linux Wry IPC frame-authentication limitation in
[SECURITY.md](SECURITY.md) remains explicit. Compilation/linking, headless protocol
tests, legal redistribution review and real product acceptance are separate evidence.
