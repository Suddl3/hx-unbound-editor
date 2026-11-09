# Upstream reconciliation

Reviewed 2026-11-08. HX Unbound owns its editor/librarian; firmware and shared
transport changes belong with their maintained projects. This source candidate
still uses the dependency revisions in `THIRD_PARTY.md`; the comparison below
does not silently upgrade the installed editor or firmware.

| Area | Upstream status | HX Unbound position |
|---|---|---|
| Floor interrupt and PM allocation guards | [Floor `29aea81`](https://github.com/savedadodo/helix-floor-nam-lite/commit/29aea8169d93151b25db61bf1140969de9d7b916) already contains the core fixes | No duplicate patch. Local soak/recall evidence remains useful corroboration. |
| DMA abort cleanup | A bounded wait can expire before descriptor cleanup | A focused patch and simulator regression are prepared locally, unsubmitted and hardware untested. They are not included in a firmware image here. |
| Live USB notifications | Open [TonePush #89](https://github.com/crmne/tonepush/pull/89) and [#85](https://github.com/crmne/tonepush/pull/85) overlap with local fixes | Support existing work. Additional exact-delivery/ACK-window tests are prepared for #89; no claim it has been integrated or fixes Floor freezes. |
| Preset preservation | TonePush main `fb43969` preserves unreadable native preset data; Floor maintainer reports model-555 compatibility work | Rebase and migration tests remain necessary. Stock HX Edit backup/restore is not a verified custom-NAM backup path. |
| IR transfer completion | Floor profile still differs from Stomp; [#86](https://github.com/crmne/tonepush/issues/86) reports a similar symptom on Stomp XL | Persistent Floor transfer screen remains unresolved. Do not enable Stomp-specific behavior by analogy. |
| LT | [andreidevs' LT port](https://github.com/andreidevs/helix-lt-nam/tree/c94834768546393420692794fdbb27c9cc31d718) adapts the Floor implementation | Not independently validated by HX Unbound. |

Offline comparison: three synthetic notification checks fail on TonePush main
and pass on #89's reviewed `496e73c` head. Its protocol/USB library suite passed
188 tests with those checks included. This did not execute the upstream GUI or
hardware/replay integration tests, or enable Stomp-only features on Floor.

The focused Floor candidate passed 14 kernel regression stages over 3,314
simulated calls and default-layout checks. Simulated drain status and instruction
counts do not establish physical freeze causation or hardware DSP capacity.
Single-NAM hardware timings agree independently with the Floor maintainer's
roughly 52.8k-cycle readings; no valid paired-meter or A2 Full capacity claim is
made. The local playing-image allocation cost and current upstream cost differ.

Suddl3's Floor recall/audio evidence is already merged as
[NAM PR #11](https://github.com/nocaseguy/hx-stomp-nam-lite/pull/11).
The remaining patches/tests above are prepared, not submitted or merged.
The owner approved MIT for the specific proposed TonePush test contributions;
HX Unbound remains GPL-3.0-or-later, with upstream notices retained.
