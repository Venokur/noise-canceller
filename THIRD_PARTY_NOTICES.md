# Сторонние компоненты и их лицензии

Noise Canceller (дальше — «программа») распространяется под лицензией MIT:
см. `LICENSE`. Всё ниже — про чужие работы, которые программа использует или
поставляет вместе с собой.

Тексты лицензий — в `licenses/`, копии кладутся рядом с программой в
`portable/licenses/` скриптом `app/scripts/package-portable.ps1`. Список
зависимостей в конце файла собирается автоматически (`app/scripts/licenses.ps1`),
руками его не правят.

## Что поставляется вместе с программой

| Компонент | Что именно | Лицензия | Копирайт |
|---|---|---|---|
| VoxLock | `models/voxlock/enroll.onnx`, `chunk_int8.onnx` | Apache-2.0 | © 2026 Arbaz Siddiqui |
| GTCRN | `models/denoise/gtcrn.onnx` | MIT | © 2024 Rong Xiaobin |
| DPDFNet | `models/denoise/dpdfnet4.onnx` | Apache-2.0 | © Ceva-IP (dpdfnet) |
| UL-UNAS | внутри exe (веса DNS3) | MIT | © 2026 Xiaobin Rong; порт © 2026 Tanix |
| Silero VAD | `models/denoise/silero_vad.onnx` | MIT | © 2020-present Silero Team |
| sherpa-onnx | `native/sherpa-onnx-c-api.dll` | Apache-2.0 | © k2-fsa |
| ONNX Runtime | `native/onnxruntime.dll` | MIT | © Microsoft Corporation |
| Tauri, wry, tao, tray-icon, global-hotkey, плагины | внутри exe | MIT / Apache-2.0 | © Tauri Programme within the Commons Conservancy |
| cpal | внутри exe | Apache-2.0 | © RustAudio developers |
| Vue | внутри exe (собранный бандл) | MIT | © 2013-present Evan You and Vue contributors |
| Tailwind CSS | внутри exe (скомпилированный CSS) | MIT | © Tailwind Labs, Inc. |
| Lucide (`@lucide/vue`) | иконки в интерфейсе и иконка приложения (`AudioLines`) | ISC | © 2026 Lucide Icons and Contributors |

Apache-2.0 обязывает не только приложить текст лицензии, но и сохранить
`NOTICE`, если он есть: у VoxLock он есть (`licenses/voxlock-NOTICE.txt`, там
атрибуция REAL-TSE/WeSep), у sherpa-onnx — нет.

Внутри `onnxruntime.dll` статически собраны сторонние библиотеки (Eigen,
abseil, protobuf, flatbuffers и другие) — это официальная сборка Microsoft,
её уведомления лежат в релизе ONNX Runtime. Текст MIT самой ONNX Runtime —
`licenses/onnxruntime-LICENSE.txt`.

## Что **не** поставляется

- **VB-CABLE** (виртуальный кабель VB-Audio). Программа не кладёт его в
  сборку и не перевыкладывает: кнопка «Установить виртуальный микрофон»
  скачивает официальный архив с `download.vb-audio.com`, проверяет подпись
  установщика и запускает его. VB-CABLE — donationware VB-Audio Software:
  бесплатен для личного использования, для профессионального нужна их
  лицензия, распространение требует отдельного разрешения.
- **DPDFNet2** — модель пробовали и убрали; в сборку не попадает.
- **test_wavs** — эталонные записи для `nc selftest` из репозитория
  sherpa-onnx; нужны только разработчику, в `portable/` не кладутся.

## Зависимости Rust

Собрано для `x86_64-pc-windows-msvc`, только то, что реально линкуется в exe.
Тексты лицензий каждого крейта лежат в его пакете на crates.io; ниже — имя,
версия и SPDX-идентификатор. Крейты с `MPL-2.0` (`cssparser`, `selectors`,
`dtoa-short`, `option-ext`, `cssparser-macros`) не изменялись, их исходники
доступны на crates.io — этого требует MPL-2.0 §3.2.

