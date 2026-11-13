<script setup lang="ts">
import { inject } from "vue";
import AppIcon from "../components/AppIcon.vue";
import AppLabel from "../components/AppLabel.vue";
import AppSelect from "../components/AppSelect.vue";
import AppSlider from "../components/AppSlider.vue";
import AppSwitch from "../components/AppSwitch.vue";
import { APP_CONTEXT } from "../appContext";
import { LANGUAGE_OPTIONS } from "../i18n";

const { applyAutostart, applyPreset, appUpdateStatus, appVersion, autostart, autoThreads, btn, btnGhost, btnPrimary, capturingHotkey, checkAppUpdate, checkingApp, checkingModel, checkModel, error, evaluateSpeakerThreshold, evaluatingThreshold, exportSettings, importSettings, maxThreads, modelStatus, openLicenses, openLog, presets, resetSettings, restartSoon, saveDiagnostics, settingRow, settings, settingsSection, settingsSections, showWizard, slowConfig, speakerRecommendation, startHotkeyCapture, stats, switchSection, sysInfo, searching, settingsQuery, filterSettings, t, u } = inject(APP_CONTEXT)!;
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
                  :placeholder="t('settings.search')"
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
                {{ t("settings.advanced") }}
              </label>
            </div>

            <div v-if="!searching" class="mt-3 flex flex-wrap items-center gap-1.5">
              <span class="mr-1 text-[11.5px] text-[var(--muted)]">{{ t("settings.presets") }}</span>
              <button
                v-for="preset in presets"
                :key="preset.id"
                class="chip"
                @click="applyPreset(preset.patch, preset.label)"
              >
                {{ preset.label }}
              </button>
            </div>

            <!-- voice -->
            <div v-if="searching || settingsSection === 'voice'" class="sblock">
              <h2 class="mt-5 mb-1 text-[15px] font-semibold tracking-[-0.01em]">{{ t("section.voice") }}</h2>
              <div :class="settingRow">
                <AppLabel
                  :text="t('settings.voice.onlyMine')"
                  :hint="t('settings.voice.onlyMineHint')"
                />
                <div class="flex justify-end">
                  <AppSwitch v-model="settings.voiceIsolation" @update:model-value="() => restartSoon()" />
                </div>
              </div>
              <div v-if="settings.voiceIsolation" :class="settingRow">
                <AppLabel
                  :text="t('settings.voice.speakerCheck')"
                  :hint="t('settings.voice.speakerCheckHint')"
                />
                <div class="flex justify-end">
                  <AppSwitch v-model="settings.speakerCheck" @update:model-value="() => restartSoon()" />
                </div>
              </div>
              <div v-if="settings.voiceIsolation && settings.speakerCheck" :class="settingRow">
                <AppLabel
                  :text="t('settings.voice.speakerGuard')"
                  :hint="t('settings.voice.speakerGuardHint')"
                />
                <div class="flex justify-end">
                  <AppSwitch v-model="settings.speakerGuard" />
                </div>
              </div>
              <div v-if="settings.voiceIsolation && settings.speakerCheck && settings.speakerGuard" :class="settingRow">
                <AppLabel
                  :text="t('settings.voice.speakerThreshold')"
                  :hint="t('settings.voice.speakerThresholdHint')"
                />
                <AppSlider
                  :label="t('settings.voice.speakerThreshold')"
                  v-model="settings.speakerThreshold"
                  :min="0.05"
                  :max="0.95"
                  :step="0.05"
                  :format="(v) => `${Math.round(v * 100)} %`"
                />
              </div>
              <div v-if="settings.voiceIsolation" :class="settingRow">
                <AppLabel
                  :text="t('settings.voice.strength')"
                  :hint="t('settings.voice.strengthHint')"
                />
                <AppSlider
                  :label="t('settings.voice.strength')"
                  v-model="settings.isolationStrength"
                  :min="0"
                  :max="1"
                  :step="0.05"
                  :format="(v) => v.toFixed(2)"
                />
              </div>
              <div v-if="settings.voiceIsolation" :class="settingRow">
                <AppLabel
                  :text="t('settings.voice.maxSuppression')"
                  :hint="t('settings.voice.maxSuppressionHint')"
                />
                <AppSlider
                  :label="t('settings.voice.maxSuppression')"
                  v-model="settings.maxSuppressionDb"
                  :min="0"
                  :max="40"
                  :step="1"
                  :format="(v) => (v <= 0 ? t('settings.voice.noLimit') : `${v} ${u.db}`)"
                />
              </div>
              <div v-if="settings.voiceIsolation" :class="settingRow">
                <AppLabel
                  :text="t('settings.voice.mode')"
                  :hint="t('settings.voice.modeHint')"
                />
                <AppSelect
                  v-model="settings.mode"
                  :options="[
                    { value: 'profile', label: t('settings.voice.modeProfile') },
                    { value: 'auto', label: t('settings.voice.modeAuto') },
                  ]"
                  @update:model-value="() => restartSoon()"
                />
              </div>
              <div :class="settingRow">
                <AppLabel
                  :text="t('settings.voice.tone')"
                  :hint="t('settings.voice.toneHint')"
                />
                <div class="flex justify-end"><AppSwitch v-model="settings.toneEnabled" /></div>
              </div>
              <div v-if="settings.toneEnabled" :class="settingRow">
                <AppLabel
                  :text="t('settings.voice.body')"
                  :hint="t('settings.voice.bodyHint')"
                />
                <AppSlider
                  :label="t('settings.voice.body')"
                  v-model="settings.toneLowDb" :min="-6" :max="3" :step="0.5" :format="(v) => `${v.toFixed(1)} ${u.db}`" />
              </div>
              <div v-if="settings.toneEnabled" :class="settingRow">
                <AppLabel
                  :text="t('settings.voice.presence')"
                  :hint="t('settings.voice.presenceHint')"
                />
                <AppSlider
                  :label="t('settings.voice.presence')"
                  v-model="settings.toneHighDb" :min="-6" :max="6" :step="0.5" :format="(v) => `${v.toFixed(1)} ${u.db}`" />
              </div>
              <div :class="settingRow">
                <AppLabel
                  :text="t('settings.voice.deess')"
                  :hint="t('settings.voice.deessHint')"
                />
                <div class="flex justify-end"><AppSwitch v-model="settings.deessEnabled" /></div>
              </div>
              <div v-if="settings.deessEnabled" :class="settingRow">
                <AppLabel
                  :text="t('settings.voice.deessAmount')"
                  :hint="t('settings.voice.deessAmountHint')"
                />
                <AppSlider
                  :label="t('settings.voice.deessAmount')"
                  v-model="settings.deessDb" :min="0" :max="12" :step="1" :format="(v) => `${v} ${u.db}`" />
              </div>
            </div>

            <!-- clean -->
            <div v-if="searching || settingsSection === 'clean'" class="sblock">
              <h2 class="mt-5 mb-1 text-[15px] font-semibold tracking-[-0.01em]">{{ t("section.clean") }}</h2>
              <div :class="settingRow">
                <AppLabel
                  :text="t('settings.clean.leveling')"
                  :hint="t('settings.clean.levelingHint')"
                />
                <div class="flex justify-end"><AppSwitch v-model="settings.inputLeveling" /></div>
              </div>
              <div v-if="!settings.basicUi" :class="settingRow">
                <AppLabel
                  :text="t('settings.clean.inputGain')"
                  :hint="t('settings.clean.inputGainHint')"
                />
                <AppSlider
                  :label="t('settings.clean.inputGain')"
                  v-model="settings.inputGainDb"
                  :min="-12"
                  :max="24"
                  :step="1"
                  :format="(v) => `${v > 0 ? '+' : ''}${v} ${u.db}`"
                />
              </div>
              <div :class="settingRow">
                <AppLabel
                  :text="t('settings.clean.residual')"
                  :hint="t('settings.clean.residualHint')"
                />
                <AppSelect
                  v-model="settings.residualDenoise"
                  :options="[
                    { value: 'off', label: t('settings.clean.residualOff') },
                    { value: 'gtcrn', label: t('settings.clean.residualGtcrn') },
                    { value: 'dpdfnet', label: t('settings.clean.residualDpdfnet') },
                    { value: 'ulunas', label: t('settings.clean.residualUlunas') },
                  ]"
                  @update:model-value="() => restartSoon()"
                />
              </div>
              <div v-if="settings.residualDenoise !== 'off'" :class="settingRow">
                <AppLabel
                  :text="t('settings.clean.residualMix')"
                  :hint="t('settings.clean.residualMixHint')"
                />
                <AppSlider
                  :label="t('settings.clean.residualMix')"
                  v-model="settings.residualMix"
                  :min="0"
                  :max="1"
                  :step="0.05"
                  :format="(v) => `${Math.round(v * 100)} %`"
                />
              </div>
              <div :class="settingRow">
                <AppLabel
                  :text="t('settings.clean.vad')"
                  :hint="t('settings.clean.vadHint')"
                />
                <div class="flex justify-end"><AppSwitch v-model="settings.vadGate" /></div>
              </div>
              <div :class="settingRow">
                <AppLabel
                  :text="t('settings.clean.highPass')"
                  :hint="t('settings.clean.highPassHint')"
                />
                <div class="flex justify-end"><AppSwitch v-model="settings.highPassEnabled" /></div>
              </div>
              <div v-if="!settings.basicUi" :class="settingRow">
                <AppLabel
                  :text="t('settings.clean.cutoff')"
                  :hint="t('settings.clean.cutoffHint')"
                />
                <AppSlider
                  v-model="settings.highPassHz" :min="20" :max="300" :step="5" :format="(v) => `${v} ${u.hz}`" />
              </div>
              <div :class="settingRow">
                <AppLabel
                  :text="t('settings.clean.air')"
                  :hint="t('settings.clean.airHint')"
                />
                <AppSlider
                  :label="t('settings.clean.air')"
                  v-model="settings.airAmount" :min="0" :max="1" :step="0.05" :format="(v) => `${Math.round(v * 100)} %`" />
              </div>
              <div :class="settingRow">
                <AppLabel
                  :text="t('settings.clean.gate')"
                  :hint="t('settings.clean.gateHint')"
                />
                <div class="flex justify-end"><AppSwitch v-model="settings.gateEnabled" /></div>
              </div>
              <div v-if="!settings.basicUi" :class="settingRow">
                <AppLabel
                  :text="t('settings.clean.gateThreshold')"
                  :hint="t('settings.clean.gateThresholdHint')"
                />
                <AppSlider
                  :label="t('settings.clean.gateThreshold')"
                  v-model="settings.gateThresholdDb" :min="-90" :max="-20" :step="1" :format="(v) => `${v} ${u.db}`" />
              </div>
              <div v-if="!settings.basicUi" :class="settingRow">
                <AppLabel
                  :text="t('settings.clean.gateRatio')"
                  :hint="t('settings.clean.gateRatioHint')"
                />
                <AppSlider
                  :label="t('settings.clean.gateRatio')"
                  v-model="settings.gateRatio" :min="1" :max="10" :step="0.5" :format="(v) => v.toFixed(1)" />
              </div>
              <div v-if="!settings.basicUi" :class="settingRow">
                <AppLabel
                  :text="t('settings.clean.attack')"
                  :hint="t('settings.clean.attackHint')"
                />
                <AppSlider
                  :label="t('settings.clean.attack')"
                  v-model="settings.gateAttackMs" :min="1" :max="100" :step="1" :format="(v) => `${v} ${u.ms}`" />
              </div>
              <div v-if="!settings.basicUi" :class="settingRow">
                <AppLabel
                  :text="t('settings.clean.release')"
                  :hint="t('settings.clean.releaseHint')"
                />
                <AppSlider
                  :label="t('settings.clean.release')"
                  v-model="settings.gateReleaseMs" :min="20" :max="600" :step="10" :format="(v) => `${v} ${u.ms}`" />
              </div>
              <div v-if="!settings.basicUi" :class="settingRow">
                <AppLabel
                  :text="t('settings.clean.hold')"
                  :hint="t('settings.clean.holdHint')"
                />
                <AppSlider
                  :label="t('settings.clean.hold')"
                  v-model="settings.gateHoldMs" :min="0" :max="500" :step="10" :format="(v) => `${v} ${u.ms}`" />
              </div>
              <div :class="settingRow">
                <AppLabel
                  :text="t('settings.clean.agc')"
                  :hint="t('settings.clean.agcHint')"
                />
                <div class="flex justify-end"><AppSwitch v-model="settings.agcEnabled" /></div>
              </div>
              <div v-if="!settings.basicUi" :class="settingRow">
                <AppLabel
                  :text="t('settings.clean.agcTarget')"
                  :hint="t('settings.clean.agcTargetHint')"
                />
                <AppSlider
                  :label="t('settings.clean.agcTarget')"
                  v-model="settings.agcTargetDb" :min="-36" :max="-8" :step="1" :format="(v) => `${v} ${u.db}`" />
              </div>
              <div v-if="!settings.basicUi" :class="settingRow">
                <AppLabel
                  :text="t('settings.clean.agcMax')"
                  :hint="t('settings.clean.agcMaxHint')"
                />
                <AppSlider
                  :label="t('settings.clean.agcMax')"
                  v-model="settings.agcMaxGainDb" :min="0" :max="30" :step="1" :format="(v) => `${v} ${u.db}`" />
              </div>
              <div :class="settingRow">
                <AppLabel
                  :text="t('settings.clean.limiter')"
                  :hint="t('settings.clean.limiterHint')"
                />
                <div class="flex justify-end"><AppSwitch v-model="settings.limiterEnabled" /></div>
              </div>
              <div :class="settingRow">
                <AppLabel
                  :text="t('settings.clean.comfort')"
                  :hint="t('settings.clean.comfortHint')"
                />
                <div class="flex justify-end"><AppSwitch v-model="settings.comfortNoiseEnabled" /></div>
              </div>
              <div v-if="!settings.basicUi && settings.comfortNoiseEnabled" :class="settingRow">
                <AppLabel :text="t('settings.clean.comfortLevel')" :hint="t('settings.clean.comfortLevelHint')" />
                <AppSlider
                  :label="t('settings.clean.comfortLevel')"
                  v-model="settings.comfortNoiseDb" :min="-90" :max="-30" :step="1" :format="(v) => `${v} ${u.db}`" />
              </div>
              <div v-if="!settings.basicUi" :class="settingRow">
                <AppLabel
                  :text="t('settings.clean.outputVolume')"
                  :hint="t('settings.clean.outputVolumeHint')"
                />
                <AppSlider
                  :label="t('sound.outputVolume')"
                  v-model="settings.outputGainDb" :min="-18" :max="18" :step="0.5" :format="(v) => `${v.toFixed(1)} ${u.db}`" />
              </div>
              <div :class="settingRow">
                <AppLabel
                  :text="t('settings.clean.loudnessTarget')"
                  :hint="t('settings.clean.loudnessTargetHint')"
                />
                <AppSelect
                  :model-value="String(settings.loudnessTargetDb)"
                  :options="[
                    { value: '0', label: t('settings.clean.loudnessOff') },
                    { value: '-16', label: t('settings.clean.loudnessStream') },
                    { value: '-20', label: t('settings.clean.loudnessCall') },
                  ]"
                  @update:model-value="(value: string) => (settings!.loudnessTargetDb = Number(value))"
                />
              </div>
            </div>

            <!-- latency -->
            <div v-if="searching || settingsSection === 'latency'" class="sblock">
              <h2 class="mt-5 mb-1 text-[15px] font-semibold tracking-[-0.01em]">{{ t("section.latency") }}</h2>
              <div :class="settingRow">
                <AppLabel
                  :text="t('settings.latency.bufferAuto')"
                  :hint="t('settings.latency.bufferAutoHint')"
                />
                <div class="flex justify-end"><AppSwitch v-model="settings.bufferAuto" /></div>
              </div>
              <div :class="settingRow">
                <AppLabel
                  :text="t('settings.latency.buffer')"
                  :hint="
                    settings.bufferAuto
                      ? t('settings.latency.bufferAutoHintShort')
                      : t('settings.latency.bufferHint')
                  "
                />
                <AppSlider
                  v-if="!settings.bufferAuto"
                  :label="t('settings.latency.buffer')"
                  v-model="settings.bufferMs"
                  :min="40"
                  :max="250"
                  :step="10"
                  :format="(v) => `${v} ${u.ms}`"
                />
                <div v-else class="text-right font-mono text-[12px] tabular-nums">
                  {{ stats.bufferMs ? stats.bufferMs.toFixed(0) + " " + u.ms : "—" }}
                  <span class="text-[var(--muted)]">· {{ t("common.auto") }}</span>
                </div>
              </div>
              <div :class="settingRow">
                <AppLabel
                  :text="t('settings.latency.block')"
                  :hint="t('settings.latency.blockHint')"
                />
                <AppSlider
                  :label="t('settings.latency.block')"
                  v-model="settings.chunkMs" :min="20" :max="120" :step="20" :format="(v) => `${v} ${u.ms}`" />
              </div>
              <div :class="settingRow">
                <AppLabel
                  :text="t('settings.latency.threads')"
                  :hint="t('settings.latency.threadsHint')"
                />
                <div class="flex items-center justify-end gap-2.5">
                  <AppSwitch v-model="settings.autoThreads" />
                  <span class="text-[11.5px] text-[var(--muted)]">
                    {{ t("settings.latency.auto") }}<span v-if="settings.autoThreads"> · {{ autoThreads }}</span>
                  </span>
                  <button
                    v-if="!settings.autoThreads"
                    :class="btnGhost"
                    class="!h-auto !px-2 !py-1"
                    :title="t('settings.latency.autoHint')"
                    @click="settings.denoiserThreads = autoThreads"
                  >
                    {{ autoThreads }}
                  </button>
                </div>
              </div>
              <div v-if="!settings.autoThreads" :class="settingRow">
                <AppLabel :text="t('settings.latency.count')" />
                <AppSlider
                  :label="t('settings.latency.count')"
                  v-model="settings.denoiserThreads"
                  :min="1"
                  :max="maxThreads"
                  :step="1"
                  :format="(v) => t('settings.latency.of', { v, max: maxThreads })"
                />
              </div>
              <p class="mt-2 text-[11.5px] leading-relaxed text-[var(--muted)]">
                {{ t("settings.latency.system") }}
                <span class="text-[var(--text)]">
                  {{ sysInfo?.logicalCores ?? "—" }} {{ t("settings.latency.logical") }}<span v-if="sysInfo">
                    ({{ sysInfo.physicalCores }} {{ t("settings.latency.physical") }})</span
                  >
                </span>
                · {{ t("settings.latency.recommended") }} <span class="text-[var(--text)]">{{ autoThreads }}</span
                >. {{ t("settings.latency.why") }}
              </p>
              <p v-if="slowConfig" class="mt-1 text-[12px] leading-relaxed text-[var(--danger)]">
                {{
                  t("settings.latency.slow", {
                    block: settings.chunkMs,
                    threads: settings.autoThreads ? autoThreads : settings.denoiserThreads,
                  })
                }}
              </p>
            </div>

            <!-- control -->
            <div v-if="searching || settingsSection === 'control'" class="sblock">
              <h2 class="mt-5 mb-1 text-[15px] font-semibold tracking-[-0.01em]">{{ t("section.control") }}</h2>
              <div :class="settingRow">
                <AppLabel
                  :text="t('settings.control.mute')"
                  :hint="t('settings.control.muteHint')"
                />
                <div class="flex items-center justify-end gap-2">
                  <button :class="capturingHotkey ? btnPrimary : btn" @click="startHotkeyCapture">
                    <AppIcon name="mic" :size="14" />
                    {{ capturingHotkey ? t("settings.control.capture") : settings.muteHotkey }}
                  </button>
                </div>
              </div>
              <div :class="settingRow">
                <AppLabel :text="t('settings.control.mode')" />
                <AppSelect
                  v-model="settings.hotkeyMode"
                  :options="[
                    { value: 'toggle', label: t('settings.control.toggle') },
                    { value: 'hold', label: t('settings.control.hold') },
                  ]"
                />
              </div>
            </div>

            <!-- diagnostics -->
            <div v-if="searching || settingsSection === 'diag'" class="sblock">
              <h2 class="mt-5 mb-2 text-[15px] font-semibold tracking-[-0.01em]">{{ t("section.diag") }}</h2>
              <div class="kv">
                <span>{{ t("settings.diag.leveler") }}</span>
                <span :class="stats.levelerGainDb > 16 ? 'text-[var(--danger)]' : ''">
                  {{ stats.levelerGainDb.toFixed(1) }} {{ u.db }}
                </span>
                <span>{{ t("settings.diag.threads") }}</span>
                <span>
                  {{ stats.threads || "—"
                  }}<span v-if="settings?.autoThreads && stats.threads" class="text-[var(--muted)]"> · {{ t("common.auto") }}</span>
                </span>
                <span>{{ t("settings.diag.resets") }}</span>
                <span>{{ stats.guardResets }}</span>
                <span :title="t('settings.diag.underrunsHint')">
                  {{ t("settings.diag.underruns") }}
                </span>
                <span>
                  {{ stats.underruns
                  }}<span v-if="stats.underruns && stats.outRate" class="text-[var(--muted)]">
                    · ≈ {{ ((stats.underruns / stats.outRate) * 1000).toFixed(0) }} {{ u.ms }}</span
                  >
                </span>
                <span :title="t('settings.diag.overflowsHint')">
                  {{ t("settings.diag.overflows") }}
                </span>
                <span>{{ stats.overflows }}</span>
                <span :title="t('settings.diag.captureDropsHint')">
                  {{ t("settings.diag.captureDrops") }}
                </span>
                <span>{{ stats.captureDrops }}</span>
                <span>{{ t("readout.suppression") }}</span>
                <span>
                  {{ stats.suppressionDb > 0.5 ? "−" + stats.suppressionDb.toFixed(0) + " " + u.db : "—" }}
                </span>
                <span>{{ t("settings.diag.inputPeak") }}</span>
                <span>{{ stats.inputPeakDb > -100 ? stats.inputPeakDb.toFixed(0) + " " + u.db : "—" }}</span>
                <span>{{ t("settings.diag.outputPeak") }}</span>
                <span :class="stats.outputPeakDb > -1 ? 'text-[var(--danger)]' : ''">
                  {{ stats.outputPeakDb > -100 ? stats.outputPeakDb.toFixed(0) + " " + u.db : "—" }}
                </span>
                <span :title="t('settings.diag.lufsHint')">{{ t("settings.diag.lufs") }}</span>
                <span>{{ stats.lufs > -100 ? stats.lufs.toFixed(0) + " LUFS" : "—" }}</span>
                <span :title="t('settings.diag.truePeakHint')">{{ t("settings.diag.truePeak") }}</span>
                <span :class="stats.truePeakDb > -1 ? 'text-[var(--danger)]' : ''">
                  {{ stats.truePeakDb > -100 ? stats.truePeakDb.toFixed(1) + " dBTP" : "—" }}
                </span>
                <span :title="t('settings.diag.speakerHint')">{{ t("settings.diag.speaker") }}</span>
                <span :class="stats.speakerConfidence >= 0 && stats.speakerConfidence < 0.5 ? 'text-[var(--danger)]' : ''">
                  {{
                    stats.speakerConfidence >= 0
                      ? (stats.speakerConfidence * 100).toFixed(0) + " %"
                      : "—"
                  }}
                </span>
                <span :title="t('settings.diag.clockHint')">
                  {{ t("settings.diag.clock") }}
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
                  {{ t("settings.diag.log") }}
                </button>
                <button :class="btnGhost" :title="t('settings.diag.reportHint')" @click="saveDiagnostics">
                  <AppIcon name="file" :size="14" />
                  {{ t("settings.diag.report") }}
                </button>
                <button
                  :class="btnGhost"
                  :title="t('settings.diag.thresholdHint')"
                  :disabled="evaluatingThreshold"
                  @click="evaluateSpeakerThreshold"
                >
                  <AppIcon :name="evaluatingThreshold ? 'spinner' : 'activity'" :size="14" :class="{ spin: evaluatingThreshold }" />
                  {{ evaluatingThreshold ? t("settings.diag.thresholdRunning") : t("settings.diag.threshold") }}
                </button>
                <button :class="btnGhost" @click="showWizard = true">{{ t("settings.diag.wizard") }}</button>
              </div>
              <p class="mt-1 text-[11.5px] leading-relaxed text-[var(--muted)]">
                {{ t("settings.diag.logHint") }}
              </p>
              <p v-if="speakerRecommendation" class="mt-2 text-[11.5px] leading-relaxed text-[var(--muted)]">
                {{
                  t("settings.diag.thresholdResult", {
                    takes: speakerRecommendation.takes,
                    p5: (speakerRecommendation.p5 * 100).toFixed(0),
                    median: (speakerRecommendation.median * 100).toFixed(0),
                    threshold: (speakerRecommendation.threshold * 100).toFixed(0),
                  })
                }}
                <button
                  class="ml-1 cursor-pointer text-[var(--accent)] hover:underline"
                  @click="settings.speakerThreshold = speakerRecommendation.threshold"
                >
                  {{ t("settings.diag.thresholdApply") }}
                </button>
              </p>
            </div>

            <!-- app -->
            <div v-if="searching || settingsSection === 'app'" class="sblock">
              <h2 class="mt-5 mb-1 text-[15px] font-semibold tracking-[-0.01em]">{{ t("section.app") }}</h2>
              <div :class="settingRow">
                <AppLabel :text="t('settings.language')" :hint="t('settings.languageHint')" />
                <AppSelect v-model="settings.language" :options="LANGUAGE_OPTIONS" />
              </div>
              <div :class="settingRow">
                <AppLabel :text="t('settings.theme')" :hint="t('settings.themeHint')" />
                <AppSelect
                  v-model="settings.theme"
                  :options="[
                    { value: 'system', label: t('settings.theme.system') },
                    { value: 'dark', label: t('settings.theme.dark') },
                    { value: 'light', label: t('settings.theme.light') },
                  ]"
                />
              </div>
              <div :class="settingRow">
                <AppLabel
                  :text="t('settings.app.version')"
                  :hint="appUpdateStatus || t('settings.app.versionHint', { version: appVersion || '—' })"
                />
                <div class="flex justify-end">
                  <button :class="btn" :disabled="checkingApp" @click="checkAppUpdate">
                    <AppIcon v-if="checkingApp" name="spinner" :size="14" class="spin" />
                    {{ checkingApp ? t("settings.app.checking") : t("settings.app.check") }}
                  </button>
                </div>
              </div>
              <div :class="settingRow">
                <AppLabel :text="t('model.label')" :hint="modelStatus" />
                <div class="flex justify-end">
                  <button :class="btn" :disabled="checkingModel" @click="checkModel">
                    <AppIcon v-if="checkingModel" name="spinner" :size="14" class="spin" />
                    {{ checkingModel ? t("model.checking") : t("model.check") }}
                  </button>
                </div>
              </div>
              <div :class="settingRow">
                <AppLabel :text="t('settings.app.autostart')" :hint="t('settings.app.autostartHint')" />
                <div class="flex justify-end"><AppSwitch v-model="autostart" @update:model-value="applyAutostart" /></div>
              </div>
              <div :class="settingRow">
                <AppLabel :text="t('settings.app.tray')" :hint="t('settings.app.trayHint')" />
                <div class="flex justify-end"><AppSwitch v-model="settings.closeToTray" /></div>
              </div>
              <div :class="settingRow">
                <AppLabel :text="t('settings.app.minimized')" :hint="t('settings.app.minimizedHint')" />
                <div class="flex justify-end"><AppSwitch v-model="settings.startMinimized" /></div>
              </div>
              <div :class="settingRow">
                <AppLabel :text="t('settings.app.autoStart')" :hint="t('settings.app.autoStartHint')" />
                <div class="flex justify-end"><AppSwitch v-model="settings.autoStart" /></div>
              </div>
              <div :class="settingRow">
                <AppLabel :text="t('settings.app.followMic')" :hint="t('settings.app.followMicHint')" />
                <div class="flex justify-end"><AppSwitch v-model="settings.followDefaultInput" /></div>
              </div>
              <div :class="settingRow">
                <AppLabel :text="t('settings.app.keepAb')" :hint="t('settings.app.keepAbHint')" />
                <AppSlider
                  :label="t('settings.app.keepAb')"
                  v-model="settings.keepRecordings" :min="5" :max="100" :step="5" :format="(v) => `${v}`" />
              </div>
              <div :class="settingRow">
                <AppLabel
                  :text="t('settings.app.licenses')"
                  :hint="t('settings.app.licensesHint')"
                />
                <div class="flex justify-end">
                  <button :class="btn" @click="openLicenses">
                    <AppIcon name="file" :size="14" />
                    {{ t("common.open") }}
                  </button>
                </div>
              </div>
              <div class="mt-4 flex flex-wrap items-center gap-1">
                <button :class="btnGhost" @click="exportSettings">{{ t("settings.app.export") }}</button>
                <button :class="btnGhost" @click="importSettings">{{ t("settings.app.import") }}</button>
                <button :class="btnGhost" class="hover:!text-[var(--danger)]" @click="resetSettings">
                  {{ t("settings.app.reset") }}
                </button>
              </div>
              <p class="mt-1 text-[11.5px] leading-relaxed text-[var(--muted)]">
                {{ t("settings.app.exportHint") }}
              </p>
            </div>
          </div>
        </div>
  </div>
</template>