# Security Policy

## Supported Versions

Sephera accepts security reports for the latest stable release line only.

At triage time, "latest stable" means the most recent published release on GitHub and the matching crates.io release line. Older releases, unreleased local states, development snapshots, forks, and custom downstream distributions are out of scope for supported security remediation.

The following are not covered by this policy:

- modified source trees or custom-patched builds
- binaries that do not match the official release artifacts or published checksums
- nightly, local, or otherwise unreleased repository states
- issues that only affect unsupported or end-of-life release lines

## Reporting a Vulnerability

If you believe you have found a security vulnerability in Sephera, report it privately through GitHub private vulnerability reporting / Security Advisories for this repository.

Do not open a public GitHub issue for a vulnerability that has not been disclosed yet.

Use the private reporting flow for issues affecting, or plausibly affecting:

- the CLI and local execution paths
- the core analysis engine
- the MCP server implementation
- official release artifacts and distribution pipeline outputs
- docs-hosted integrations when there is a real security impact

## What to Include

Please include as much of the following as you can:

- affected version, tag, or release artifact name
- operating system, architecture, and installation method
- exact commands, inputs, repository URL, or sample data needed to reproduce the issue
- expected impact and observed behavior
- proof of concept, logs, stack traces, or screenshots when relevant
- your assessment of exploitability, reachability, or attacker prerequisites
- whether you have verified the behavior against the latest stable release

## Response Process

Sephera aims to follow this process for private security reports:

1. Acknowledge receipt within 5 business days.
2. Triage the report, validate reproducibility, and assess impact.
3. Decide whether the issue qualifies as a security vulnerability for supported versions.
4. Prepare remediation, release planning, and disclosure timing as appropriate.
5. Publish a fix or mitigation before public disclosure when reasonable.

Response times are best-effort and may vary depending on report quality, severity, and maintainer availability.

## Disclosure Expectations

Please give maintainers reasonable time to investigate and remediate an issue before public disclosure.

If coordinated disclosure is needed, Sephera will work in good faith to agree on a timeline once the report has been validated.

## Safe Harbor

Good-faith security research is welcome.

When researching Sephera, please:

- avoid actions that degrade availability, integrity, or performance for other users
- avoid accessing, modifying, or exfiltrating data that is not your own
- avoid social engineering, spam, or credential attacks
- stop testing and report promptly once you confirm a plausible vulnerability

Activities that exceed these boundaries may be treated as abusive rather than responsible disclosure.

## Non-Security Reports

If the issue is a bug, feature request, usability problem, or hardening idea without a demonstrated security impact, use the normal issue templates instead of the private reporting channel.
