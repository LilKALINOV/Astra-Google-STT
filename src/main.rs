use astra_plugin_sdk::prelude::*;

use reqwest::Client;
use std::time::Duration;

/// Mark a string as a declared-plane locale key for the daemon to resolve in
/// the user's current UI language. Equals `i18n::key()` from newer SDKs.
fn i18n_key(k: &str) -> String {
    format!("${k}")
}

// Chromium's public key used by the free Web Speech API when no user key
// is configured. Free and unauthenticated, but shared: Google can throttle
// it. Users can set their own key in the plugin config.
const DEFAULT_API_KEY: &str = "AIzaSyBOti4mM-6x9WDnZIjIeyEU21OpBXqWBgw";

/// Maximum audio chunk duration in seconds sent in one Google Speech API request.
/// Google rejects audio longer than ~60 seconds. We chunk into 50-second segments.
const MAX_CHUNK_SECS: u32 = 50;

fn default_timeout() -> u32 {
    10
}

#[derive(Debug, Clone, Default)]
#[astra::config]
struct GoogleSttConfig {
    #[serde(default)]
    language_code: String,
    #[serde(default)]
    sample_rate_hz: u32,
    /// Optional personal Google API key for https://www.google.com/speech-api.
    /// Configured via plugin settings (plugin.toml [config]).
    /// Any non-empty value is used as the key as-is;
    /// empty (or whitespace-only) means: use the built-in free chromium key.
    #[serde(default)]
    api_key: String,
    /// How many seconds to wait for Google before giving up.
    #[serde(default = "default_timeout")]
    request_timeout_secs: u32,
}

/// Transcribe a single chunk of f32-LE PCM (up to ~50s) through Google's Web Speech endpoint.
/// Returns `(best_text, confidence)`. Empty key → built-in chromium key.
async fn google_transcribe_chunk(
    cfg: &GoogleSttConfig,
    audio: &[u8],
    sample_rate: u32,
    language: &str,
) -> anyhow::Result<(String, f32)> {
    if audio.is_empty() {
        return Ok((String::new(), 0.0));
    }

    let f32_count = audio.len() / 4;
    let mut i16_pcm = Vec::with_capacity(f32_count * 2);
    for i in 0..f32_count {
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

/// Transcribe f32-LE PCM through Google's free Web Speech endpoint.
/// Automatically chunks audio into segments up to MAX_CHUNK_SECS (50s)
/// so audio longer than 1 minute is safely recognized without truncation.
async fn google_transcribe(
    cfg: &GoogleSttConfig,
    audio: &[u8],
    sample_rate: u32,
    language: &str,
) -> anyhow::Result<(String, f32)> {
    if audio.is_empty() {
        return Ok((String::new(), 0.0));
    }

    let chunk_limit_bytes = (sample_rate as usize)
        .saturating_mul(MAX_CHUNK_SECS as usize)
        .saturating_mul(4);

    if audio.len() <= chunk_limit_bytes {
        return google_transcribe_chunk(cfg, audio, sample_rate, language).await;
    }

    eprintln!(
        "[web-stt] Audio duration ({:.1}s) exceeds chunk limit ({}s); splitting into multiple segments",
        audio.len() as f64 / (sample_rate as f64 * 4.0),
        MAX_CHUNK_SECS
    );

    let mut full_text = String::new();
    let mut total_confidence = 0.0f32;
    let mut count = 0usize;

    for chunk in audio.chunks(chunk_limit_bytes) {
        let (part_text, conf) = google_transcribe_chunk(cfg, chunk, sample_rate, language).await?;
        let trimmed = part_text.trim();
        if !trimmed.is_empty() {
            if !full_text.is_empty() {
                full_text.push(' ');
            }
            full_text.push_str(trimmed);
            total_confidence += conf;
            count += 1;
        }
    }

    let avg_confidence = if count > 0 {
        total_confidence / (count as f32)
    } else {
        0.0
    };

    Ok((full_text, avg_confidence))
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
        let timeout = if config.request_timeout_secs == 0 {
            "10s".to_string()
        } else {
            format!("{}s", config.request_timeout_secs)
        };
        eprintln!(
            "[web-stt] Config loaded: language={}, sample_rate={}, key={}, request_timeout={}",
            lang, config.sample_rate_hz, key, timeout
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

    /// Streaming recognition with automatic chunking for audio up to 1 minute.
    ///
    /// The plugin buffers incoming audio and relies on Astra daemon's VAD to signal
    /// the end of an utterance. If the speech exceeds 50 seconds, it sends the segment
    /// to Google, yields an interim transcript event, and continues streaming.
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

        let chunk_limit_bytes = (rate as usize)
            .saturating_mul(MAX_CHUNK_SECS as usize)
            .saturating_mul(4);

        let mut buf: Vec<u8> = Vec::new();
        let mut full_transcript = String::new();
        let mut total_confidence = 0.0f32;
        let mut segments_count = 0usize;

        while let Some(chunk) = audio.recv().await {
            buf.extend_from_slice(&chunk);

            // Once the accumulated buffer reaches 50s, send it to Google
            if buf.len() >= chunk_limit_bytes {
                let chunk_to_send = std::mem::take(&mut buf);
                match google_transcribe_chunk(&cfg, &chunk_to_send, rate, &language).await {
                    Ok((text, conf)) => {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            if !full_transcript.is_empty() {
                                full_transcript.push(' ');
                            }
                            full_transcript.push_str(trimmed);
                            total_confidence += conf;
                            segments_count += 1;
                            let avg_conf = total_confidence / (segments_count as f32);
                            let _ = events
                                .send(SttEvent {
                                    text: full_transcript.clone(),
                                    is_final: false,
                                    confidence: avg_conf,
                                    language: language.clone(),
                                })
                                .await;
                        }
                    }
                    Err(e) => {
                        eprintln!("[web-stt] Chunk recognition error: {}", e);
                    }
                }
            }
        }

        // Channel closed = Astra's VAD determined the utterance ended.
        // Recognize whatever remains in the buffer.
        if !buf.is_empty() {
            match google_transcribe_chunk(&cfg, &buf, rate, &language).await {
                Ok((text, conf)) => {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        if !full_transcript.is_empty() {
                            full_transcript.push(' ');
                        }
                        full_transcript.push_str(trimmed);
                        total_confidence += conf;
                        segments_count += 1;
                    }
                }
                Err(e) => {
                    eprintln!("[web-stt] Final chunk recognition error: {}", e);
                }
            }
        }

        let avg_conf = if segments_count > 0 {
            total_confidence / (segments_count as f32)
        } else {
            0.0
        };

        let _ = events
            .send(SttEvent {
                text: full_transcript,
                is_final: true,
                confidence: avg_conf,
                language,
            })
            .await;

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
            "el-GR".into(),
            "he-IL".into(),
            "ro-RO".into(),
            "hu-HU".into(),
            "cs-CZ".into(),
            "bg-BG".into(),
            "hr-HR".into(),
            "sl-SI".into(),
            "sr-RS".into(),
            "sk-SK".into(),
            "lt-LT".into(),
            "lv-LV".into(),
            "et-EE".into(),
            "id-ID".into(),
            "th-TH".into(),
            "vi-VN".into(),
            "zh-TW".into(),
        ]
    }

    /// Config fields on the Voice page — language selection.
    /// The API key is configured in the plugin's settings tab (plugin.toml [config]).
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
        ]
    }
}

astra::main!(GoogleStt::default());
