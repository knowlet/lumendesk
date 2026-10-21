# Contributing

## Branches and pull requests

The initial `main` publication is the validated first milestone from local
source snapshot `8913199473040cf10780359d0cc0f33889c6df9b`. Its GitHub
commit is `288e79fda31580bdcb969f1951f21c2372ee5ef6`; both have tree
`b51ac79aeb9c4eb245ee1be4f580bdde8277cb5b`. The supported import API
assigns new commit metadata; original authored history remains in the Git bundle. Subsequent changes use a focused
branch and a **draft pull request** targeting `main`; do not push follow-up
changes directly to `main`. Open the PR early, explain scope and verification,
and mark it ready only after the current revision is tested and ready for review.
Never include credentials, build caches, binary artifacts or personal data.

Use Conventional Commits for new commits and the PR title:

- `feat(webview): add bounded pull streams`
- `fix(cli): reject symlink archive inputs`
- `docs: explain platform verification gaps`
- `ci: verify Linux contracts and portable archives`

Use `!` and a `BREAKING CHANGE:` footer when applicable. Before first publication, the follow-up branch normalizes one legacy commit
subject while preserving every source tree and the original delivered Git bundle.
Do not rewrite remote history after publication without maintainer coordination.

## Review and merge

Include the motivation, behavior, tests, compatibility/security implications and
known gaps using the PR template. Prefer small changes; isolate unrelated work.
Resolve review findings and rerun relevant checks after fixes. A green CI run
is necessary evidence, not permission to merge or proof of OS compatibility.

Merge only with the maintainer's explicit approval, using **Squash and merge**.
Use the Conventional Commit PR title as the squash subject and summarize the
change and verification in the body. Do not enable auto-merge or merge merely
because a PR was requested. Start the next branch from updated `main` after
merge rather than continuing the old feature branch.

Recommended repository configuration is squash-only merging, PRs required on
`main`, review and CI required, resolved review threads, and no force pushes.
These are setup recommendations, not a claim that remote rules have been
configured. Repository administrators should verify plan support and choose
review requirements that fit the number of maintainers.

## Local verification

Use Rust 1.99.0, Node 24 and the Linux development dependencies described in
[PLATFORMS](docs/PLATFORMS.md). Install locked Node tooling with `npm ci`, then:

```sh
sh scripts/verify.sh
cargo test -p lumendesk-cli --test package --locked \
  real_web_source_archive_builds_and_retains_sdk_test_fixtures -- --ignored --exact
```

The second test requires Linux GTK/WebKit development libraries and dependencies
cached before its offline checks. On a fresh machine, run `cargo fetch --locked`
for the workspace and fetch the generated web scaffold's locked dependencies too;
a Linux-only build does not download every cross-platform metadata dependency. It rebuilds a relocated source archive and runs its SDK
tests. No display is used. CI uses a read-only token, no repository secrets, a
full-SHA-pinned checkout and no credential persistence. It does not publish,
deploy, sign, create releases or merge PRs. Hosted CI has not been verified until
a successful remote run is recorded. A private repository's Actions usage may
consume its owner's included allowance; no paid quota is provisioned here.

Real native/webview windows, accessibility and OS integration on all three
desktop platforms remain separate manual acceptance work. Hosted macOS/Windows
compilation and display-free tests are recorded in the platform notes. Keep
[ACCEPTANCE](docs/ACCEPTANCE.md) honest when adding coverage.

## Clean-room boundary

Read [CLEAN_ROOM](docs/CLEAN_ROOM.md) and the functional specification. Work from
public functional requirements and independent dependency documentation; do not
consult or copy the original toolkit's implementation, tests, examples or assets.
Record provenance for added material and review third-party license obligations.
