use astra_plugin_sdk::prelude::*;

use reqwest::Client;
use std::time::{Duration, Instant};

/// Mark a string as a declared-plane locale key for the daemon to resolve in
/// the user's current UI language. Equals `i18n::key()` from newer SDKs.
fn i18n_key(k: &str) -> String {
    format!("${k}")
}

// Chromium's public key used by the free Web Speech API when no user key
// is configured. Free and unauthenticated, but shared: Google can throttle
// it. Users can set their own key in the plugin config.
const DEFAULT_API_KEY: &str = "AIzaSyBOti4mM-6x9WDnZIjIeyEU21OpBXqWBgw";

/// Root-mean-square energy threshold (out of 1.0 scaled f-32 PCM) above which a
/// chunk counts as speech for the plugin's own silence detection.
const VAD_RMS: f32 = 0.01;

/// Transcribe f32-LE PCM through Google's free Web Speech endpoint.
/// Returns `(best_text, confidence)`. Empty key → built-in chromium key.
async fn google_transcribe(
    cfg: &GoogleSttConfig,
    audio: &[u8],
    sample_rate: u32,
    language: &str,
) -> anyhow::Result<(String, f32)> {
    // Astra SDK sends f32 LE PCM — convert to i16 LE PCM for Google
    let f32_count = audio.len() / 4;
    let mut max_frames = f32_count;
    if cfg.max_audio_secs > 0 {
        let cap = (cfg.max_audio_secs as usize).saturating_mul(sample_rate as usize);
        if cap < max_frames {
            max_frames = cap;
            eprintln!(
                "[web-stt] Truncating {}s of audio to the configured {}-second limit",
                f32_count as f64 / sample_rate as f64,
                cfg.max_audio_secs
            );
        }
    }
    let mut i16_pcm = Vec::with_capacity(max_frames * 2);
    for i in 0..max_frames {
        let bytes = [
            audio[i * 4],
            audio[i * 4 + 1],
            audio[i * 4 + 2],
            audio[i * 4 + 3],
        ];
        let sample_f32 = f32::from_le_bytes(bytes);
        let clamped = sample_f32.clamp(-1.0, 1.0);
        let sample_i16 = (clamped * 32767.0) as i16;
        i16_pcm.extend_from_slice(&sample_i16.to_le_bytes());
    }

    let client = Client::new();
    let url = "http://www.google.com/speech-api/v2/recognize";

    // A non-empty key field, even a single character, is used as-is: the user
    // explicitly wants their own key. Whitespace-only counts as empty.
    let key_value = cfg.api_key.trim();
    let (key, key_label) = if key_value.is_empty() {
        (DEFAULT_API_KEY, "built-in chromium key")
    } else {
        (key_value, "personal key")
    };
    let tail = |k: &str| -> String {
        let chars: Vec<char> = k.chars().collect();
        if chars.len() > 10 {
            let head: String = chars.iter().take(2).collect();
            let end: String = chars[chars.len() - 8..].iter().collect();
            format!("{}…{}", head, end)
        } else {
            k.to_string()
        }
    };
    eprintln!(
        "[web-stt] Request using {} ({})",
        key_label,
        if key_label == "personal key" {
            tail(key)
        } else {
            "google's free chromium key".to_string()
        }
    );

    let timeout_secs = if cfg.request_timeout_secs == 0 {
        10
    } else {
        cfg.request_timeout_secs
    };

    let resp = match client
        .post(url)
        .query(&[
            ("client", "chromium"),
            ("lang", language),
            ("key", key),
            ("pFilter", "0"),
        ])
        .header(
            "Content-Type",
            format!("audio/l16; rate={}; endian=little", sample_rate),
        )
        .body(i16_pcm)
        .timeout(Duration::from_secs(timeout_secs as u64))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[web-stt] HTTP request failed ({}): {}", key_label, e);
            return Err(e.into());
        }
    };

    let status = resp.status();
    let text = match resp.text().await {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[web-stt] Failed to read response ({}): {}", key_label, e);
            return Err(e.into());
        }
    };

    if !status.is_success() {
        eprintln!(
            "[web-stt] Google rejected the request ({}): HTTP {} — {}",
            key_label, status, text
        );
        return Err(anyhow::anyhow!(
            "Google Speech API error ({}): {}",
            status,
            text
        ));
    }

    // Google returns newline-separated JSON — parse each line
    let mut best_text = String::new();
    let mut best_confidence = 0.0f32;
    let mut saw_error = false;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(line_val) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(error) = line_val.get("error") {
                let msg = error
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown");
                let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
                eprintln!(
                    "[web-stt] Google error ({}): code={} msg={}",
                    key_label, code, msg
                );
                saw_error = true;
                continue;
            }
            if let Some(results) = line_val.get("result").and_then(|r| r.as_array()) {
                for result in results {
                    if let Some(alts) = result.get("alternative").and_then(|a| a.as_array()) {
                        for alt in alts {
                            let transcript = alt
                                .get("transcript")
                                .and_then(|t| t.as_str())
                                .unwrap_or("")
                                .to_string();
                            let confidence = alt
                                .get("confidence")
                                .and_then(|c| c.as_f64())
                                .unwrap_or(0.5) as f32;
                            if confidence > best_confidence {
                                best_text = transcript;
                                best_confidence = confidence;
                            }
                        }
                    }
                }
            }
        }
    }

    // An explicit "error" JSON means the key or the request was rejected —
    // surface it instead of silently returning an empty transcript.
    if saw_error {
        return Err(anyhow::anyhow!(
            "Google Speech API rejected the request ({}): {}",
            key_label,
            text
        ));
    }

    if best_text.is_empty() {
        eprintln!(
            "[web-stt] No transcript in the answer ({}) — check the key or retry",
            key_label
        );
    }

    Ok((best_text, best_confidence))
}

