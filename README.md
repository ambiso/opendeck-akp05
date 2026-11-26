![Plugin Icon](assets/icon.png)

# OpenDeck Ajazz AKP05 / Mirabox N4 Plugin

An unofficial plugin for Mirabox N4-family devices

## Community

Questions, troubleshooting, or sharing your setup? [Join the Signal group](https://signal.group/#CjQKIHyUkzhTbUGZIeMoFhrnPfTglV_zx8x2wfO7oyA6uthqEhA7ZNnwF_EUd5XmaNIVPtEN).

## OpenDeck version

Requires OpenDeck 2.5.0 or newer

## Supported devices

- Mirabox N4E (6603:1007)
- Mirabox N4 (6602:1001)
- Mirabox N4 Pro E (5548:1021)
- Mirabox N4 Pro (5548:1008)
- Ajazz AKP05E (0300:3004)
- Ajazz AKP05E Pro (0300:3013)
- Ajazz AKP05 (0300:3006)
- VSDInside N4 (5548:1004)
- VSDInside N4 Pro (5548:1023)
- Mars Gaming MSD-Pro (0B00:1003)
- Soomfon CN003 (1500:3002)
- Streonor S10 Pro (1500:3007)
- Redragon SS552 (0200:3001)

## Platform support

- Linux: Guaranteed, if stuff breaks - I'll probably catch it before public release
- Mac: Zero effort, no tests before release, if stuff breaks - too bad, it's up to you to contribute fixes
- Windows: Zero effort, no tests before release, if stuff breaks - too bad, it's up to you to contribute fixes

## Installation

1. Download an archive from [releases](https://github.com/ambiso/opendeck-akp05/releases)
2. In OpenDeck: Plugins -> Install from file
3. Download [udev rules](./40-opendeck-akp05.rules) and install them by copying into `/etc/udev/rules.d/` and running `sudo udevadm control --reload-rules`
4. Unplug and plug again the device, restart OpenDeck

## Configuration

Create `~/.config/opendeck-akp05/config.toml`.
(Windows: `%APPDATA%\opendeck-akp05\config.toml`, macOS: `~/Library/Application Support/opendeck-akp05/config.toml`)

```toml
# Uncomment to test the experimental full-width touch bar on your deck.
# Please report your USB VID:PID, exact firmware version, and result.
# See "Report a compatibility test" below for how to find them.
# bgpic = true

# N4 Pro-family only. Omit to leave the device's current setting alone.
vibration = false

[leds]
brightness = 100 # 0-100

[leds.mode.Static]
colors = [[255, 0, 128]] # RGB — one entry per knob, missing entries repeat the last value
```

By default no LED commands are sent, so the device keeps its own built-in effect. Each LED a different color:

```toml
[leds]
brightness = 100

[leds.mode.Static]
colors = [
    [255, 0,   0  ], # RGB knob 1
    [0,   255, 0  ], # RGB knob 2
    [0,   0,   255], # RGB knob 3
    [255, 255, 0  ], # RGB knob 4
]
```

When OpenDeck is being terminated, a disconnect signal is sent to the device, which results in a hardcoded red for all knobs.

### `leds.toml` override

An optional `~/.config/opendeck-akp05/leds.toml` in the top-level LED schema (without the `[leds]` prefix) overrides just the LED section of `config.toml`:

```toml
brightness = 100

[mode.Static]
colors = [[255, 0, 128]]
```

### Vibration

Some N4 Pro-family devices (N4 Pro, N4 Pro E, VSDInside N4 Pro) vibrate on key press. Set `vibration = false` in `config.toml` to silence it, `true` to explicitly enable it, or omit the field to leave the current setting alone. The setting is applied on each connect. Devices that don't expose the config command in their firmware will log a warning and skip it.

### Experimental continuous strip

The plugin enables the full-width strip when it can read a firmware version that meets the device's minimum.

| Device | USB VID:PID | Minimum firmware | Full-width image slot |
| --- | --- | --- | --- |
| Mirabox N4E | `6603:1007` | `02.014` | `BAT` wire slot 4 |
| Streonor S10 Pro | `1500:3007` | `02.017` | `BAT` wire slot 5 |
| VSDInside N4 Pro | `5548:1023` | `02.010` | `BGPIC` 800×160, panel rows 320–479 |

The VSDInside N4 Pro draws its keys and touch strip on one 800×480 panel. The strip is the panel's bottom 160 rows, and about the top 24 of those sit behind the bezel, so dial images are drawn in the visible 800×136 area.

Set `bgpic = true` at the top level of `config.toml` to try `BGPIC` on any deck recognized by the plugin, including when the firmware version is unavailable. Set `bgpic = false` to disable `BGPIC` or omit it to use the firmware rules above. Restart OpenDeck or reconnect the deck to apply the setting.

#### Report a compatibility test

1. Enable `bgpic`, restart OpenDeck, and test an action on the touch bar. Note whether the rotary dial images appear in the right place, are clipped, or are missing.
2. In OpenDeck, choose **Settings → Open logs**. Open `plugins/com.github.ambiso.opendeck-akp05.sdPlugin.log` and copy the line beginning with `Using full-width touch strip`. It contains the USB VID:PID and firmware version when available, for example `6603:1007, firmware V3.Mbox_N4E.02.014`. The default log level includes this line.
3. On Windows, the plugin reports `firmware unknown`. With the deck connected, open VSD Craft or Stream Dock and copy the current firmware version from **Settings → Device**. Close the vendor app before testing in OpenDeck.
4. [Open a compatibility report](https://github.com/ambiso/opendeck-akp05/issues/new) with the log line, any firmware version you looked up separately, and what appeared on the touch bar.

On Linux, you can also find the log line from a terminal:

```sh
grep -F 'Using full-width touch strip' \
    "${XDG_DATA_HOME:-$HOME/.local/share}/opendeck/logs/plugins/com.github.ambiso.opendeck-akp05.sdPlugin.log"
```

If the plugin did not log the device, run `lsusb` with the deck connected and copy the value after `ID` on its entry. If the entry is ambiguous, compare the output with the deck unplugged.

## Adding new devices

Read [this wiki page](https://github.com/4ndv/opendeck-akp03/wiki/Adding-support-for-new-devices) for more information.

## Building

### Prerequisites

Install Nix with flakes enabled. The pinned flake supplies Rust (1.88 or newer),
linkers, test tools, and packaging tools; Docker and manually installed cross
compilers are not required.

```sh
nix develop
nix flake check --print-build-logs
```

### Release executables

On Linux:

```sh
just build-linux # Fully static musl binary; no host glibc or Nix store required
just build-win   # Cross-compiled x86-64 Windows executable
just build-mac   # Cross-compiled universal macOS executable (Intel + ARM)
```

`just build-mac` also works on macOS using the same pinned SDK/Zig toolchain:

```sh
just build-mac   # Universal binary containing both x86-64 and arm64
```

These commands put executables in `build/binaries/`. To assemble a release
entirely on Linux, run `just build-all`, then `just package`. The result is
`build/opendeck-akp05.plugin.zip`; packaging rejects missing platforms and an
existing output file. Remove or rename an old ZIP before packaging again.

The Linux-to-macOS build uses Nix-provided cargo-zigbuild and a hash-pinned
macOS 11.3 SDK, matching the previous Docker toolchain. It compiles and checks
both binary architectures locally; executing macOS binaries still requires macOS.

CI uses Nix for every build and check, cross-compiles all release executables
on a Linux runner, then combines them into one ZIP.
Tag pushes publish that ZIP as a GitHub release.

The sandboxed audit uses the advisory database pinned in `flake.lock` and skips
yanked-crate checks, which require registry network access. Update the database
with `nix flake update advisory-db`. The exception lives only in the Nix audit
derivation; ordinary `cargo audit` retains its default yanked-crate checks.

## Acknowledgments

This plugin is heavily based on work by contributors of [elgato-streamdeck](https://github.com/streamduck-org/elgato-streamdeck) crate

Further inspiration was taken from these sister repos:
- https://github.com/naerschhersch/opendeck-akp05
- https://github.com/GrauBlitz/opendeck-akp05
- https://github.com/maillota/opendeck-akp05

The icon was yoinked from https://github.com/naerschhersch/opendeck-akp05/
