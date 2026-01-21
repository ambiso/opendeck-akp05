![Plugin Icon](assets/icon.png)

# OpenDeck Ajazz AKP05 / Mirabox N4 Plugin

An unofficial plugin for Mirabox N4-family devices

## OpenDeck version

Requires OpenDeck 2.5.0 or newer

## Supported devices

- Mirabox N4 (6603:1007)
- Ajazz AKP05E (0300:3004)
- VSDInside N4 Pro (5548:1023)
- Mars Gaming MSD-Pro (0B00:1003)
- Soomfon CN003 (1500:3002)

## Platform support

- Linux: Guaranteed, if stuff breaks - I'll probably catch it before public release
- Mac: Zero effort, no tests before release, if stuff breaks - too bad, it's up to you to contribute fixes
- Windows: Zero effort, no tests before release, if stuff breaks - too bad, it's up to you to contribute fixes

## Installation

1. Download an archive from [releases](https://github.com/4ndv/opendeck-akp03/releases)
2. In OpenDeck: Plugins -> Install from file
3. Download [udev rules](./40-opendeck-akp03.rules) and install them by copying into `/etc/udev/rules.d/` and running `sudo udevadm control --reload-rules`
4. Unplug and plug again the device, restart OpenDeck

## Adding new devices

Read [this wiki page](https://github.com/4ndv/opendeck-akp03/wiki/Adding-support-for-new-devices) for more information.

## Building

### Prerequisites

You'll need:

- A Linux OS of some sort
- Rust 1.87 and up with `x86_64-unknown-linux-gnu` and `x86_64-pc-windows-gnu` targets installed
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
