# Noise Canceller

Портативное приложение для Windows 11: подавляет шум и **убирает чужие голоса** из
микрофона, отдавая очищенный звук в Zoom, Google Meet, Discord, Telegram через
виртуальный кабель. Всё считается на устройстве, без интернета.

![Главное окно](docs/screenshot-main.png)

## Что умеет

- **Изоляция голоса** моделью [VoxLock](https://huggingface.co/arbazsiddiqui/voxlock)
  (target speaker extraction, 27M параметров, 16 кГц): запоминает ваш тембр по
  15-секундной записи и пропускает только его.
- **Профиль голоса с оценкой качества**: речь в %, SNR, пик, клиппинг, оценка 0–100.
- **Чистка сигнала**: срез низов, гейт-экспандер, AGC, лимитер, комфортный шум —
  всё настраивается вручную, изменения применяются на лету.
- **Пресеты**: Дом / Офис / Улица / Стрим.
- **Проверка без драйвера**: «Проверить в колонках» и A/B-запись (два wav: до и после).
- **Установка VB-CABLE одной кнопкой** с проверкой подписи.
- Спектр, метры, тёмная и светлая темы, трей, автозапуск, один экземпляр,
  проверка обновлений модели.

## Стек

| Слой | Решение |
|---|---|
| Ядро | Rust: `cpal` (WASAPI), `rtrb`, собственный ресемплер и DSP |
| Модель | sherpa-onnx 2.0.1 (C API) + VoxLock int8 |
| Оболочка | Tauri 2.12.5, свой заголовок окна |
| UI | Vue 3 Composition API + TypeScript + Vite 8 + Tailwind 4 |

Модель, DSP и захват/вывод звука — в одном процессе, без прослоек.

## Сборка

```powershell
cd app
.\scripts\fetch-models.ps1          # модели (app/models) + DLL (app/native)
npm.cmd install
npm.cmd run tauri build -- --no-bundle
copy target\release\noise-canceller.exe ..\portable\
```

Портативная папка:

```
portable\
  noise-canceller.exe
  models\voxlock\     enroll.onnx, chunk_int8.onnx
  native\             sherpa-onnx-c-api.dll, onnxruntime.dll
  README.txt
```

## Проверка

```powershell
cd app
cargo run -p noise-canceller-cli -- selftest 80   # паритет по звуку
powershell -File scripts\golden.ps1               # golden-тест
```

Ожидаемые цифры: `mic 0.0534 · model 0.0417 · dsp 0.1261 · RTF 0.43`.

## Аудиотракт

```
mic 48k → 16k → VoxLock (40 мс блок, 24 мс задержки)
        → 48k → HPF → гейт → AGC → лимитер → «CABLE Input»
```

Итоговая задержка ≈ 100–120 мс. Обычный запуск требует виртуальный кабель;
в колонки звук идёт только в режиме проверки.

## Ограничения

- VoxLock работает на 16 кГц — полоса уже, чем у «сырого» микрофона.
- Модель обучена на английской речи; на русской качество проверялось косвенно.
- Чужие голоса гасятся на 10–25 дБ, а не в абсолютный ноль.
- Нужен VB-CABLE (ставится одной кнопкой), права администратора один раз.

## Лицензии

VoxLock — Apache-2.0 © Arbaz Siddiqui · sherpa-onnx — Apache-2.0 © k2-fsa ·
NAudio больше не используется · Tauri/Vue/Tailwind — MIT.
