## What this changes

<!-- One or two sentences. This repo holds user funds: say which money paths are touched, if any. -->

Closes #

## Contract(s)

- [ ] `circle`
- [ ] `reputation`
- [ ] CI / scripts / docs

## How I verified it

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --target wasm32v1-none --release
```

## Checklist

- [ ] CI is green
- [ ] Every new public function documents who may call it and calls `require_auth()` where it acts for a user
- [ ] New functions have tests, including `#[should_panic]` error paths
- [ ] Balance-changing logic asserts **exact** balances
- [ ] Every new persistent write extends TTL
- [ ] No `unwrap()` / `expect()` outside tests
- [ ] If an event topic or data tuple changed, a coordinated issue is open in `openajo-app`