/// Approximate the loudness of a chunk of f32-LE PCM as an RMS value in 0..=1.
fn chunk_rms(chunk: &[u8]) -> f32 {
    let frames = chunk.len() / 4;
    if frames == 0 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    for i in 0..frames {
        let bytes = [
            chunk[i * 4],
            chunk[i * 4 + 1],
            chunk[i * 4 + 2],
            chunk[i * 4 + 3],
        ];
        let s = f32::from_le_bytes(bytes).clamp(-1.0, 1.0) as f64;
        sum += s * s;
    }
    (sum / frames as f64).sqrt() as f32
}

#[derive(Debug, Clone, Default)]
#[astra::config]
struct GoogleSttConfig {
    #[serde(default)]
    language_code: String,
    #[serde(default)]
    sample_rate_hz: u32,
    /// Optional personal Google API key for https://www.google.com/speech-api.
    /// Any non-empty value (even a single character) is used as the key as-is;
    /// empty (or whitespace-only) means: use the built-in free chromium key.
    #[serde(default)]
    api_key: String,
    /// Maximum utterance length in seconds per one recognition request.
    /// Google rejects audio longer than ~60 s. 0 means: no explicit limit.
    #[serde(default)]
    max_audio_secs: u32,
    /// How many seconds to wait for Google before giving up.
    #[serde(default)]
    request_timeout_secs: u32,
    /// How many milliseconds of trailing silence end an utterance.
    /// The plugin watches the incoming audio itself and, once it has been
    /// quiet for this long, sends the accumulated phrase as its final
    /// transcript instead of waiting for the daemon to cut the stream.
    /// 0 means: rely on the daemon's own silence detection.
    #[serde(default)]
    silence_timeout_ms: u32,
}

#[derive(Default)]
struct GoogleStt {
    config: Config<GoogleSttConfig>,
}

#[astra::plugin]
impl GoogleStt {
    #[hook]
    async fn on_config(&self, _ctx: &PluginContext, config: GoogleSttConfig) {
        let lang = if config.language_code.is_empty() {
            "ru-RU"
        } else {
            &config.language_code
        };
        let key = if config.api_key.trim().is_empty() {
            "built-in chromium key".to_string()
        } else {
            let tail: String = config
                .api_key
                .trim()
                .chars()
                .rev()
                .take(4)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            format!("personal key …{}", tail)
        };
        let max = if config.max_audio_secs == 0 {
            "unlimited".to_string()
        } else {
            format!("{}s", config.max_audio_secs)
        };
        let timeout = if config.request_timeout_secs == 0 {
            "10s".to_string()
        } else {
            format!("{}s", config.request_timeout_secs)
        };
        let silence = if config.silence_timeout_ms == 0 {
            "daemon-managed".to_string()
        } else {
            format!("{}ms", config.silence_timeout_ms)
        };
        eprintln!(
            "[web-stt] Config loaded: language={}, sample_rate={}, key={}, max_audio_secs={}, request_timeout={}, silence={}",
            lang, config.sample_rate_hz, key, max, timeout, silence
        );
    }

