use astra_plugin_sdk::prelude::*;

use reqwest::Client;

#[derive(Debug, Clone, Default)]
#[astra::config]
struct GoogleSttConfig {
    #[serde(default)]
    language_code: String,
    #[serde(default)]
    sample_rate_hz: u32,
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
        eprintln!(
            "[google-stt] Config loaded: language={}, sample_rate={}",
            lang, config.sample_rate_hz
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

        // Astra SDK sends f32 LE PCM — convert to i16 LE PCM for Google
        let f32_count = audio.len() / 4;
        let mut i16_pcm = Vec::with_capacity(f32_count * 2);
        for i in 0..f32_count {
            let bytes = [audio[i * 4], audio[i * 4 + 1], audio[i * 4 + 2], audio[i * 4 + 3]];
            let sample_f32 = f32::from_le_bytes(bytes);
            let clamped = sample_f32.clamp(-1.0, 1.0);
            let sample_i16 = (clamped * 32767.0) as i16;
            i16_pcm.extend_from_slice(&sample_i16.to_le_bytes());
        }

        // Send i16 PCM as L16 to Google
        let client = Client::new();
        let url = "http://www.google.com/speech-api/v2/recognize";

        let resp = match client
            .post(url)
            .query(&[
                ("client", "chromium"),
                ("lang", language.as_str()),
                ("key", "AIzaSyBOti4mM-6x9WDnZIjIeyEU21OpBXqWBgw"),
                ("pFilter", "0"),
            ])
            .header("Content-Type", format!("audio/l16; rate={}; endian=little", rate))
            .body(i16_pcm)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[google-stt] HTTP request failed: {}", e);
                return Err(e.into());
            }
        };

        let status = resp.status();
        let text = match resp.text().await {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[google-stt] Failed to read response: {}", e);
                return Err(e.into());
            }
        };

        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "Google Speech API error ({}): {}",
                status,
                text
            ));
        }

        // Google returns newline-separated JSON — parse each line
        let mut best_text = String::new();
        let mut best_confidence = 0.0f32;

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
                    eprintln!("[google-stt] Google error: {}", msg);
                    continue;
                }
                if let Some(results) = line_val.get("result").and_then(|r| r.as_array()) {
                    for result in results {
                        if let Some(alts) =
                            result.get("alternative").and_then(|a| a.as_array())
                        {
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

        Ok(SttEvent {
            text: best_text,
            is_final: true,
            confidence: best_confidence,
            language,
        })
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

    /// Config fields — only language selection, no API key needed.
    #[hook]
    async fn stt_config_fields(&self) -> Vec<FieldDef> {
        vec![FieldDef::dropdown(
            "language_code",
            "Language",
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
        .with_default("ru-RU")]
    }
}

astra::main!(GoogleStt::default());
