# Security

## Reporting a problem

Please report vulnerabilities privately through GitHub: **Security → Report a vulnerability** on this repository.
Do not open a public issue for security problems.

## What is checked

| Check | When | Tool |
|---|---|---|
| Known vulnerabilities in dependencies | Every change and every Monday | `cargo deny check advisories` (RustSec database) |
| Licenses, banned crates, untrusted sources | Every change | `cargo deny check` |
| Dependency updates | Weekly pull requests | Dependabot |

Run the same checks locally:

```sh
cargo install --locked cargo-deny
cargo deny check
```

## Priority areas

| Area | Why |
|---|---|
| JavaScript engine (`boa_engine`) | It is the sandbox boundary between plugins and the device. Update promptly. |
| Networking and TLS (`ureq`, `rustls`) | Handles untrusted data from the internet. |
| Plugins | Run untrusted code. Signed updates are planned; plugins that bundle JavaScript libraries must be audited when built. |