    /// Recognize speech using Google's free Web Speech API (no key needed).
    #[hook]
    async fn stt_transcribe(
        &self,
        _ctx: &PluginContext,
        audio: &[u8],
        sample_rate: u32,
        options: &SttOptions,
    ) -> anyhow::Result<SttEvent> {
        if audio.is_empty() {
            return Ok(SttEvent {
                text: String::new(),
                is_final: true,
                confidence: 0.0,
                language: options.language.clone(),
            });
        }

        let cfg = self.config.load();

        let language = if options.language.is_empty() {
            if cfg.language_code.is_empty() {
                "ru-RU".to_string()
            } else {
                cfg.language_code.clone()
            }
        } else {
            options.language.clone()
        };

        let rate = if sample_rate > 0 {
            sample_rate
        } else if cfg.sample_rate_hz > 0 {
            cfg.sample_rate_hz
        } else {
            16000
        };

        let (text, confidence) = google_transcribe(&cfg, audio, rate, &language).await?;

        Ok(SttEvent {
            text,
            is_final: true,
            confidence,
            language,
        })
    }

    /// Streaming recognition with in-plugin silence detection.
    ///
    /// The daemon decides when an utterance ends and closes the channel;
    /// `silence_timeout_ms` lets the plugin end it *earlier* once the incoming
    /// audio has been quiet long enough (useful when the daemon's own silence
    /// window is longer than the configured one). The final event is always
    /// emitted.
    #[hook]
    async fn stt_transcribe_stream(
        &self,
        _ctx: &PluginContext,
        mut audio: tokio::sync::mpsc::Receiver<Vec<u8>>,
        events: tokio::sync::mpsc::Sender<SttEvent>,
        sample_rate: u32,
        options: SttOptions,
    ) -> anyhow::Result<()> {
        let cfg = self.config.load();

        let language = if options.language.is_empty() {
            if cfg.language_code.is_empty() {
                "ru-RU".to_string()
            } else {
                cfg.language_code.clone()
            }
        } else {
            options.language.clone()
        };

        let rate = if sample_rate > 0 {
            sample_rate
        } else if cfg.sample_rate_hz > 0 {
            cfg.sample_rate_hz
        } else {
            16000
        };

        let silence_timeout = if cfg.silence_timeout_ms == 0 {
            None
        } else {
            Some(Duration::from_millis(cfg.silence_timeout_ms as u64))
        };

        let mut buf: Vec<u8> = Vec::new();
        let mut last_voice = Instant::now();

        loop {
            match audio.recv().await {
                Some(c) => {
                    buf.extend_from_slice(&c);
                    if chunk_rms(&c) >= VAD_RMS {
                        last_voice = Instant::now();
                    }
                }
                // Channel closed = end of utterance (daemon's own VAD won).
                None => break,
            }

            // In-plugin endpointing: when the buffered audio has been quiet for
            // `silence_timeout`, finish the phrase now and end the stream — the
            // SDK contract is one final transcript per utterance, and returning
            // early is exactly what "wait for this much silence, then send"
            // means. When disabled, rely on the daemon closing the channel.
            if let Some(to) = silence_timeout {
                if !buf.is_empty() && last_voice.elapsed() >= to {
                    let (text, confidence) = google_transcribe(&cfg, &buf, rate, &language).await?;
                    let _ = events
                        .send(SttEvent {
                            text,
                            is_final: true,
                            confidence,
                            language,
                        })
                        .await;
                    return Ok(());
                }
            }
        }

        // Final transcript for whatever the daemon last had us ingest — the
        // part that never hit the silence threshold above.
        if !buf.is_empty() {
            let (text, confidence) = google_transcribe(&cfg, &buf, rate, &language).await?;
            let _ = events
                .send(SttEvent {
                    text,
                    is_final: true,
                    confidence,
                    language,
                })
                .await;
        }

        Ok(())
    }

