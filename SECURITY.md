# Security Policy

## Reporting security issues

Please do not disclose security vulnerabilities publicly before they are reviewed.

For now, report issues directly to the maintainer through the official project communication channel.

Include:

- QUB Core version
- OS
- Reproduction steps
- Screenshots/logs if safe
- Whether funds, mining rewards, wallet data, or consensus behavior may be affected

## Sensitive data

Never send private keys, wallet.json, ethereum-wallets.json, seed phrases, SSH keys, or .env files.

## Scope

High priority issues include:

- consensus bugs
- wallet/key safety
- private key exposure
- transaction validation errors
- mempool bugs that can cause invalid blocks
- mining/fork/stale-chain bugs
- update/installer integrity issues
- bridge or stablecoin contract security issues
