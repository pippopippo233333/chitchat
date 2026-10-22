# Contributing

Thanks for helping. Bug reports, fixes, documentation and testing on other systems are all useful.

## Before you start

- For anything bigger than a small fix, open an issue first so we can agree on the approach.
- The API between the host and the web client is described in [docs/api.md](docs/api.md). Change it there in the same pull request as the code.

## Checks

The host:

```sh
cargo fmt --all --check
cargo clippy --all-targets
cargo test
```

The web client:

```sh
npm ci --prefix web
npm --prefix web run check
npm --prefix web test
npm --prefix web run build
```

The backup script:

```sh
python3 -m unittest discover -s scripts -p "test_*.py"
```

CI runs the same checks on every pull request.

## Guidelines

- Keep changes focused; one topic per pull request.
- Database changes go in a new schema version in `crates/chitchat-host/src/db.rs`. Never edit an old one, because existing hosts have already run it.
- Don't commit data folders, certificates, keys, `.env` files or screenshots with real people's messages.

## License

By contributing, you agree that your contribution is licensed under the [GNU AGPL v3.0](LICENSE), the license of this project.
