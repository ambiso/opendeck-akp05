# Continuous strip on Windows

On the VSDinside N4 Pro (`5548:1023`), the plugin combines four independent encoder images into one 800 × 160 JPEG frame for the strip above the knobs. It sends the frame to the temporary framebuffer. Power cycling clears the temporary frame. The live display uses baseline quality-90 JPEG with full chroma.

## Enable the panel

Create `%APPDATA%\Interstitial\config.toml` and reload the device plugin. Start at the original alignment and adjust it to your viewing angle.

```toml
continuous_strip = true
strip_native_y = 0
strip_center_offset = 0
vibration = true
ambient_rings = true

[leds]
brightness = 50
```

In the companion Hardware Graph action, select **Appearance → Strip alignment** to move the picture between −30 and +30 pixels. Positive values move the content down. Set `continuous_strip = false` and reload to return to the firmware's individual display zones.

## Optional artwork and locking

Supply your own upright 800 × 480 PNG files in `%APPDATA%\Interstitial`. The base image is `live-wallpaper.png`. The optional `day-wallpaper.png` follows Windows light mode and `night-original.png` supplies a separate dark lock image. Reload after replacing these files.

With a valid base image and continuous mode enabled, a locked session shows only the themed wallpaper and suppresses inputs. Unlock restores the current key faces and strip. Without a base image the continuous strip can still show graphs; the wallpaper-only privacy behavior requires that image. The device observes Windows session, resume and theme notifications. If observation fails it uses one-second checks and retries registration every 30 seconds.

## Knob lights

With `ambient_rings = true` and the companion graph action, each ring follows its slot. Temperatures change from cool blue through teal and amber to warm red, with 2 °C of hysteresis. Memory and network use different palettes relative to the configured graph scales. Unavailable or expired readings become dim neutral. Lock turns the ambient lights off.

A static LED mode takes precedence. Configure it in the same file:

```toml
[leds.mode.Static]
colors = [[24, 98, 170], [42, 174, 181], [70, 159, 182], [19, 85, 88]]
```

The existing `leds.toml` override in the Interstitial configuration directory accepts `brightness` and `[mode.Static]` at its top level. Setting `ambient_rings = false` explicitly and reloading turns the rings off if there is no static palette. Omit the setting to leave the device's existing effect alone.

## Transport

The bundled Windows transport DLL is the original x64 SDK file at [this pinned revision](https://github.com/MiraboxSpace/StreamDock-Device-SDK/blob/fc124cb5bc8fe0cd6c72d3f2af80588adc7dcffb/CPP-SDK/src/Transport/TransportDLL/transport.dll). Its SHA-256 is `0FE8361D25A2C1B6D401C6156900758D346B6ECBE5772621E6EC8C81B445EB88`. The live strip uses volatile framebuffer operations. The packager includes the matching transport and JPEG notices.
