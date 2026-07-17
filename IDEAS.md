# IDEAS

Ideas and proposals for future development.

## Coverage guard

New development should not lower overall test coverage.
Consider automating this as part of the CI pipeline:

- Run `cargo llvm-cov` before and after the change
- Compare coverage percentages
- Fail the build if coverage drops below a threshold
- Report the delta in the PR description

This could be implemented as a GitHub Action or a pre-push hook.
