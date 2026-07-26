# Contributing

Thanks for helping improve `solana-receive`.

## Commit and PR conventions

- Use Conventional Commits, for example `fix: reject stale receipts` or
  `docs: clarify Surfpool setup`.
- Keep PRs focused and describe the behavior, docs, or generated output they
  change.
- Include the commands you ran in the PR description. If a command is skipped,
  explain why.
- Commit regenerated files when changing the IDL, Codama config, or client
  generation.

## Before opening a PR

Run the smoke gate from the repo root:

```bash
./scripts/smoke.sh
```

LiteSVM tests load the SBF artifact from `target/deploy`, so run `cargo
build-sbf` before LiteSVM-only verification:

```bash
cargo build-sbf --manifest-path program/token-2022-receive/Cargo.toml -- --locked
cargo test -p token-2022-receive --test litesvm_before_after --test litesvm_lifecycle -- --nocapture
```

Read [docs/OPERATOR.md](docs/OPERATOR.md) before demos or local deploys, and
[docs/VERIFICATION.md](docs/VERIFICATION.md) before making evidence or compute
unit claims.

## Safety

- Never commit keypairs, wallet files, seed phrases, private keys, or `.env`
  files.
- Treat `target/deploy/*-keypair.json` as local secret material.
- Keep the custom-program / non-mainnet-product caveats intact unless the
  project scope changes explicitly.
