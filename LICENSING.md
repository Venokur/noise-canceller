# Лицензионная политика

Приложение распространяется бесплатно под MIT (`LICENSE`). Чтобы так и
оставалось, всё, что попадает в сборку — крейты, npm-пакеты, модели, DLL, —
должно быть permissive и допускать перевыкладывание.

## Что разрешено

- MIT, MIT-0, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, 0BSD,
  Unlicense, CC0-1.0, Unicode-3.0;
- MPL-2.0 — только для пяти неизменённых крейтов (`cssparser`,
  `cssparser-macros`, `selectors`, `dtoa-short`, `option-ext`);
- модели и веса — только с явной лицензией и правом на перевыкладывание.

## Что запрещено

- GPL, AGPL, LGPL, SSPL, BUSL, Commons Clause и подобные;
- CC-BY-NC и другие «non-commercial» веса: они делают некоммерческой всю
  сборку и конфликтуют с MIT;
- лицензия `other`, `unknown` или её отсутствие;
- TEN-VAD (Apache-2.0 с дополнительными условиями Agora) — до отдельного
  юридического решения;
- VB-CABLE в архиве или установщике: только официальный установщик с
  vb-audio.com по действию пользователя.

## Как это проверяется

```powershell
cd app
powershell -File scripts\licenses.ps1 -Check -Policy
```

- `-Check` сверяет `THIRD_PARTY_NOTICES.md` с `Cargo.lock` и `package-lock.json`;
- `-Policy` проверяет allowlist для всех крейтов и npm-пакетов;
- `MODEL_LICENSES.json` перечисляет каждый `.onnx` и `.dll` с лицензией,
  источником и NOTICE; новый файл без строки в манифесте валит проверку.

CI гоняет обе проверки, поэтому «забыть» лицензию не получится.

## VB-CABLE

VB-CABLE — donationware: для личного и некоммерческого использования бесплатен,
для профессионального нужна лицензия VB-Audio. Приложение не кладёт драйвер в
сборку и не перевыкладывает его: кнопка установки скачивает официальный архив и
запускает официальный установщик. Пользователь, которому нужен коммерческий
сценарий, лицензирует драйвер у VB-Audio отдельно.

## Модели

| Модель | Лицензия | Что сохраняем |
|---|---|---|
| VoxLock | Apache-2.0 | текст + `voxlock-NOTICE.txt` |
| GTCRN | MIT | текст |
| DPDFNet4 | Apache-2.0 | текст |
| Silero VAD | MIT | текст |
| NovaSR | Apache-2.0 | текст + `novasr-NOTICE.txt` |
| 3D-Speaker CAM++ | Apache-2.0 | текст |
| UL-UNAS | MIT | текст |
| sherpa-onnx | Apache-2.0 | текст |
| ONNX Runtime | MIT | текст |

Источники и файлы — в `MODEL_LICENSES.json`.
