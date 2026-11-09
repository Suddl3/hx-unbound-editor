# Source provenance

| Component | Origin | Terms / local treatment |
|---|---|---|
| USB protocol, transport and catalog | [crmne/tonepush](https://github.com/crmne/tonepush), `dcdedcf1e45c5855e984e0ca552621e5463d5201` | MIT, retained under `vendor/tonepush/LICENSE`. Three crates only. `patches/tonepush-local.patch` records our complete local source delta; workspace manifest narrowed for this package. |
| A2 Lite converter and imported helpers | [savedadodo/helix-floor-nam-lite](https://github.com/savedadodo/helix-floor-nam-lite), `c7cd0bacaf7e08c0de2b5fedfccb05d13826e9f0`, derived from [nocaseguy/hx-stomp-nam-lite](https://github.com/nocaseguy/hx-stomp-nam-lite) | GPL-3.0-or-later. Original LICENSE and NOTICE retained under `vendor/nam`. No firmware builder is included. |
| Assembler opcode-table ancestry | [roblg/ghidra-sharc](https://github.com/roblg/ghidra-sharc) | Apache-2.0; original attribution is retained in the imported assembler and NAM NOTICE, with license text in `LICENSES/Apache-2.0.txt`. |
| Neural Amp Modeler format and algorithm | [sdatkinson/neural-amp-modeler](https://github.com/sdatkinson/neural-amp-modeler) | Follow upstream converter NOTICE; this package contains no downloaded trained captures. |
| HX Edit resources | User's installed application, read locally | Not bundled or relicensed. Public artwork strategy remains a release-review item. |

OpenHX and ToneRelay informed product/protocol research. They are not claimed as
HX Unbound inventions and their complete applications are not copied here.
Floor firmware support originates with savedadodo, based on nocaseguy's kernel
and tooling. The LT adaptation belongs to andreidevs. Our editor is a separate
product, not a claim to those ports.

`SOURCE-MANIFEST.json` records each included file's source, content hash and
packaging changes. Architecture-only test data uses generated zero weights;
the protocol binary fixtures originate in the pinned public TonePush archive,
not the owner's presets or backup files.
