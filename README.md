# HX Unbound

An independent preset editor and NAM capture librarian for Line 6 Helix and HX devices.

**Experimental source preview for Windows. Not a stability-certified editor.**
Development and hardware evidence currently target the original Helix Floor on
3.80 with custom A2 Lite firmware. Other devices and A2 Full playback are not
validated. This package does not build, install or distribute firmware.

## Screenshots

The current editor rendered with anonymous demonstration data. These are actual
interface screenshots, not device-connected test results. No personal presets,
capture files or extracted HX Edit artwork are included in these images.

![Dark-mode signal path and NAM model browser](docs/screenshots/signal-path-dark.png)

<details>
<summary>Light-mode NAM list and shared storage</summary>

![Light-mode NAM library and alphabetical model list](docs/screenshots/nam-library-light.png)

![Shared NAM and IR storage, including unidentified slots](docs/screenshots/shared-storage-dark.png)

</details>

## What is here

- An opt-in live signal-path editor, stock-model browser, parameter and bypass
  controls, block placement, preset navigation, snapshot controls and preset tools.
- A capture library with separate NAM/IR views, gear classification, named
  capture metadata and shared-slot storage accounting.
- The Python loopback server, Rust USB bridge, browser source, selected offline
  tests and pinned converter/transport dependencies.

Features have different verification levels. Read [current limitations](docs/STATUS.md)
before connecting hardware. In particular, some USB sequences have left the
panel frozen while audio continued. Successful command readback alone is not
proof that the panel is healthy. Standard HX Edit backups of custom model 380
have lost NAM blocks/settings; they are not a complete NAM recovery method.

The app always starts offline. **Device capture/IR installs, slot-sample reads
and verified NAM selection are disabled in this preview**, including in device
mode. Named NAM selection uses the same unresolved sample-read path. You can
import and classify captures locally; no transfer is needed for that. Existing
NAM firmware and captures must be managed separately. See [upstream work](docs/UPSTREAM.md).

## Start locally

Requires Windows and Python 3.11+. In PowerShell, from this folder:

```powershell
python -m venv .venv
.\.venv\Scripts\python.exe -m pip install -r requirements.txt
& '.\Launch HX Unbound.ps1'
```

It opens the local library, creates an empty database under `build/`, and logs
to `build/server-48736.*.log`. No Floor is needed. Use the NAM tab to import a
`.nam` file; engine compatibility and source metadata determine its label.
Unsupported architectures remain browse-only. The screenshots above show demo
data; a fresh installation starts empty.

To stop the server cleanly, use `& '.\Stop HX Unbound.ps1'`. It refuses shutdown
while an edit is pending. If the default port is occupied, launch with
`-Port 48749` and use the same port when stopping. `-NoBrowser` runs without
opening a desktop browser. The scripts do not change execution policy.

## Offline source checks

Python 3.11+ and Node.js are required for these checks. No device, installed
Codex runtime, capture library, downloaded capture or HX Edit artwork is required.

```powershell
python scripts/check_package.py
python scripts/verify_offline.py
```

The verification runner imposes per-command time limits and uses mocked device
operations. It does not launch the editor, open a browser or connect to USB.

## Build the bridge

Use a standard Rust toolchain (1.87+ and the Windows C++ build tools). From the
package root:

```powershell
cargo build --locked --manifest-path floor-session/Cargo.toml --bin floor-editor --release --target-dir build/floor-session/target
```

The three required TonePush crates are vendored with their MIT license and the
actual local modifications. No dependency points outside this package. The
initial Cargo dependency download requires internet access; builds contain no
device test. Optional recorded-hardware tests are not portable fixtures and
must not be counted as passed when skipped.

For explicit hardware testing, after building the bridge, close stock HX Edit
and start `& '.\Launch HX Unbound.ps1' -ExperimentalDevice`. The app still starts
offline; click Connect Floor to begin a session. Transfers remain disabled.
Keep monitoring low and test on a spare preset. Disconnect in the app before
ending a session. Source publication does not establish new device/firmware support.

The stock model browser reads metadata and optional artwork from the user's
installed HX Edit. These resources are not included in this repository or its
screenshots. Without HX Edit or the bridge, the stock catalog is unavailable;
the local NAM library remains usable with the app's own illustrations. No legacy
transfer librarian or separately downloaded TonePush executable is required for
this preview. See [contributing](CONTRIBUTING.md) for test/report expectations.

## Credit and licensing

HX Unbound's product and independent Floor testing are maintained by **Suddl3**.
The [Floor recall test](https://github.com/nocaseguy/hx-stomp-nam-lite/pull/11)
is merged upstream. Firmware foundations and transport dependencies retain
their own authorship: see [THIRD_PARTY.md](THIRD_PARTY.md).

HX Unbound is GPL-3.0-or-later; see [LICENSE](LICENSE). Original third-party
notices and licenses are retained. Line 6, Helix and HX are trademarks of their
respective owners. This project is independent of Line 6 and Yamaha Guitar Group.

No firmware images, personal backups, downloaded captures, user databases or
raw hardware session recordings belong in this source repository.
