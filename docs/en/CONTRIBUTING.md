# Contributing to Twitch Live Alert

[Français](../../CONTRIBUTING.md) · [Source repository](https://github.com/LINDECKER-Charles/twitch-live-alert)

Start feature branches from `dev` and target it for integration. `test` contains the installation candidate. Promote a verified candidate to `main`. State the expected behaviour and the checks performed. Include screenshots for interface changes. Keep profile files, databases and personal credentials on your machine.

| Location | Responsibility |
| --- | --- |
| `src/`, `tests/` | Rust service, Twitch polling, rules, delivery channels and the local HTTP API |
| `web/` | Browser interface embedded in the service |
| `src-tauri/` | Windows desktop, notification area, service lifetime and NSIS configuration |
| `scripts/` | Development, packaging and installation checks |
| `docs/` | User guides, distribution and interface references |

Each Rust manifest has its own lockfile. Follow the commit scopes in `AGENTS.md`.

## Checks

Run the following commands from the project root with Rust 1.99.1 and Node 24.

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
node --test tests/smoke-dashboard.mjs
```

Run the browser workflows against fixture data in headless Chromium.

```sh
python -m pip install playwright==1.60.0
python -m playwright install chromium
python docs/design/verify_ui.py
```

On Linux, `python -m playwright install --with-deps chromium` also installs system libraries. On Windows, `--channel msedge` selects an installed Microsoft Edge. Use the Windows distribution guide for the native desktop checks and installer test. [Windows distribution](DISTRIBUTION.md)
