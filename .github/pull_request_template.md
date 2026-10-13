## Summary

Explain the user-visible change and why it is needed. Keep the PR focused.

## Verification

List commands and results, including checks that failed or were not run.
Distinguish headless contract tests from actual window/OS validation.

## Security and compatibility

Describe API changes, migration needs, cancellation/resource limits, origin or
asset trust changes, and any new dependencies or license obligations.

## Checklist

- [ ] PR title and new commits follow Conventional Commits
- [ ] Tests cover normal behavior, failure, cancellation and repeated use where applicable
- [ ] Documentation and acceptance status match verified behavior
- [ ] No credentials, private content, generated binaries or build caches included
- [ ] Clean-room provenance preserved; no upstream implementation copied or consulted
- [ ] CI passes for the current commit; review findings resolved
- [ ] Maintainer approval obtained before using **Squash and merge**
