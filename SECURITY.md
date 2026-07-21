# Security Policy

OpenAjo escrows user funds. Treat every finding accordingly.

## Reporting

Do **not** open a public issue for vulnerabilities. Use GitHub's private
vulnerability reporting (Security → Advisories → Report a vulnerability) on
this repository.

Include: the affected contract/function, reproduction steps, and impact.

## Scope of greatest concern

- Auth bypass on any user action (`create_circle`, `join`, `leave`, `cancel`,
  `contribute`)
- `settle_cycle` manipulation: double payout, wrong recipient, slashing more
  than `deposit_remaining`, settlement before due
- Cross-contract reporter spoofing against `reputation`
- Any path that strands or leaks escrowed funds

## Status

Unaudited. Deployed to **testnet only**. Do not use with mainnet funds until a
professional audit has been completed.
