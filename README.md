# Web Speech-to-Text – Astra Plugin

[![Version](https://img.shields.io/badge/version-0.6.0-blue.svg)](https://github.com/LilKALINOV/astra-websearch-ddg/releases)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)
[![Build Status](https://github.com/LilKALINOV/astra-websearch-ddg/actions/workflows/release.yml/badge.svg)](https://github.com/LilKALINOV/astra-websearch-ddg/actions/workflows/release.yml)
[![Astra Plugin](https://img.shields.io/badge/Astra-Plugin-purple)](https://github.com/astra-ai/astra)

**Free, keyless speech‑to‑text** directly inside your Astra AI assistant – powered by Google's Web Speech API, no registration or setup required.

---

![icon](icon.svg)

---

## 📖 Table of Contents

- [Features](#-features)
- [Why This Plugin?](#-why-this-plugin)
- [How It Works](#-how-it-works)
- [Requirements](#-requirements)
- [Installation](#-installation)
- [License](#-license)
- [Author](#-author)

---

## ✨ Features

- **Free and keyless** – Uses the public Google Web Speech endpoint with a built‑in shared key; no API key needed.
- **Wide language support** – 38 locales, including Russian, English (US/UK), Ukrainian, German, French, Spanish, Italian, Portuguese (BR), Japanese, Korean, Chinese (Simplified/Traditional), Arabic, Hindi, Turkish, Polish, Dutch, Swedish, Norwegian, Finnish, Danish, Greek, Hebrew, Romanian, Hungarian, Czech, Bulgarian, Croatian, Slovenian, Serbian, Slovak, Lithuanian, Latvian, Estonian, Indonesian, Thai, Vietnamese.
- **Configurable** – Adjust language, max phrase length, network timeout, and silence detection to end phrases early.
- **Bring your own key** – Optionally supply your own Google API key if the shared one is throttled.
- **Live streaming** – Processes audio chunk by chunk, with built‑in silence detection that can override Astra's own VAD.
- **Tool‑only design** – Integrates seamlessly as an STT engine; no extra UI panels.
- **Localised** – Plugin settings and store descriptions available in English, Russian, and Ukrainian (matching Astra's UI language).

---

## 🤔 Why This Plugin?

- Requires **no registration, no credit card, no API key** – it just works.
- Gives you **full control** over language, timeouts, and phrase cutting.
- Uses the same free endpoint that Google Chrome and the popular `SpeechRecognition` Python library call.
- Keeps your audio and transcripts **private** – Astra never sees the audio or the response; all traffic goes directly from the plugin process to Google's servers.

If you use voice dictation or want hands‑free interaction with your AI, this plugin is essential.

---

## ❗Limitations

- One request at a time; a phrase is limited to ~60 s (configurable, see Max phrase length).
- Requires an internet connection — every request goes to Google's servers, and the transcript leaves your machine.
- The endpoint is the free, unofficial one. It is not a guaranteed SLA service; treat it as best-effort.
- Google may throttle, require a personal key, or change the free endpoint at any time. That is the price of a free, keyless engine.

---

## 🔄 How It Works

Here’s a step‑by‑step breakdown of the recognition flow:

1. **Astra captures audio** – The assistant listens via microphone, runs its own VAD (voice‑activity detection) to detect speech and phrase boundaries.
2. **Audio streamed to plugin** – Once a phrase is detected, the raw PCM audio (f32 format) is streamed chunk by chunk to the plugin.
3. **Plugin detects silence** – The plugin monitors the stream live; if you have set a custom **Silence to end phrase** value, it will end the phrase as soon as that much silence is detected (overriding Astra's VAD if needed).
4. **Audio conversion & request** – When the phrase ends (either by Astra's signal or the plugin's own silence detection), the audio is converted from 32‑bit float to 16‑bit little‑endian PCM and posted to Google's speech endpoint (`www.google.com/speech-api`).
5. **JSON parsing** – The plugin parses the JSON response, extracts the transcript, and returns it as a final `SttEvent` to Astra.

> **Note:** If the default silence detection feels too long or too short, adjust the **Silence to end phrase** setting (in milliseconds) – the plugin will then end the phrase on its own after that much quiet.

---

## 📡 Requirements

| Resource | Purpose |
|----------|---------|
| **Outbound network access** to `www.google.com:80` | The recognition request is sent directly from the plugin process to Google. Astra never sees the audio, transcript, or network traffic. |
| **Astra Desktop Assistant** (v0.8.0 or later) | The plugin targets the Astra plugin SDK. |
| **Rust runtime** (if building manually) | The plugin is written in Rust; pre‑built binaries are provided via releases. |

One permission is declared up front, though it is not used in the current version:

| Permission | Why it is asked |
|------------|-----------------|
| `client` | Reserved for a future release that may send recognized text directly into an Astra chat (dictation). Declared in advance because adding a high‑risk permission later would require a new review. Today, the plugin only uses the `stt` capability and does not call any permission‑gated RPCs. |

No additional host‑side permissions are needed – the plugin runs with the default sandboxed capabilities.

---

## 📦 Installation

### From the Astra Catalogue (Recommended)

1. Open Astra → **Plugins** → **Browse**.
2. Search for "Web Speech-to-Text"
3. Click **Install** – the plugin will be added and ready to use.

### Manual / Development Installation

1. **Enable unsigned plugins** in Astra:  
   Settings → Privacy → **Allow unsigned plugins** (toggle on).
2. Download or clone the plugin source:
   ```bash
   git clone https://github.com/LilKALINOV/Astra-Google-STT.git
   cd Astra-Google-STT
   cargo build --release
   astra-plugin build
---

   ## 📄 License

This plugin is released under the **MIT License**.  See LICENSE for the full text.

---

## 👤 Author

**Lil KALINOV**  
GitHub: https://github.com/LilKALINOV  
Discord: bass_kalinov

---

## 🙏 Acknowledgements

- Google for providing the public Web Speech API endpoint.
- The Python `SpeechRecognition` library for inspiration and reference implementation.

Feel free to open issues or submit pull requests if you find bugs or have ideas for improvements.
