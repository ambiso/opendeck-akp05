![Plugin Icon](assets/icon.png)

# OpenDeck Ajazz AKP05 / Mirabox N4 Plugin

An unofficial plugin for Mirabox N4-family devices

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

## Adding new devices

Read [this wiki page](https://github.com/4ndv/opendeck-akp03/wiki/Adding-support-for-new-devices) for more information.

## Building

### Prerequisites

You'll need:

- A Linux OS of some sort
- Rust 1.88 and up with `x86_64-unknown-linux-gnu` and `x86_64-pc-windows-gnu` targets installed
- gcc with Windows support
- Docker
- [just](https://just.systems)

On Arch Linux:

```sh
sudo pacman -S just mingw-w64-gcc mingw-w64-binutils
```

Adding rust targets:

```sh
rustup target add x86_64-pc-windows-gnu
rustup target add x86_64-unknown-linux-gnu
```

### Preparing environment

```sh
$ just prepare
```

This will build docker image for macOS crosscompilation

### Building a release package

```sh
$ just package
```

## Acknowledgments

This plugin is heavily based on work by contributors of [elgato-streamdeck](https://github.com/streamduck-org/elgato-streamdeck) crate

Further inspiration was taken from these sister repos:
- https://github.com/naerschhersch/opendeck-akp05
- https://github.com/GrauBlitz/opendeck-akp05
- https://github.com/maillota/opendeck-akp05

The icon was yoinked from https://github.com/naerschhersch/opendeck-akp05/
