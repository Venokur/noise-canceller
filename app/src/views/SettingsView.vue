<script setup lang="ts">
import { inject } from "vue";
import AppIcon from "../components/AppIcon.vue";
import AppLabel from "../components/AppLabel.vue";
import AppSelect from "../components/AppSelect.vue";
import AppSlider from "../components/AppSlider.vue";
import AppSwitch from "../components/AppSwitch.vue";
import { APP_CONTEXT } from "../appContext";

const { applyAutostart, applyPreset, autostart, btn, btnGhost, btnPrimary, capturingHotkey, checkingModel, checkModel, error, exportSettings, importSettings, modelStatus, openLicenses, openLog, presets, resetSettings, restartSoon, settingRow, settings, settingsSection, settingsSections, showWizard, startHotkeyCapture, stats, switchSection } = inject(APP_CONTEXT)!;
</script>

<template>
  <div class="pt-4">
        <div class="grid grid-cols-[178px_minmax(0,1fr)] gap-7">
          <aside class="flex flex-col gap-0.5">
            <button
              v-for="section in settingsSections"
              :key="section.id"
              class="nav-item"
              :data-active="!searching && settingsSection === section.id"
              @click="switchSection(section.id)"
            >
              {{ section.label }}
            </button>
          </aside>

          <div class="min-w-0">
            <div class="flex items-center gap-3.5">
              <label
                class="flex h-[34px] flex-1 items-center gap-2 rounded-[9px] border border-[var(--stroke)] bg-[var(--surface)] px-2.5 text-[var(--muted)] transition-colors focus-within:border-[var(--accent)]"
              >
                <AppIcon name="search" :size="14" />
                <input
                  :value="settingsQuery"
                  class="min-w-0 flex-1 border-0 bg-transparent text-[13px] text-[var(--text)] outline-none placeholder:text-[var(--muted)]"
                  placeholder="Поиск по настройкам"
                  @input="filterSettings(($event.target as HTMLInputElement).value)"
                />
              </label>
              <label
                class="flex shrink-0 cursor-pointer items-center gap-2.5 text-[11.5px] whitespace-nowrap text-[var(--muted)]"
              >
                <AppSwitch
                  :model-value="!settings.basicUi"
                  @update:model-value="(v: boolean) => (settings!.basicUi = !v)"
                />
                Дополнительно
              </label>
            </div>

            <div v-if="!searching" class="mt-3 flex flex-wrap items-center gap-1.5">
              <span class="mr-1 text-[11.5px] text-[var(--muted)]">Пресеты</span>
              <button
                v-for="preset in presets"
                :key="preset.id"
                class="chip"
                @click="applyPreset(preset.patch)"
              >
                {{ preset.label }}
              </button>
            </div>

            <!-- voice -->
            <div v-if="searching || settingsSection === 'voice'" class="sblock">
              <h2 class="mt-5 mb-1 text-[15px] font-semibold tracking-[-0.01em]">Голос</h2>
              <div :class="settingRow">
                <AppLabel
                  text="Оставлять только мой голос"
                  hint="VoxLock пропускает только ваш голос. Выключено — остаётся обычный шумодав: легче процессору и профиль не нужен."
                />
                <div class="flex justify-end">
                  <AppSwitch v-model="settings.voiceIsolation" @update:model-value="() => restartSoon()" />
                </div>
              </div>
              <div v-if="settings.voiceIsolation" :class="settingRow">
                <AppLabel
                  text="Сила изоляции"
                  hint="Сколько очищенного моделью сигнала идёт в выход. 1.00 — максимальная изоляция, ниже — естественнее, но слышнее фон."
                />
                <AppSlider
                  v-model="settings.isolationStrength"
                  :min="0"
                  :max="1"
                  :step="0.05"
                  :format="(v) => v.toFixed(2)"
                />
              </div>
              <div v-if="settings.voiceIsolation" :class="settingRow">
                <AppLabel
                  text="Предел подавления"
                  hint="Защита от провалов: пока вы говорите, выход модели не станет тише входа больше чем на эту величину. Включается только на ваших фразах — музыку и телевизор в паузах он не возвращает. 0 — без защиты: максимальная изоляция, но модель иногда может проглотить слово."
                />
                <AppSlider
                  v-model="settings.maxSuppressionDb"
                  :min="0"
                  :max="40"
                  :step="1"
                  :format="(v) => (v <= 0 ? 'без предела' : `${v} дБ`)"
                />
              </div>
              <div v-if="settings.voiceIsolation" :class="settingRow">
                <AppLabel
                  text="Режим"
                  hint="«По профилю» ищет записанный тембр — точнее всего. «Авто» работает без профиля, но чаще пропускает чужие голоса."
                />
                <AppSelect
                  v-model="settings.mode"
                  :options="[
                    { value: 'profile', label: 'По профилю голоса' },
                    { value: 'auto', label: 'Авто-определение' },
                  ]"
                  @update:model-value="() => restartSoon()"
                />
              </div>
            </div>

            <!-- clean -->
            <div v-if="searching || settingsSection === 'clean'" class="sblock">
              <h2 class="mt-5 mb-1 text-[15px] font-semibold tracking-[-0.01em]">Чистка сигнала</h2>
              <div :class="settingRow">
                <AppLabel
                  text="Усиление тихого микрофона"
                  hint="Поднимает тихий микрофон до −26 dBFS перед моделью. Влияет на разборчивость и ровность громкости."
                />
                <div class="flex justify-end"><AppSwitch v-model="settings.inputLeveling" /></div>
              </div>
              <div :class="settingRow">
                <AppLabel
                  text="Остаточный шум"
                  hint="Чем агрессивнее, тем сильнее давит фон и чужие голоса, но тем выше нагрузка и риск подъесть тихие слоги. GTCRN — баланс, DPDFNet и UL-UNAS — сильнее."
                />
                <AppSelect
                  v-model="settings.residualDenoise"
                  :options="[
                    { value: 'off', label: 'Выключено' },
                    { value: 'gtcrn', label: 'GTCRN · лёгкий' },
                    { value: 'dpdfnet', label: 'DPDFNet · агрессивный' },
                    { value: 'ulunas', label: 'UL-UNAS · самый сильный' },
                  ]"
                  @update:model-value="() => restartSoon()"
                />
              </div>
              <div :class="settingRow">
                <AppLabel
                  text="VAD-гейт"
                  hint="Закрывает выход в паузах по детектору речи, а не по уровню. Фон между словами исчезает, тихие слова не режутся."
                />
                <div class="flex justify-end"><AppSwitch v-model="settings.vadGate" /></div>
              </div>
              <div :class="settingRow">
                <AppLabel
                  text="Срез низов"
                  hint="Фильтр 4-го порядка убирает гул, ветер и низкочастотный стук по столу."
                />
                <div class="flex justify-end"><AppSwitch v-model="settings.highPassEnabled" /></div>
              </div>
              <div v-if="!settings.basicUi" :class="settingRow">
                <AppLabel
                  text="Частота среза"
                  hint="Ниже этой частоты сигнал ослабляется. 80–120 Гц убирают гул, выше — начинает худеть сам голос."
                />
                <AppSlider v-model="settings.highPassHz" :min="20" :max="300" :step="5" :format="(v) => `${v} Гц`" />
              </div>
              <div :class="settingRow">
                <AppLabel
                  text="Тембр"
                  hint="Две полки после автогромкости: «тело» на 200 Гц и разборчивость на 3 кГц. 0 дБ — звук не меняется."
                />
                <div class="flex justify-end"><AppSwitch v-model="settings.toneEnabled" /></div>
              </div>
              <div v-if="settings.toneEnabled" :class="settingRow">
                <AppLabel
                  text="Тело"
                  hint="Полка на 200 Гц. Вниз убирает бубнёж и остатки гула, вверх добавляет плотность. Выше +3 дБ возвращает то, что снимает срез низов."
                />
                <AppSlider v-model="settings.toneLowDb" :min="-6" :max="3" :step="0.5" :format="(v) => `${v.toFixed(1)} дБ`" />
              </div>
              <div v-if="settings.toneEnabled" :class="settingRow">
                <AppLabel
                  text="Разборчивость"
                  hint="Полка на 3 кГц — там согласные. Вверх делает речь яснее, но поднимает и остаточный шип, поэтому предел ±6 дБ. Выше 8 кГц полосы нет: модель работает на 16 кГц."
                />
                <AppSlider v-model="settings.toneHighDb" :min="-6" :max="6" :step="0.5" :format="(v) => `${v.toFixed(1)} дБ`" />
              </div>
              <div :class="settingRow">
                <AppLabel
                  text="Воздух"
                  hint="Добавляет верх выше 8 кГц, которого у 16-кГц модели нет: голос перестаёт звучать «в подушку». Это не шумодав и не изоляция — полоса 0–8 кГц, громкость и чужие голоса не меняются. 0 % — выключено."
                />
                <AppSlider v-model="settings.airAmount" :min="0" :max="1" :step="0.05" :format="(v) => `${Math.round(v * 100)} %`" />
              </div>
              <div :class="settingRow">
                <AppLabel
                  text="Гейт"
                  hint="Пороговый шумодав для остатков сигнала: закрывает всё, что тише порога."
                />
                <div class="flex justify-end"><AppSwitch v-model="settings.gateEnabled" /></div>
              </div>
              <div v-if="!settings.basicUi" :class="settingRow">
                <AppLabel
                  text="Порог гейта"
                  hint="Уровень закрытия гейта. Выше — чище паузы, но можно срезать тихие слова; ниже — мягче."
                />
                <AppSlider v-model="settings.gateThresholdDb" :min="-90" :max="-20" :step="1" :format="(v) => `${v} дБ`" />
              </div>
              <div v-if="!settings.basicUi" :class="settingRow">
                <AppLabel
                  text="Степень"
                  hint="Насколько сильно гейт глушит ниже порога: 1 — мягко, 10 — почти в ноль."
                />
                <AppSlider v-model="settings.gateRatio" :min="1" :max="10" :step="0.5" :format="(v) => v.toFixed(1)" />
              </div>
              <div v-if="!settings.basicUi" :class="settingRow">
                <AppLabel
                  text="Атака"
                  hint="Как быстро открывается гейт. Больше — плавнее, но начало слова может приглушаться."
                />
                <AppSlider v-model="settings.gateAttackMs" :min="1" :max="100" :step="1" :format="(v) => `${v} мс`" />
              </div>
              <div v-if="!settings.basicUi" :class="settingRow">
                <AppLabel
                  text="Восстановление"
                  hint="Как быстро гейт закрывается. Больше — не режет концы слов, но дольше тянет фон."
                />
                <AppSlider v-model="settings.gateReleaseMs" :min="20" :max="600" :step="10" :format="(v) => `${v} мс`" />
              </div>
              <div v-if="!settings.basicUi" :class="settingRow">
                <AppLabel
                  text="Удержание"
                  hint="Сколько гейт остаётся открытым после спада уровня, чтобы не обрубать концы слов."
                />
                <AppSlider v-model="settings.gateHoldMs" :min="0" :max="500" :step="10" :format="(v) => `${v} мс`" />
              </div>
              <div :class="settingRow">
                <AppLabel
                  text="Автогромкость"
                  hint="Выравнивает громкость фраз: подтягивает тихие и приглушает громкие, но медленно, чтобы не дёргать уровень внутри слова. Работает только на вашей речи."
                />
                <div class="flex justify-end"><AppSwitch v-model="settings.agcEnabled" /></div>
              </div>
              <div v-if="!settings.basicUi" :class="settingRow">
                <AppLabel
                  text="Целевой уровень"
                  hint="К какому уровню AGC тянет речь. Ниже — тише и безопаснее, выше — громче, но ближе к перегрузу."
                />
                <AppSlider v-model="settings.agcTargetDb" :min="-36" :max="-8" :step="1" :format="(v) => `${v} дБ`" />
              </div>
              <div v-if="!settings.basicUi" :class="settingRow">
                <AppLabel
                  text="Макс. усиление"
                  hint="Предел подъёма AGC. Большой поднимает и остатки шума; для тихого микрофона хватает 12–18 дБ."
                />
                <AppSlider v-model="settings.agcMaxGainDb" :min="0" :max="30" :step="1" :format="(v) => `${v} дБ`" />
              </div>
              <div :class="settingRow">
                <AppLabel
                  text="Лимитер"
                  hint="Не даёт сигналу выйти за 0 дБ и срезает пики. Лучше держать включённым."
                />
                <div class="flex justify-end"><AppSwitch v-model="settings.limiterEnabled" /></div>
              </div>
              <div :class="settingRow">
                <AppLabel
                  text="Комфортный шум"
                  hint="Тихий ровный фон в паузах вместо «звонкой» тишины. По умолчанию выключен."
                />
                <div class="flex justify-end"><AppSwitch v-model="settings.comfortNoiseEnabled" /></div>
              </div>
              <div v-if="!settings.basicUi && settings.comfortNoiseEnabled" :class="settingRow">
                <AppLabel text="Уровень шума" hint="Громкость комфортного шума: −60…−70 дБ почти не слышно." />
                <AppSlider v-model="settings.comfortNoiseDb" :min="-90" :max="-30" :step="1" :format="(v) => `${v} дБ`" />
              </div>
              <div v-if="!settings.basicUi" :class="settingRow">
                <AppLabel
                  text="Выходная громкость"
                  hint="Общее усиление после всей обработки. Меняет только громкость, шумодав не трогает."
                />
                <AppSlider v-model="settings.outputGainDb" :min="-18" :max="18" :step="0.5" :format="(v) => `${v.toFixed(1)} дБ`" />
              </div>
            </div>

            <!-- latency -->
            <div v-if="searching || settingsSection === 'latency'" class="sblock">
              <h2 class="mt-5 mb-1 text-[15px] font-semibold tracking-[-0.01em]">Задержка и нагрузка</h2>
              <div :class="settingRow">
                <AppLabel
                  text="Буфер автоматически"
                  hint="Подбирает минимальный буфер без пропусков: при пропуске +20 мс, после 90 с без пропусков −10 мс. Значение запоминается между запусками."
                />
                <div class="flex justify-end"><AppSwitch v-model="settings.bufferAuto" /></div>
              </div>
              <div :class="settingRow">
                <AppLabel
                  text="Буфер"
                  :hint="
                    settings.bufferAuto
                      ? 'Текущее значение подобрано автоматически. Выключите авто, чтобы задать вручную.'
                      : 'Запас в кольце вывода. Больше — стабильнее и без пропусков, но выше задержка.'
                  "
                />
                <AppSlider
                  v-if="!settings.bufferAuto"
                  v-model="settings.bufferMs"
                  :min="20"
                  :max="250"
                  :step="10"
                  :format="(v) => `${v} мс`"
                />
                <div v-else class="text-right font-mono text-[12px] tabular-nums">
                  {{ stats.bufferMs ? stats.bufferMs.toFixed(0) + " мс" : "—" }}
                  <span class="text-[var(--muted)]">· авто</span>
                </div>
              </div>
              <div :class="settingRow">
                <AppLabel
                  text="Блок модели"
                  hint="Размер блока VoxLock: меньше — ниже задержка и выше нагрузка. 60 мс — баланс."
                />
                <AppSlider v-model="settings.chunkMs" :min="20" :max="120" :step="20" :format="(v) => `${v} мс`" />
              </div>
              <div :class="settingRow">
                <AppLabel
                  text="Потоки модели"
                  hint="Сколько ядер отдаётся VoxLock. До 4 потоков время обработки падает почти вдвое, дальше почти не помогает. «Автоматически» подбирает число по конфигурации машины и размеру блока."
                />
                <div class="flex items-center justify-end gap-2.5">
                  <AppSwitch v-model="settings.autoThreads" />
                  <span class="text-[11.5px] text-[var(--muted)]">
                    Автоматически<span v-if="settings.autoThreads"> · {{ autoThreads }}</span>
                  </span>
                  <button
                    v-if="!settings.autoThreads"
                    :class="btnGhost"
                    class="!h-auto !px-2 !py-1"
                    title="Подставить рекомендованное число потоков"
                    @click="settings.denoiserThreads = autoThreads"
                  >
                    {{ autoThreads }}
                  </button>
                </div>
              </div>
              <div v-if="!settings.autoThreads" :class="settingRow">
                <AppLabel text="Сколько потоков" />
                <AppSlider
                  v-model="settings.denoiserThreads"
                  :min="1"
                  :max="maxThreads"
                  :step="1"
                  :format="(v) => `${v} из ${maxThreads}`"
                />
              </div>
              <p class="mt-2 text-[11.5px] leading-relaxed text-[var(--muted)]">
                Система:
                <span class="text-[var(--text)]">
                  {{ sysInfo?.logicalCores ?? "—" }} логических ядер<span v-if="sysInfo">
                    ({{ sysInfo.physicalCores }} физических)</span
                  >
                </span>
                · рекомендуется <span class="text-[var(--text)]">{{ autoThreads }}</span
                >. До четырёх потоков время обработки падает почти вдвое, выше — почти не помогает, но
                забирает ядра у игры.
              </p>
              <p v-if="slowConfig" class="mt-1 text-[12px] leading-relaxed text-[var(--danger)]">
                На блоке {{ settings.chunkMs }} мс с
                {{ settings.autoThreads ? autoThreads : settings.denoiserThreads }} поток(ами) модель не
                успевает за реальным временем — звук будет рваться. Поставьте 60 мс или больше потоков.
              </p>
            </div>

            <!-- control -->
            <div v-if="searching || settingsSection === 'control'" class="sblock">
              <h2 class="mt-5 mb-1 text-[15px] font-semibold tracking-[-0.01em]">Управление</h2>
              <div :class="settingRow">
                <AppLabel
                  text="Выключение микрофона"
                  hint="Глобальное сочетание работает из любой программы. «Удержание» — пока клавиша нажата, микрофон включён (push-to-talk). Escape отменяет запись сочетания."
                />
                <div class="flex items-center justify-end gap-2">
                  <button :class="capturingHotkey ? btnPrimary : btn" @click="startHotkeyCapture">
                    <AppIcon name="mic" :size="14" />
                    {{ capturingHotkey ? "Нажмите сочетание…" : settings.muteHotkey }}
                  </button>
                </div>
              </div>
              <div :class="settingRow">
                <AppLabel text="Режим клавиши" />
                <AppSelect
                  v-model="settings.hotkeyMode"
                  :options="[
                    { value: 'toggle', label: 'Переключение' },
                    { value: 'hold', label: 'Удержание (PTT)' },
                  ]"
                />
              </div>
            </div>

            <!-- diagnostics -->
            <div v-if="searching || settingsSection === 'diag'" class="sblock">
              <h2 class="mt-5 mb-2 text-[15px] font-semibold tracking-[-0.01em]">Диагностика</h2>
              <div class="kv">
                <span>Усиление выравнивания</span>
                <span :class="stats.levelerGainDb > 16 ? 'text-[var(--danger)]' : ''">
                  {{ stats.levelerGainDb.toFixed(1) }} дБ
                </span>
                <span>Потоков модели</span>
                <span>
                  {{ stats.threads || "—"
                  }}<span v-if="settings?.autoThreads && stats.threads" class="text-[var(--muted)]"> · авто</span>
                </span>
                <span>Сбросов модели</span>
                <span>{{ stats.guardResets }}</span>
                <span title="Сэмплы, которые не успели доехать до вывода; в скобках — примерная длительность. Растёт — обработка не успевает.">
                  Пропущено сэмплов
                </span>
                <span>
                  {{ stats.underruns
                  }}<span v-if="stats.underruns && stats.outRate" class="text-[var(--muted)]">
                    · ≈ {{ ((stats.underruns / stats.outRate) * 1000).toFixed(0) }} мс</span
                  >
                </span>
                <span title="Блоки, которые не влезли в кольцо вывода и были потеряны. Ноль — норма.">
                  Переполнений буфера
                </span>
                <span>{{ stats.overflows }}</span>
                <span title="Сэмплы микрофона, потерянные из-за переполнения входного кольца. Ноль — норма.">
                  Потеряно на входе
                </span>
                <span>{{ stats.captureDrops }}</span>
                <span>Подавление</span>
                <span>
                  {{ stats.suppressionDb > 0.5 ? "−" + stats.suppressionDb.toFixed(0) + " дБ" : "—" }}
                </span>
                <span>Пик входа</span>
                <span>{{ stats.inputPeakDb > -100 ? stats.inputPeakDb.toFixed(0) + " дБ" : "—" }}</span>
                <span title="Подстройка под расхождение часов микрофона и вывода. 0 ppm — конвертер работает ровно, темп и тембр не меняются.">
                  Расхождение часов
                </span>
                <span>
                  {{
                    Math.abs(stats.slewPpm) < 1
                      ? "0 ppm"
                      : (stats.slewPpm > 0 ? "+" : "−") + Math.abs(stats.slewPpm).toFixed(0) + " ppm"
                  }}
                </span>
              </div>
              <div class="mt-3 flex flex-wrap items-center gap-1">
                <button :class="btnGhost" @click="openLog">
                  <AppIcon name="file" :size="14" />
                  Открыть лог
                </button>
                <button :class="btnGhost" @click="showWizard = true">Мастер настройки</button>
              </div>
              <p class="mt-1 text-[11.5px] leading-relaxed text-[var(--muted)]">
                Лог пишется в data/app.log: старт и стоп движка, ошибки устройств, сбросы модели.
              </p>
            </div>

            <!-- app -->
            <div v-if="searching || settingsSection === 'app'" class="sblock">
              <h2 class="mt-5 mb-1 text-[15px] font-semibold tracking-[-0.01em]">Приложение</h2>
              <div :class="settingRow">
                <AppLabel text="Модель VoxLock" :hint="modelStatus" />
                <div class="flex justify-end">
                  <button :class="btn" :disabled="checkingModel" @click="checkModel">
                    <AppIcon v-if="checkingModel" name="spinner" :size="14" class="spin" />
                    {{ checkingModel ? "Проверяю…" : "Проверить обновления" }}
                  </button>
                </div>
              </div>
              <div :class="settingRow">
                <AppLabel text="Запускать с Windows" hint="Автозапуск при входе в систему: приложение стартует свёрнутым в трей." />
                <div class="flex justify-end"><AppSwitch v-model="autostart" @update:model-value="applyAutostart" /></div>
              </div>
              <div :class="settingRow">
                <AppLabel text="Свернуть в трей при закрытии" hint="Крестик прячет окно в трей, обработка при этом продолжает работать." />
                <div class="flex justify-end"><AppSwitch v-model="settings.closeToTray" /></div>
              </div>
              <div :class="settingRow">
                <AppLabel text="Запускать свёрнутым" hint="При старте окно не показывается: приложение сразу живёт в трее и обрабатывает звук." />
                <div class="flex justify-end"><AppSwitch v-model="settings.startMinimized" /></div>
              </div>
              <div :class="settingRow">
                <AppLabel text="Включать обработку сразу" hint="Как только виртуальный кабель доступен — обработка запускается без нажатия кнопки." />
                <div class="flex justify-end"><AppSwitch v-model="settings.autoStart" /></div>
              </div>
              <div :class="settingRow">
                <AppLabel text="Следовать системному микрофону" hint="Если в Windows выбран другой микрофон по умолчанию, приложение переключится на него." />
                <div class="flex justify-end"><AppSwitch v-model="settings.followDefaultInput" /></div>
              </div>
              <div :class="settingRow">
                <AppLabel text="Хранить записей A/B" hint="Сколько последних пар хранить в data/recordings; старые удаляются." />
                <AppSlider v-model="settings.keepRecordings" :min="5" :max="100" :step="5" :format="(v) => `${v}`" />
              </div>
              <div :class="settingRow">
                <AppLabel
                  text="Лицензии"
                  hint="Программа распространяется под MIT. Рядом с ней лежат тексты лицензий третьих сторон: моделей VoxLock, GTCRN, Silero и библиотек sherpa-onnx, ONNX Runtime."
                />
                <div class="flex justify-end">
                  <button :class="btn" @click="openLicenses">
                    <AppIcon name="file" :size="14" />
                    Открыть
                  </button>
                </div>
              </div>
              <div class="mt-4 flex flex-wrap items-center gap-1">
                <button :class="btnGhost" @click="exportSettings">Экспорт настроек</button>
                <button :class="btnGhost" @click="importSettings">Импорт настроек</button>
                <button :class="btnGhost" class="hover:!text-[var(--danger)]" @click="resetSettings">
                  Сбросить настройки
                </button>
              </div>
              <p class="mt-1 text-[11.5px] leading-relaxed text-[var(--muted)]">
                Экспорт кладёт файл в data/settings-export.json; для импорта положите его туда же.
              </p>
            </div>
          </div>
        </div>
  </div>
</template>