# Contributing

HX Unbound is an experimental Windows source preview. Start with the offline
checks in README.md. Device tests are separate, explicitly supervised work;
never infer panel health from a successful command reply alone.

For a bug report, include the app revision, Windows/Python versions, device and
firmware identity, the action sequence, whether USB was connected, and whether
audio and front-panel controls still worked. Reproduce with synthetic data first.
Review logs before sharing; they can contain preset or capture names. Do not
attach personal backups, third-party capture weights, credentials or firmware.

Keep transport/firmware changes distinct from editor features. See
[upstream reconciliation](docs/UPSTREAM.md) and retain upstream attribution.
Do not assume another device uses the Floor's memory layout, model IDs or USB
completion sequence. A2 Full execution is not validated here.

The repository is GPL-3.0-or-later with third-party terms preserved. The separate
MIT exception for explicitly offered TonePush regression tests does not change
HX Unbound's license. Tests must identify synthetic, recorded and hardware
evidence separately and report skipped/private-fixture checks honestly.