| Крейт | Версия | Лицензия |
|---|---|---|
<!-- inventory:rust:start -->
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |
| aho-corasick | 1.1.5 | Unlicense OR MIT |
| alloc-no-stdlib | 3.0.0 | BSD-3-Clause |
| alloc-stdlib | 0.3.0 | BSD-3-Clause |
| anyhow | 1.0.104 | MIT OR Apache-2.0 |
| auto-launch | 0.6.0 | MIT |
| base64 | 0.23.1 | MIT OR Apache-2.0 |
| bitflags | 2.13.2 | MIT OR Apache-2.0 |
| bit-set | 0.8.0 | Apache-2.0 OR MIT |
| bit-vec | 0.8.0 | Apache-2.0 OR MIT |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 |
| brotli | 9.1.1 | BSD-3-Clause AND MIT |
| brotli-decompressor | 6.2.0 | BSD-3-Clause/MIT |
| byteorder | 1.5.0 | Unlicense OR MIT |
| bytes | 1.12.1 | MIT |
| camino | 1.2.6 | MIT OR Apache-2.0 |
| cargo_metadata | 0.19.2 | MIT |
| cargo-platform | 0.1.9 | MIT OR Apache-2.0 |
| cfb | 0.14.0 | MIT |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 |
| cookie | 0.18.2 | MIT OR Apache-2.0 |
| cpal | 0.18.2 | Apache-2.0 |
| cpufeatures | 0.2.17 | MIT OR Apache-2.0 |
| crc32fast | 1.5.4 | MIT OR Apache-2.0 |
| crossbeam-channel | 0.5.17 | MIT OR Apache-2.0 |
| crossbeam-utils | 0.8.23 | MIT OR Apache-2.0 |
| crypto-common | 0.1.7 | MIT OR Apache-2.0 |
| cssparser | 0.37.0 | MPL-2.0 |
| cssparser-macros | 0.7.1 | MPL-2.0 |
| ctor | 1.0.13 | Apache-2.0 OR MIT |
| darling | 0.24.1 | MIT |
| darling_core | 0.24.1 | MIT |
| darling_macro | 0.24.1 | MIT |
| dasp_sample | 0.11.0 | MIT OR Apache-2.0 |
| deranged | 0.5.9 | MIT OR Apache-2.0 |
| derive_more | 2.1.1 | MIT |
| derive_more-impl | 2.1.1 | MIT |
| digest | 0.10.7 | MIT OR Apache-2.0 |
| dirs | 7.0.0 | MIT OR Apache-2.0 |
| dirs-sys | 0.5.0 | MIT OR Apache-2.0 |
| displaydoc | 0.2.7 | MIT OR Apache-2.0 |
| dom_query | 0.28.0 | MIT |
| dpi | 0.1.2 | Apache-2.0 AND MIT |
| dtoa | 1.0.11 | MIT OR Apache-2.0 |
| dtoa-short | 0.3.5 | MPL-2.0 |
| dunce | 1.0.5 | CC0-1.0 OR MIT-0 OR Apache-2.0 |
| dyn-clone | 1.0.20 | MIT OR Apache-2.0 |
| equivalent | 1.0.3 | Apache-2.0 OR MIT |
| erased-serde | 0.4.11 | MIT OR Apache-2.0 |
| fastrand | 2.5.0 | Apache-2.0 OR MIT |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 |
| flate2 | 1.1.10 | MIT OR Apache-2.0 |
| fnv | 1.0.7 | Apache-2.0 / MIT |
| foldhash | 0.2.0 | Zlib |
| form_urlencoded | 1.2.2 | MIT OR Apache-2.0 |
| generic-array | 0.14.7 | MIT |
| getrandom | 0.3.4 | MIT OR Apache-2.0 |
| getrandom | 0.4.3 | MIT OR Apache-2.0 |
| glob | 0.3.4 | MIT OR Apache-2.0 |
| global-hotkey | 0.8.0 | Apache-2.0 OR MIT |
| hashbrown | 0.12.3 | MIT OR Apache-2.0 |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 |
| heck | 0.5.0 | MIT OR Apache-2.0 |
| hound | 3.5.1 | Apache-2.0 |
| html5ever | 0.39.0 | MIT OR Apache-2.0 |
| http | 1.5.0 | MIT OR Apache-2.0 |
| ico | 0.6.0 | MIT |
| icu_collections | 2.4.0 | Unicode-3.0 |
| icu_locale_core | 2.4.0 | Unicode-3.0 |
| icu_normalizer | 2.4.0 | Unicode-3.0 |
| icu_normalizer_data | 2.4.0 | Unicode-3.0 |
| icu_properties | 2.4.0 | Unicode-3.0 |
| icu_properties_data | 2.4.0 | Unicode-3.0 |
| icu_provider | 2.4.0 | Unicode-3.0 |
| ident_case | 1.0.1 | MIT/Apache-2.0 |
| idna | 1.1.0 | MIT OR Apache-2.0 |
| idna_adapter | 1.2.2 | Apache-2.0 OR MIT |
| indexmap | 1.9.3 | Apache-2.0 OR MIT |
| indexmap | 2.14.2 | Apache-2.0 OR MIT |
| infer | 0.22.0 | MIT |
| itoa | 1.0.18 | MIT OR Apache-2.0 |
| json-patch | 4.2.0 | MIT/Apache-2.0 |
| jsonptr | 0.7.1 | MIT OR Apache-2.0 |
| keyboard-types | 0.7.0 | MIT OR Apache-2.0 |
| keyboard-types | 0.8.3 | MIT OR Apache-2.0 |
| libc | 0.2.191 | MIT OR Apache-2.0 |
| libloading | 0.8.9 | ISC |
| litemap | 0.8.3 | Unicode-3.0 |
| lock_api | 0.4.14 | MIT OR Apache-2.0 |
| log | 0.4.35 | MIT OR Apache-2.0 |
| markup5ever | 0.39.0 | MIT OR Apache-2.0 |
| memchr | 2.8.3 | Unlicense OR MIT |
| mime | 0.3.17 | MIT OR Apache-2.0 |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 |
| mio | 1.2.4 | MIT |
| muda | 0.20.0 | Apache-2.0 OR MIT |
| new_debug_unreachable | 1.0.6 | MIT |
| num-conv | 0.2.2 | MIT OR Apache-2.0 |
| once_cell | 1.21.4 | MIT OR Apache-2.0 |
| option-ext | 0.2.0 | MPL-2.0 |
| os_info | 3.15.0 | MIT |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 |
| phf | 0.13.1 | MIT |
| phf_generator | 0.13.1 | MIT |
| phf_macros | 0.13.1 | MIT |
| phf_shared | 0.13.1 | MIT |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT |
| plist | 1.10.3 | MIT |
| png | 0.18.1 | MIT OR Apache-2.0 |
| potential_utf | 0.1.6 | Unicode-3.0 |
| powerfmt | 0.2.1 | MIT OR Apache-2.0 |
| precomputed-hash | 0.1.1 | MIT |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 |
| quick-xml | 0.42.0 | MIT |
| quote | 1.0.47 | MIT OR Apache-2.0 |
| raw-window-handle | 0.6.2 | MIT OR Apache-2.0 OR Zlib |
| regex | 1.13.1 | MIT OR Apache-2.0 |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 |
| rtrb | 0.4.1 | MIT OR Apache-2.0 |
| rustc-hash | 2.1.3 | Apache-2.0 OR MIT |
| same-file | 1.0.6 | Unlicense/MIT |
| schemars | 0.8.22 | MIT |
| schemars_derive | 0.8.22 | MIT |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 |
| selectors | 0.38.0 | MPL-2.0 |
| semver | 1.0.28 | MIT OR Apache-2.0 |
| serde | 1.0.229 | MIT OR Apache-2.0 |
| serde_core | 1.0.229 | MIT OR Apache-2.0 |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 |
| serde_derive_internals | 0.29.1 | MIT OR Apache-2.0 |
| serde_json | 1.0.154 | MIT OR Apache-2.0 |
| serde_repr | 0.1.21 | MIT OR Apache-2.0 |
| serde_spanned | 1.1.2 | MIT OR Apache-2.0 |
| serde_with | 3.24.0 | MIT OR Apache-2.0 |
| serde_with_macros | 3.24.0 | MIT OR Apache-2.0 |
| serde-untagged | 0.1.9 | MIT OR Apache-2.0 |
| serialize-to-javascript | 0.1.2 | MIT OR Apache-2.0 |
| serialize-to-javascript-impl | 0.1.2 | MIT OR Apache-2.0 |
| servo_arc | 0.4.3 | MIT OR Apache-2.0 |
| sha2 | 0.10.9 | MIT OR Apache-2.0 |
| simd-adler32 | 0.3.10 | MIT |
| siphasher | 1.0.4 | MIT OR Apache-2.0 |
| smallvec | 1.16.4 | MIT OR Apache-2.0 |
| socket2 | 0.6.5 | MIT OR Apache-2.0 |
| softbuffer | 0.4.8 | MIT OR Apache-2.0 |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 |
| string_cache | 0.9.0 | MIT OR Apache-2.0 |
| strsim | 0.11.1 | MIT |
| syn | 2.0.119 | MIT OR Apache-2.0 |
| syn | 3.0.9 | MIT OR Apache-2.0 |
| synstructure | 0.14.0 | MIT |
| tao | 0.37.2 | Apache-2.0 |
| tauri | 2.12.5 | Apache-2.0 OR MIT |
| tauri-codegen | 2.7.4 | Apache-2.0 OR MIT |
| tauri-macros | 2.7.4 | Apache-2.0 OR MIT |
| tauri-plugin-autostart | 2.7.0 | Apache-2.0 OR MIT |
| tauri-plugin-global-shortcut | 2.4.1 | Apache-2.0 OR MIT |
| tauri-plugin-single-instance | 2.5.5 | Apache-2.0 OR MIT |
| tauri-runtime | 2.12.2 | Apache-2.0 OR MIT |
| tauri-runtime-wry | 2.12.3 | Apache-2.0 OR MIT |
| tauri-utils | 2.10.2 | Apache-2.0 OR MIT |
| tendril | 0.5.1 | MIT OR Apache-2.0 |
| thiserror | 2.0.21 | MIT OR Apache-2.0 |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 |
| time | 0.3.55 | MIT OR Apache-2.0 |
| time-core | 0.1.9 | MIT OR Apache-2.0 |
| time-macros | 0.2.32 | MIT OR Apache-2.0 |
| tinystr | 0.8.5 | Unicode-3.0 |
| tokio | 1.54.1 | MIT |
| toml | 1.1.8+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_datetime | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_parser | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_writer | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 |
| tracing | 0.1.44 | MIT |
| tracing-attributes | 0.1.31 | MIT |
| tracing-core | 0.1.36 | MIT |
| tray-icon | 0.25.1 | MIT OR Apache-2.0 |
| typeid | 1.0.3 | MIT OR Apache-2.0 |
| typenum | 1.20.1 | MIT OR Apache-2.0 |
| ul_unas_rs | 0.1.2 | MIT |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| unicode-segmentation | 1.13.3 | MIT OR Apache-2.0 |
| url | 2.5.8 | MIT OR Apache-2.0 |
| urlpattern | 0.6.0 | MIT |
| utf_types | 1.0.0 | Apache-2.0 OR MIT |
| utf8_iter | 1.1.0 | Apache-2.0 OR MIT |
| uuid | 1.29.0 | Apache-2.0 OR MIT |
| walkdir | 2.5.0 | Unlicense/MIT |
| web_atoms | 0.2.6 | MIT OR Apache-2.0 |
| web-time | 1.1.0 | MIT OR Apache-2.0 |
| webview2-com | 0.39.1 | MIT |
| webview2-com-macros | 0.8.1 | MIT |
| webview2-com-sys | 0.39.1 | MIT |
| winapi-util | 0.1.11 | Unlicense OR MIT |
| windows | 0.62.2 | MIT OR Apache-2.0 |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 |
| windows-collections | 0.3.2 | MIT OR Apache-2.0 |
| windows-core | 0.62.2 | MIT OR Apache-2.0 |
| windows-future | 0.3.2 | MIT OR Apache-2.0 |
| windows-implement | 0.60.2 | MIT OR Apache-2.0 |
| windows-interface | 0.59.3 | MIT OR Apache-2.0 |
| windows-link | 0.2.1 | MIT OR Apache-2.0 |
| windows-numerics | 0.3.1 | MIT OR Apache-2.0 |
| windows-registry | 0.6.1 | MIT OR Apache-2.0 |
| windows-result | 0.4.1 | MIT OR Apache-2.0 |
| windows-strings | 0.5.1 | MIT OR Apache-2.0 |
| windows-sys | 0.59.0 | MIT OR Apache-2.0 |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 |
| windows-threading | 0.2.1 | MIT OR Apache-2.0 |
| windows-version | 0.1.7 | MIT OR Apache-2.0 |
| window-vibrancy | 0.8.1 | Apache-2.0 OR MIT |
| winnow | 1.0.4 | MIT |
| writeable | 0.6.4 | Unicode-3.0 |
| wry | 0.57.1 | Apache-2.0 OR MIT |
| yoke | 0.8.4 | Unicode-3.0 |
| yoke-derive | 0.8.4 | Unicode-3.0 |
| zerofrom | 0.1.8 | Unicode-3.0 |
| zerofrom-derive | 0.1.8 | Unicode-3.0 |
| zerotrie | 0.2.5 | Unicode-3.0 |
| zerovec | 0.11.9 | Unicode-3.0 |
| zerovec-derive | 0.11.7 | Unicode-3.0 |
| zmij | 1.0.23 | MIT |
<!-- inventory:rust:end -->