    #[hook]
    async fn stt_languages(&self) -> Vec<String> {
        vec![
            "ru-RU".into(),
            "en-US".into(),
            "en-GB".into(),
            "uk-UA".into(),
            "de-DE".into(),
            "fr-FR".into(),
            "es-ES".into(),
            "it-IT".into(),
            "pt-BR".into(),
            "ja-JP".into(),
            "ko-KR".into(),
            "zh-CN".into(),
            "ar-SA".into(),
            "hi-IN".into(),
            "tr-TR".into(),
            "pl-PL".into(),
            "nl-NL".into(),
            "sv-SE".into(),
            "no-NO".into(),
            "fi-FI".into(),
            "da-DK".into(),
            "zh-TW".into(),
            "id-ID".into(),
            "th-TH".into(),
            "vi-VN".into(),
            "cs-CZ".into(),
            "el-GR".into(),
            "he-IL".into(),
            "ro-RO".into(),
            "hu-HU".into(),
            "sk-SK".into(),
            "bg-BG".into(),
            "hr-HR".into(),
            "sl-SI".into(),
            "sr-RS".into(),
            "lt-LT".into(),
            "lv-LV".into(),
            "et-EE".into(),
        ]
    }

    /// Config fields — language, optional API key and recognition limits.
    /// Labels/descriptions are declared-plane keys the daemon resolves in the
    /// user's current language from `locales/{en,ru,uk}.json`.
    #[hook]
    async fn stt_config_fields(&self) -> Vec<FieldDef> {
        vec![
            FieldDef::dropdown(
                "language_code",
                i18n_key("config.language.label"),
                &[
                    ("ru-RU", "Русский"),
                    ("en-US", "English (US)"),
                    ("en-GB", "English (UK)"),
                    ("uk-UA", "Українська"),
                    ("de-DE", "Deutsch"),
                    ("fr-FR", "Français"),
                    ("es-ES", "Español"),
                    ("it-IT", "Italiano"),
                    ("pt-BR", "Português (BR)"),
                    ("ja-JP", "日本語"),
                    ("ko-KR", "한국어"),
                    ("zh-CN", "中文"),
                    ("ar-SA", "العربية"),
                    ("hi-IN", "हिन्दी"),
                    ("tr-TR", "Türkçe"),
                    ("pl-PL", "Polski"),
                    ("nl-NL", "Nederlands"),
                    ("sv-SE", "Svenska"),
                    ("no-NO", "Norsk"),
                    ("fi-FI", "Suomi"),
                    ("da-DK", "Dansk"),
                    ("el-GR", "Ελληνικά"),
                    ("he-IL", "עברית"),
                    ("ro-RO", "Română"),
                    ("hu-HU", "Magyar"),
                    ("cs-CZ", "Čeština"),
                    ("bg-BG", "Български"),
                    ("hr-HR", "Hrvatski"),
                    ("sl-SI", "Slovenščina"),
                    ("sr-RS", "Српски"),
                    ("sk-SK", "Slovenčina"),
                    ("lt-LT", "Lietuvių"),
                    ("lv-LV", "Latviešu"),
                    ("et-EE", "Eesti"),
                    ("id-ID", "Bahasa Indonesia"),
                    ("th-TH", "ไทย"),
                    ("vi-VN", "Tiếng Việt"),
                    ("zh-TW", "中文 (繁體)"),
                ],
            )
            .with_default("ru-RU"),
            FieldDef::text("api_key", i18n_key("config.api_key.label"))
                .with_placeholder(i18n_key("config.api_key.placeholder"))
                .with_description(i18n_key("config.api_key.description")),
            FieldDef::number("max_audio_secs", i18n_key("config.max_audio_secs.label"))
                .with_default("55")
                .with_min(1.0)
                .with_max(60.0)
                .with_step(1.0)
                .with_description(i18n_key("config.max_audio_secs.description")),
            FieldDef::number(
                "request_timeout_secs",
                i18n_key("config.timeout_secs.label"),
            )
            .with_default("10")
            .with_min(1.0)
            .with_max(60.0)
            .with_step(1.0)
            .with_description(i18n_key("config.timeout_secs.description")),
            FieldDef::number(
                "silence_timeout_ms",
                i18n_key("config.silence_timeout_ms.label"),
            )
            .with_default("0")
            .with_min(0.0)
            .with_max(3000.0)
            .with_step(50.0)
            .with_description(i18n_key("config.silence_timeout_ms.description")),
        ]
    }
}

astra::main!(GoogleStt::default());
