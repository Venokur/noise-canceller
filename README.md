# Noise Canceller

Портативное приложение для Windows 11: подавляет шум и **убирает чужие голоса** из
микрофона, отдавая очищенный звук в Zoom, Google Meet, Discord, Telegram через
виртуальный кабель. Всё считается на устройстве, без интернета: единственный
сетевой запрос — проверка ревизии модели на HuggingFace по кнопке.

![Главное окно](docs/screenshot-main.png)

## Что умеет

- **Изоляция голоса** моделью [VoxLock](https://huggingface.co/arbazsiddiqui/voxlock)
  (target speaker extraction, 27M параметров, 16 кГц): запоминает ваш тембр по
  15-секундной записи и пропускает только его.
- **Остаточный шумодав GTCRN** после VoxLock и **VAD-гейт** на Silero: паузы
  закрываются по детектору речи, а не по порогу громкости.
- **Профиль голоса с оценкой качества**: речь в %, SNR, пик, клиппинг, оценка 0–100.
- **Чистка сигнала**: срез низов, гейт-экспандер, AGC, лимитер, комфортный шум,
  предел подавления — всё настраивается вручную, изменения применяются на лету.
- **Пресеты**: Дом / Офис / Улица / Стрим.
- **Проверка без драйвера**: «Проверить в колонках» и A/B-запись (два wav: до и
  после) с метриками — громкость, паузы тише речи, самый длинный провал.
- **Установка VB-CABLE одной кнопкой** с проверкой подписи.
- Спектр, метры, тёмная и светлая темы, трей, автозапуск, один экземпляр,
  глобальный mute по Ctrl+Alt+M, проверка обновлений модели.

## Стек

| Слой | Решение |
|---|---|
| Ядро | Rust: `cpal` (WASAPI), `rtrb`, собственный ресемплер и DSP |
| Модели | sherpa-onnx 2.0.1 (C API): VoxLock int8, GTCRN, Silero VAD |
| Оболочка | Tauri 2.12.5, свой заголовок окна |
| UI | Vue 3 Composition API + TypeScript + Vite 8 + Tailwind 4 |

Модель, DSP и захват/вывод звука — в одном процессе, без прослоек.

## Сборка

```powershell
cd app
.\scripts\fetch-models.ps1          # модели (voxlock + denoise) и DLL (native)
npm.cmd install
npm.cmd run tauri build -- --no-bundle
copy target\release\noise-canceller.exe ..\portable\
```

Портативная папка:

```
portable\
  noise-canceller.exe
  models\voxlock\     enroll.onnx, chunk_int8.onnx
  models\denoise\     gtcrn.onnx, silero_vad.onnx
  native\             sherpa-onnx-c-api.dll, onnxruntime.dll
  data\               настройки, профили, записи, журнал (создаётся само)
  README.txt
```

Модели и DLL в `portable\` берутся из `app\models\` и `app\native\` — те же файлы,
что скачивает `fetch-models.ps1`.

## Проверка

```powershell
cd app
cargo run -p noise-canceller-cli -- selftest 80   # модель + DSP на эталонном wav
powershell -File scripts\golden.ps1               # golden-тест, падает при дрейфе
powershell -File scripts\regression.ps1 -Count 3  # последние A/B через тот же тракт
```

Ожидаемые цифры `selftest 80`: `mic 0.0534 · model 0.0417 · dsp 0.0283 · RTF 0.32`.

## Аудиотракт

```
mic 48k → 16k → выравнивание уровня → пиковый ограничитель
        → VoxLock (блок 60 мс) → GTCRN → VAD-гейт
        → 48k → HPF → гейт → AGC → лимитер → «CABLE Input»
```

Итоговая задержка ≈ 60–150 мс: модель (24 мс) плюс содержимое кольца (блок +
буфер, буфер при авто-режиме подбирается по пропускам). Обычный запуск требует
виртуальный кабель; в колонки звук идёт только в режиме проверки.

## Ограничения

- VoxLock работает на 16 кГц — полоса уже, чем у «сырого» микрофона.
- Модель обучена на английской речи; на русской качество проверялось косвенно.
- Чужие голоса гасятся на 10–25 дБ, а не в абсолютный ноль.
- Нужен VB-CABLE (ставится одной кнопкой), права администратора один раз.

## Лицензии

VoxLock — Apache-2.0 © Arbaz Siddiqui · sherpa-onnx — Apache-2.0 © k2-fsa ·
ONNX Runtime — MIT · cpal — Apache-2.0 · Tauri — MIT/Apache-2.0 · Vue — MIT ·
Tailwind — MIT · Lucide — ISC.