## Зависимости npm

Рантайм-зависимости фронтенда: то, из чего Vite собирает бандл, и их
собственные зависимости. Инструменты сборки (Vite, Tailwind, TypeScript) в
бандл не входят и здесь не перечислены.

| Пакет | Версия | Лицензия |
|---|---|---|
<!-- inventory:npm:start -->
| @babel/helper-string-parser | 7.29.11 | MIT |
| @babel/helper-validator-identifier | 7.29.7 | MIT |
| @babel/parser | 7.29.12 | MIT |
| @babel/types | 7.29.11 | MIT |
| @jridgewell/sourcemap-codec | 1.6.0 | MIT |
| @lucide/vue | 1.68.0 | ISC |
| @tauri-apps/api | 2.12.4 | Apache-2.0 OR MIT |
| @vue/compiler-core | 3.5.44 | MIT |
| @vue/compiler-dom | 3.5.44 | MIT |
| @vue/compiler-sfc | 3.5.44 | MIT |
| @vue/compiler-ssr | 3.5.44 | MIT |
| @vue/reactivity | 3.5.44 | MIT |
| @vue/runtime-core | 3.5.44 | MIT |
| @vue/runtime-dom | 3.5.44 | MIT |
| @vue/server-renderer | 3.5.44 | MIT |
| @vue/shared | 3.5.44 | MIT |
| csstype | 3.2.3 | MIT |
| entities | 7.0.1 | BSD-2-Clause |
| estree-walker | 2.0.2 | MIT |
| magic-string | 1.4.5 | MIT |
| nanoid | 3.3.20 | MIT |
| picocolors | 1.1.1 | ISC |
| postcss | 8.5.32 | MIT |
| source-map-js | 1.2.2 | BSD-3-Clause |
| vue | 3.5.44 | MIT |
<!-- inventory:npm:end -->

## Лицензии текстом

- `licenses/apache-2.0.txt` — Apache License 2.0 (VoxLock, sherpa-onnx, cpal, Tauri)
- `licenses/voxlock-NOTICE.txt` — NOTICE модели VoxLock
- `licenses/gtcrn-LICENSE.txt` — MIT, GTCRN
- `licenses/dpdfnet-LICENSE.txt` — Apache-2.0, DPDFNet
- `licenses/ulunas-LICENSE.txt` — MIT, UL-UNAS и её Rust-порт
- `licenses/silero-vad-LICENSE.txt` — MIT, Silero VAD
- `licenses/onnxruntime-LICENSE.txt` — MIT, ONNX Runtime
- `licenses/lucide-LICENSE.txt` — ISC, Lucide
- `licenses/mit.txt` — MIT для остальных крейтов и npm-пакетов

Если компонент меняется или добавляется новый, обновите таблицу выше и
перегенерируйте список: `powershell -File app\scripts\licenses.ps1`.
