# Лицензии сторонних компонентов

Здесь лежат тексты лицензий и уведомления ровно в том виде, в каком их отдают
авторы. Программа их не переписывает.

Раскладка:

| Файл | Кому | Лицензия |
|---|---|---|
| `apache-2.0.txt` | VoxLock, sherpa-onnx, NovaSR, cpal, Tauri | Apache-2.0 |
| `voxlock-NOTICE.txt` | VoxLock © Arbaz Siddiqui | Apache-2.0, §4(d) |
| `gtcrn-LICENSE.txt` | GTCRN © Rong Xiaobin | MIT |
| `dpdfnet-LICENSE.txt` | DPDFNet © Ceva-IP | Apache-2.0 |
| `ulunas-LICENSE.txt` | UL-UNAS © Xiaobin Rong, порт © Tanix | MIT |
| `silero-vad-LICENSE.txt` | Silero VAD © Silero Team | MIT |
| `novasr-LICENSE.txt` | NovaSR © Yatharth Sharma | Apache-2.0 |
| `novasr-NOTICE.txt` | NovaSR © Yatharth Sharma | Apache-2.0, §4(d) |
| `3dspeaker-LICENSE.txt` | 3D-Speaker CAM++ © ModelScope | Apache-2.0 |
| `onnxruntime-LICENSE.txt` | ONNX Runtime © Microsoft | MIT |
| `lucide-LICENSE.txt` | Lucide © Lucide Icons and Contributors | ISC |
| `rfd-LICENSE.txt` | rfd © Poly | MIT |
| `tauri-plugin-dialog-LICENSE_*.txt` | tauri-plugin-dialog © Tauri Programme | Apache-2.0 / MIT |
| `tauri-plugin-fs-LICENSE_*.txt` | tauri-plugin-fs © Tauri Programme | Apache-2.0 / MIT |
| `tauri-plugin-dialog-js-LICENSE.spdx.txt` | @tauri-apps/plugin-dialog © Tauri Programme | Apache-2.0 / MIT |
| `mit.txt` | крейты и npm-пакеты | MIT |

Общая картина — в `THIRD_PARTY_NOTICES.md`. Список зависимостей в нём
собирается скриптом `app/scripts/licenses.ps1`, руками его не правят.

Правило: если компонент уезжает в `portable/` или в установщик NSIS, рядом с ним
должны лежать его лицензия и уведомления. Это делают
`app/scripts/package-portable.ps1` и `bundle.resources` в
`app/src-tauri/tauri.conf.json`.
