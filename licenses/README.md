# Лицензии сторонних компонентов

Здесь лежат тексты лицензий и уведомления ровно в том виде, в каком их отдают
авторы. Программа их не переписывает.

Раскладка:

| Файл | Кому | Лицензия |
|---|---|---|
| `apache-2.0.txt` | VoxLock, sherpa-onnx, cpal, Tauri | Apache-2.0 |
| `voxlock-NOTICE.txt` | VoxLock © Arbaz Siddiqui | Apache-2.0, §4(d) |
| `gtcrn-LICENSE.txt` | GTCRN © Rong Xiaobin | MIT |
| `silero-vad-LICENSE.txt` | Silero VAD © Silero Team | MIT |
| `onnxruntime-LICENSE.txt` | ONNX Runtime © Microsoft | MIT |
| `lucide-LICENSE.txt` | Lucide © Lucide Icons and Contributors | ISC |
| `mit.txt` | крейты и npm-пакеты | MIT |

Общая картина — в `THIRD_PARTY_NOTICES.md`. Список зависимостей в нём
собирается скриптом `app/scripts/licenses.ps1`, руками его не правят.

Правило: если компонент уезжает в `portable/` или в установщик NSIS, рядом с ним
должны лежать его лицензия и уведомления. Это делают
`app/scripts/package-portable.ps1` и `bundle.resources` в
`app/src-tauri/tauri.conf.json`.
