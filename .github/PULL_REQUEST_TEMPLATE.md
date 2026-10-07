## What Changed?
<!-- Clear description of the changes introduced in this PR -->

## Why?
<!-- Context, motivation, or related GitHub issue (e.g. Closes #123) -->

## Type of Change
- [ ] Bug fix (non-breaking change which fixes an issue)
- [ ] New feature (non-breaking change which adds functionality)
- [ ] Performance improvement
- [ ] Documentation update
- [ ] Refactoring
- [ ] Security fix
- [ ] Other

## Testing Completed
- [ ] Added or updated automated tests
- [ ] Rust tests passed (`cargo test --workspace`)
- [ ] .NET integration tests passed (`dotnet test`)
- [ ] Rust clippy passed (`cargo clippy --all-targets -- -D warnings`)
- [ ] Tested on Windows 10 / Windows 11

## Security & Safety Impact
- Does this change touch file deletion, cleanup rules, or privilege boundaries?
- [ ] No safety impact
- [ ] Yes (explain below how the safety protocol is preserved):

## Screenshots / Visual Changes (if applicable)
<!-- Attach screenshots or GIFs for UI/UX changes -->

## Checklist
- [ ] No secrets, credentials, or personal local paths committed
- [ ] Code follows existing project conventions and patterns
- [ ] All unmanaged FFI pointers wrapped in `SafeHandle`
- [ ] Native Rust exports wrapped in `catch_unwind` panic guards
- [ ] Documentation updated where relevant
