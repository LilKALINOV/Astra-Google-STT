# Agent Instructions: Astra-Google-STT (web-stt)

## 1. Архитектура и стек проекта
- **Плагин для Astra Assistant**: `web-stt` (Web Speech-to-Text).
- **Язык разработки**: Rust (Rust 2021 edition), SDK: `astra-plugin-sdk 0.6`.
- **Протокол и сборка плагина**: CLI `astra-plugin` (версия 0.5.1), спецификация Bundle v2 (`.astraplugin`).
- **Бинарник**: `target/release/web-stt.exe` (Windows) / `target/release/web-stt` (Linux).
- **Интеграция**: Google Web Speech API v2 (Chromium public key / personal key).
- **Каталог локалей**: `locales/` (`en.json`, `ru.json`, `uk.json`), контролируется через `locales.lock.json`.

## 2. Команды сборки и проверки
- **Компиляция бинарника**:
  ```bash
  cargo build --release
  ```
- **Проверка манифеста и локалей**:
  ```bash
  astra-plugin check
  ```
- **Синхронизация локалей**:
  ```bash
  astra-plugin locale sync
  ```
- **Сборка бандла плагина (.astraplugin)**:
  ```bash
  astra-plugin build . --target windows-x64 --reproducible
  ```
- **Верификация бандла**:
  ```bash
  astra-plugin verify <bundle-name>.astraplugin
  ```

## 3. Системные конвенции и правила
1. **Автор коммитов**: строго `Lil KKALINOV` (`150132236+LilKALINOV@users.noreply.github.com`).
2. **Никаких сторонних соавторов** и трейлеров (`Co-authored-by`).
3. **Требования к ассетам**: `icon.png` строго <= 8192 байт (8 KiB) для соблюдения лимитов реестра Astra.
4. **Зависимости**: `protoc` (Protocol Buffers compiler) требуется для кодогенерации `astra-plugin-sdk` / `prost-build` / `tonic-build`. Находится в `.cargo/bin/protoc.exe`.

## 4. Датасет предотвращения ошибок
1. **Отсутствие protoc**: при сборке `astra-plugin-sdk` падает с ошибкой `Could not find protoc`. Решение: установить `protoc` в PATH или `.cargo/bin/`.
2. **Лимит иконки (max_icon_bytes)**: иконка `icon.png` размером > 8192 байт отбрасывается каталогом Astra. Решение: сжимать/ресайзить `icon.png` до <= 8192 байт или использовать `icon.svg`.
3. **Отсутствие listing.name в en.json**: `astra-plugin check` требует наличия `listing.name` во всех файлах локалей, если присутствует `locales/`. Решение: `astra-plugin locale sync` и `astra-plugin locale add <lang>`.
4. **Команды в PowerShell/Windows**: избегать сложных пайпов с неэкранированными переменными `$`; использовать `.ps1` скрипты или Python-скрипты.
