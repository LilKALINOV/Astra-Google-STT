# Web Speech-to-Text — Free STT plugin for Astra

Speech recognition via Google Web Speech API — **free, no API key required, no registration, no setup**. Install the plugin and talk.

![icon](icon.svg)

## What it does

This plugin turns spoken audio into text. It uses the same free Web Speech API endpoint that Chrome and the Python `SpeechRecognition` library call, with the `client=chromium` handshake and no key.

Everything happens inside the plugin process: Astra hands it PCM audio, the plugin converts it to 16-bit little-endian, sends it to Google, parses the JSON transcript and returns it as an `SttEvent`. No daemon services are involved, so **no permissions are requested** — there is nothing for Astra to consent to.

## What it requires and why

| What | Why this plugin asks |
| --- | --- |
| Outbound network access to Google's speech servers (`www.google.com:80`) | The recognition request is sent from the plugin's own process to Google directly. Astra never sees the audio, the transcript or the network traffic. |

No `[permissions]` grants are declared, deliberately: the plugin calls none of the permission-gated host RPCs. A pure STT plugin needs nothing from the daemon beyond the `stt` capability itself.

## Configuration

All settings are optional — the plugin works out of the box with Russian as the default language.

| Setting | Required | Value |
| --- | --- | --- |
| Language | No (defaults to `ru-RU`) | The recognition language, picked from a dropdown of 38 supported locales: Russian, English (US/UK), Ukrainian, German, French, Spanish, Italian, Portuguese (BR), Japanese, Korean, Chinese (Simplified/Traditional), Arabic, Hindi, Turkish, Polish, Dutch, Swedish, Norwegian, Finnish, Danish, Greek, Hebrew, Romanian, Hungarian, Czech, Bulgarian, Croatian, Slovenian, Serbian, Slovak, Lithuanian, Latvian, Estonian, Indonesian, Thai, Vietnamese |
| Sample rate | No (defaults to 16000 Hz) | Audio sample rate in Hz. Normally let Astra's own rate win; override only if your source produces something else. |

> The language chosen in Astra's STT picker overrides this setting for that call.

## Limitations

- One request at a time; audio up to ~60 seconds per shot.
- Requires an internet connection — every request goes to Google's servers, and the transcript leaves your machine.
- The endpoint is the free, unofficial one. It is not a guaranteed SLA service; treat it as best-effort.
- Google may throttle or change the free endpoint at any time. That is the price of no key and no bill.

## Installation

1. In Astra: **Settings → Privacy** → enable **"Allow unsigned plugins"**
2. **Plugins → Dev** → paste the plugin folder path and click Load
3. Done. The plugin works immediately with no configuration.

## Build it yourself

```sh
cargo build --release
astra-plugin build
```

## Files

- `src/main.rs` — plugin lifecycle, config handling, f32→i16 PCM conversion, Google request and JSON parsing.
- `locales/` — English and Russian store-card text.
- `icon.svg` — store icon, drawn by hand.

Licensed MIT.
