<script setup lang="ts">
import { computed, inject, onUnmounted, ref } from "vue";
import AppIcon from "../components/AppIcon.vue";
import AppSelect from "../components/AppSelect.vue";
import AppSlider from "../components/AppSlider.vue";
import AppSwitch from "../components/AppSwitch.vue";
import { APP_CONTEXT } from "../appContext";

const { abBlind, abClean, abDelta, abFolder, abMetrics, abNormalize, abProgress, abRaw, abRunning, abSide, abSpeaker, abVisible, askApplyTake, btn, btnGhost, btnPrimary, busy, cableMessage, cableOk, calibrateRoom, calibrating, checkAbSpeaker, deleteProfile, formatStamp, history, inputChannels, inputDb, inputIsCable, inputLevel, inputOptions, installCable, installingCable, matchLevel, micReady, outputDb, outputLevel, outputOptions, pickInput, pickOutput, pickProfile, playAb, playingKey, playProfile, probeHint, probeLeft, probeMic, probeResult, probing, profileInfo, profileMeta, profileMismatch, profileOptions, profileWeak, readouts, reconnecting, recording, recordLeft, recordProfile, refreshDevicesManually, refreshing, refreshProfiles, removeRecording, renameProfile, running, selectedInput, selectedOutput, selectedProfile, selectRecording, settings, shortName, shownSpectrum, spectrumLive, starting, stats, tab, t, testMode, testOutput, toggleAb, togglePower, toggleTest, u } = inject(APP_CONTEXT)!;

const renaming = ref(false);
const nameDraft = ref("");

function startRename() {
  nameDraft.value = profileInfo?.value?.name || "";
  renaming.value = true;
}

async function saveRename() {
  await renameProfile(nameDraft.value);
  renaming.value = false;
}

const channelOptions = computed(() => [
  { value: "-1", label: t("sound.channelMix") },
  ...Array.from({ length: inputChannels.value }, (_, index) => ({
    value: String(index),
    label: t("sound.channelN", { n: index + 1 }),
  })),
]);

/// The take currently shown in the card, so its settings snapshot can be
/// applied without hunting for the timestamp.
const selectedTake = computed(
  () => history.value.find((entry) => entry.raw === abRaw.value) ?? null,
);

/// The real capture format, not a hard-coded caption: a 44.1 kHz stereo
/// interface used to be labelled "48 kHz · mono".
const format = computed(() => {
  const rate = stats.value.inRate;
  const rateText = rate
    ? `${(rate / 1000).toFixed(rate % 1000 === 0 ? 0 : 1)} ${u.khz}`
    : "—";
  const channels = stats.value.inChannels;
  const channelText =
    channels > 1 ? t("sound.channels", { n: channels }) : t("sound.mono");
  return `${rateText} · ${channelText}`;
});

/// The trash sits next to "play", so a stray click used to delete the profile
/// outright. The first click only arms the button for three seconds.
const confirmDelete = ref(false);
let confirmTimer: number | undefined;
function askDeleteProfile() {
  if (!confirmDelete.value) {
    confirmDelete.value = true;
    if (confirmTimer) window.clearTimeout(confirmTimer);
    confirmTimer = window.setTimeout(() => (confirmDelete.value = false), 3000);
    return;
  }
  if (confirmTimer) window.clearTimeout(confirmTimer);
  confirmDelete.value = false;
  void deleteProfile();
}
onUnmounted(() => {
  if (confirmTimer) window.clearTimeout(confirmTimer);
});

/// Which denoiser actually sits after VoxLock: the chain has to name the one
/// the user picked, not the model it used to ship with.
const noiseStage = computed(() => {
  switch (settings.residualDenoise) {
    case "dpdfnet":
      return "DPDFNet";
    case "ulunas":
      return "UL-UNAS";
    case "gtcrn":
      return "GTCRN";
    default:
      return t("sound.stageNoise");
  }
});
</script>

<template>
  <div class="pt-4">
        <section class="overflow-hidden rounded-[14px] border border-[var(--stroke)] bg-[var(--surface)]">
          <div class="grid grid-cols-[auto_minmax(0,1fr)] items-center gap-6 px-[22px] pt-4">
            <div class="flex flex-col items-center">
              <button
                class="power"
                :data-on="running"
                :disabled="busy || starting || (!micReady && !running)"
                :title="
                  running
                    ? t('sound.powerOff')
                    : micReady
                      ? t('sound.powerOn')
                      : t('sound.powerNoMic')
                "
                @click="togglePower"
              >
                <AppIcon v-if="starting" name="spinner" :size="26" :stroke="2" class="spin" />
                <AppIcon v-else name="power" :size="26" :stroke="2" />
              </button>
              <span class="mt-2 text-[11.5px] text-[var(--muted)]">
                {{ running ? t("sound.turnOff") : starting ? t("sound.starting") : micReady ? t("sound.turnOn") : t("sound.noMicShort") }}
              </span>
            </div>

            <div
              class="flex min-w-0 items-center justify-between gap-6"
            >
              <div class="min-w-0">
                <div class="mb-[3px] text-[11.5px] text-[var(--muted)]">{{ t("sound.input") }}</div>
                <div class="truncate text-[13px]" :title="selectedInput">
                  {{ shortName(selectedInput) || t("common.none") }}
                </div>
                <div class="mt-[7px] flex items-center gap-2">
                  <span class="meter"><i :style="{ width: inputLevel * 100 + '%' }"></i></span>
                  <span class="flex w-[54px] shrink-0 justify-end font-mono text-[10.5px] tabular-nums">
                    <span v-if="stats.inputPeakDb > -1" class="text-[var(--danger)]">{{ t("sound.clipping") }}</span>
                    <span v-else class="text-[var(--muted)]">{{ inputDb }}</span>
                  </span>
                </div>
              </div>

              <div class="min-w-0">
                <div class="mb-[3px] text-[11.5px] text-[var(--muted)]">{{ t("sound.processing") }}</div>
                <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-[11.5px] text-[var(--muted)]">
                  <span class="flex items-center gap-1.5">
                    <span class="dot" :data-off="!settings.voiceIsolation"></span>VoxLock
                  </span>
                  <span class="flex items-center gap-1.5">
                    <span class="dot" :data-off="settings.residualDenoise === 'off'"></span>{{ noiseStage }}
                  </span>
                  <span class="flex items-center gap-1.5">
                    <span class="dot" :data-off="!settings.vadGate"></span>VAD
                  </span>
                </div>
              </div>

              <div class="min-w-0">
                <div class="mb-[3px] text-[11.5px] text-[var(--muted)]">{{ t("sound.output") }}</div>
                <div class="truncate text-[13px]" :title="testMode ? testOutput : 'CABLE Input'">
                  {{ testMode ? shortName(testOutput) || t("common.outputDevice") : "CABLE Input" }}
                </div>
                <div class="mt-[7px] flex items-center gap-2">
                  <span class="meter"><i :style="{ width: outputLevel * 100 + '%' }"></i></span>
                  <span class="w-[54px] shrink-0 text-right font-mono text-[10.5px] tabular-nums text-[var(--muted)]">{{ outputDb }}</span>
                </div>
              </div>
            </div>
          </div>

          <div class="mt-4 flex items-stretch border-t border-[var(--stroke)]">
            <div class="flex flex-none items-stretch border-r border-[var(--stroke)] px-[22px] py-2.5">
              <div
                v-for="item in readouts"
                :key="item.label"
                class="border-l border-[var(--stroke)] px-4 first:border-l-0 first:pl-0 last:pr-0"
              >
                <div class="font-mono text-[20px] leading-none tabular-nums">
                  {{ item.value
                  }}<span v-if="item.unit" class="ml-[3px] text-[10.5px] text-[var(--muted)]">{{ item.unit }}</span>
                </div>
                <div class="mt-1.5 text-[11px] text-[var(--muted)]">{{ item.label }}</div>
              </div>
            </div>

            <div class="relative flex min-w-0 flex-1 items-end gap-[3px] px-[22px] py-2.5">
              <span class="pointer-events-none absolute inset-x-[22px] bottom-8 h-px bg-[var(--stroke)] opacity-70"></span>
              <span class="pointer-events-none absolute inset-x-[22px] bottom-12 h-px bg-[var(--stroke)] opacity-70"></span>
              <div
                v-for="(value, index) in shownSpectrum"
                :key="index"
                class="min-h-[2px] flex-1 rounded-t-[1px] bg-[var(--accent)] transition-[height] duration-150"
                :style="{
                  height: spectrumLive ? Math.max(3, value * 100) + '%' : '6%',
                  opacity: spectrumLive ? 0.8 : 0.18,
                }"
              ></div>
            </div>
          </div>
        </section>

        <!-- cable -->
        <section
          v-if="!cableOk"
          class="mt-5 rounded-[14px] border border-[var(--danger)] bg-[var(--surface)] px-[18px] py-4"
        >
          <div class="text-[14px] font-semibold text-[var(--danger)]">{{ t("sound.cableTitle") }}</div>
          <p class="mt-1 text-[12px] leading-relaxed text-[var(--muted)]">
            {{ t("sound.cableText") }}
          </p>
          <div class="mt-3 flex flex-wrap items-center gap-2.5">
            <button :class="btnPrimary" :disabled="installingCable" @click="installCable">
              <AppIcon v-if="installingCable" name="spinner" :size="14" class="spin" />
              {{ installingCable ? t("wizard.installing") : t("sound.cableInstall") }}
            </button>
            <span class="text-[11.5px] text-[var(--muted)]">{{ cableMessage }}</span>
          </div>
        </section>

        <!-- devices -->
        <section class="mt-[18px] border-t border-[var(--stroke)] pt-3">
          <div class="mb-2.5 flex items-center gap-2.5">
            <h2 class="text-[15px] font-semibold tracking-[-0.01em]">{{ t("sound.devices") }}</h2>
            <span class="ml-auto font-mono text-[11.5px] tabular-nums text-[var(--muted)]">{{ format }}</span>
          </div>
          <div class="grid grid-cols-2 gap-4">
            <div class="min-w-0">
              <span class="mb-[5px] block text-[11.5px] text-[var(--muted)]">{{ t("sound.input") }}</span>
              <AppSelect
                :model-value="selectedInput"
                :options="inputOptions"
                :disabled="reconnecting || starting"
                :placeholder="t('sound.noDevices')"
                @update:model-value="pickInput"
              />
            </div>
            <div class="min-w-0">
              <span class="mb-[5px] block text-[11.5px] text-[var(--muted)]">{{ t("sound.headphones") }}</span>
              <AppSelect
                :model-value="selectedOutput"
                :options="outputOptions"
                :disabled="reconnecting || starting"
                :placeholder="t('sound.noDevices')"
                @update:model-value="pickOutput"
              />
            </div>
          </div>
          <div v-if="inputChannels > 1" class="mt-2 flex flex-wrap items-center gap-2 text-[11.5px] text-[var(--muted)]">
            <span class="whitespace-nowrap">{{ t("sound.channel") }}</span>
            <div class="w-[160px]">
              <AppSelect
                :model-value="String(settings.inputChannel)"
                :options="channelOptions"
                @update:model-value="(value: string) => (settings!.inputChannel = Number(value))"
              />
            </div>
            <span class="min-w-0">{{ t("sound.channelHint") }}</span>
          </div>
          <p v-if="inputIsCable" class="mt-2 text-[12px] leading-relaxed text-[var(--danger)]">
            {{ t("sound.cableAsMic") }}
          </p>
          <div class="mt-2 flex flex-wrap items-center gap-1">
            <button :class="btnGhost" :disabled="refreshing" @click="refreshDevicesManually">
              <AppIcon name="refresh" :size="14" :class="{ spin: refreshing }" />
              {{ refreshing ? t("sound.refreshing") : t("common.refresh") }}
            </button>
            <button :class="btnGhost" :disabled="probing || calibrating" @click="probeMic">
              <AppIcon name="mic" :size="14" />
              {{
                probing ? (probeLeft > 0 ? t("wizard.listening", { s: probeLeft }) : t("wizard.listen")) : t("wizard.probe")
              }}
            </button>
            <button :class="btnGhost" :disabled="calibrating || probing || !micReady" @click="calibrateRoom">
              <AppIcon :name="calibrating ? 'spinner' : 'sliders'" :size="14" :class="{ spin: calibrating }" />
              {{ calibrating ? t("calibrate.running") : t("calibrate.start") }}
            </button>
            <span v-if="reconnecting" class="ml-2 flex items-center gap-1.5 text-[11.5px] text-[var(--muted)]">
              <AppIcon name="spinner" :size="12" class="spin" />
              {{ t("sound.reconnecting") }}
            </span>
            <span v-else-if="probeResult" class="ml-2 font-mono text-[11px] tabular-nums text-[var(--muted)]">
              {{
                t("sound.probeResult", {
                  db: probeResult.peakDb.toFixed(0),
                  speech: (probeResult.speechPercent * 100).toFixed(0),
                  snr: probeResult.snrDb.toFixed(0),
                })
              }}
            </span>
          </div>
          <p v-if="probeResult" class="mt-1 text-[11.5px] leading-relaxed text-[var(--muted)]">{{ probeHint }}</p>
          <div class="mt-3 flex items-center gap-3">
            <span class="shrink-0 text-[11.5px] whitespace-nowrap text-[var(--muted)]">{{ t("sound.outputVolume") }}</span>
            <div class="w-[220px] shrink-0">
              <AppSlider
                :label="t('sound.outputVolume')"
                v-model="settings.outputGainDb"
                :min="-18"
                :max="18"
                :step="0.5"
                :format="(v) => `${v > 0 ? '+' : ''}${v.toFixed(1)} ${u.db}`"
              />
            </div>
            <span class="min-w-0 truncate text-[11px] text-[var(--muted)]">
              {{ t("sound.outputVolumeHint") }}
            </span>
          </div>
        </section>

        <!-- profile -->
        <section class="mt-[18px] border-t border-[var(--stroke)] pt-3">
          <div class="mb-2.5 flex items-center gap-2.5">
            <h2 class="text-[15px] font-semibold tracking-[-0.01em]">{{ t("sound.profile") }}</h2>
            <span v-if="profileMeta" class="ml-auto font-mono text-[11.5px] tabular-nums text-[var(--muted)]">
              {{ profileMeta }}
            </span>
          </div>
          <div class="flex items-center gap-2">
            <div class="min-w-0 flex-1">
              <AppSelect v-model="selectedProfile" :options="profileOptions" @update:model-value="pickProfile" />
            </div>
            <button
              class="iconbtn"
              :disabled="!selectedProfile"
              :title="t('sound.renameProfile')"
              @click="startRename"
            >
              <AppIcon name="pencil" :size="14" />
            </button>
            <button
              :class="btnPrimary"
              :disabled="recording || !micReady"
              :title="micReady ? t('wizard.recordTitle') : t('sound.profileRecordNeedMic')"
              @click="recordProfile"
            >
              <span v-if="recording" class="h-2 w-2 shrink-0 rounded-full bg-[var(--danger)]"></span>
              <AppIcon v-else name="mic" :size="14" />
              {{ recording ? t("wizard.recording", { s: recordLeft }) : t("wizard.record") }}
            </button>
            <button
              class="iconbtn"
              :disabled="!selectedProfile"
              :title="playingKey === 'profile' ? t('sound.stop') : t('sound.playProfile')"
              @click="playProfile"
            >
              <AppIcon :name="playingKey === 'profile' ? 'stop' : 'play'" :size="14" />
            </button>
            <button
              class="iconbtn danger"
              :class="confirmDelete ? 'ring-2 ring-[var(--danger)]' : ''"
              :disabled="!selectedProfile"
              :title="confirmDelete ? t('sound.deleteProfileConfirm') : t('sound.deleteProfile')"
              @click="askDeleteProfile"
            >
              <AppIcon name="trash" :size="14" />
            </button>
          </div>
          <div v-if="renaming" class="mt-2 flex items-center gap-2">
            <input
              v-model="nameDraft"
              maxlength="48"
              class="h-[32px] min-w-0 flex-1 rounded-[8px] border border-[var(--stroke)] bg-[var(--surface)] px-2.5 text-[12.5px] outline-none focus:border-[var(--accent)]"
              :placeholder="t('sound.renamePlaceholder')"
              @keydown.enter="saveRename"
              @keydown.esc="renaming = false"
            />
            <button class="iconbtn" :title="t('common.save')" @click="saveRename">
              <AppIcon name="check" :size="14" />
            </button>
            <button class="iconbtn" :title="t('common.cancel')" @click="renaming = false">
              <AppIcon name="close" :size="14" />
            </button>
          </div>
          <p v-if="profileWeak" class="mt-2 text-[12px] leading-relaxed text-[var(--danger)]">
            {{ t("sound.profileWeak") }}
          </p>
          <p v-if="profileMismatch" class="mt-2 text-[12px] leading-relaxed text-[var(--danger)]">
            {{ t("sound.profileOtherMic", { device: profileInfo?.device ?? "" }) }}
          </p>
          <p v-else-if="profileInfo?.device" class="mt-2 text-[11.5px] leading-relaxed text-[var(--muted)]">
            {{ t("sound.profileRecordedOn", { device: profileInfo.device }) }}
          </p>
          <div class="mt-2.5 flex items-center gap-2.5 text-[12.5px] text-[var(--muted)]">
            <AppSwitch v-model="settings.autoProfile" @update:model-value="refreshProfiles" />
            <span>{{ t("sound.autoProfile") }}</span>
          </div>
        </section>

        <!-- A/B -->
        <section class="mt-[18px] border-t border-[var(--stroke)] pt-3">
          <div class="mb-2.5 flex items-center gap-2.5">
            <h2 class="text-[15px] font-semibold tracking-[-0.01em]">{{ t("sound.ab") }}</h2>
            <label class="ml-auto flex items-center gap-1.5 text-[11.5px] whitespace-nowrap text-[var(--muted)]" :title="t('sound.blindHint')">
              <AppSwitch v-model="abBlind" />
              {{ t("sound.blind") }}
            </label>
            <div class="seg">
              <button
                v-for="sec in [10, 30, 60]"
                :key="sec"
                class="segbtn"
                :data-on="settings.abSeconds === sec"
                @click="settings.abSeconds = sec"
              >
                {{ sec }} {{ u.s }}
              </button>
            </div>
          </div>
          <div class="flex flex-wrap items-center gap-2">
            <button :class="btnPrimary" :disabled="!running && !abRunning" @click="toggleAb">
              <span class="h-2 w-2 shrink-0 rounded-full bg-[var(--danger)]"></span>
              {{
                abRunning
                  ? t("wizard.recording", { s: Math.round(abProgress * (settings?.abSeconds ?? 30)) })
                  : t("sound.abRecord", { s: settings?.abSeconds ?? 30 })
              }}
            </button>
            <button
              :class="testMode ? btn : btnPrimary"
              :disabled="busy || starting || (!micReady && !testMode)"
              :title="micReady || testMode ? '' : t('sound.noMicFound')"
              @click="toggleTest"
            >
              <AppIcon
                :name="busy || starting ? 'spinner' : 'headphones'"
                :size="14"
                :class="{ spin: busy || starting }"
              />
              {{
                busy || starting
                  ? t("sound.switching")
                  : testMode
                    ? t("sound.stopTest")
                    : t("sound.test")
              }}
            </button>
            <span class="text-[11.5px] text-[var(--muted)]">{{ t("sound.abHint") }}</span>
          </div>
          <div v-if="abRunning" class="mt-2 h-0.5 overflow-hidden rounded-full bg-[var(--stroke)]">
            <div class="h-full rounded-full bg-[var(--danger)]" :style="{ width: abProgress * 100 + '%' }"></div>
          </div>
          <p v-if="!running" class="mt-2 text-[11.5px] leading-relaxed text-[var(--muted)]">
            {{ t("sound.abNeedCable") }}
          </p>

          <div
            v-if="abRaw && abClean"
            class="mt-3 rounded-[11px] border border-[var(--stroke)] bg-[var(--surface)] px-3 py-2.5"
            :title="abFolder || undefined"
          >
            <div class="flex flex-wrap items-center justify-between gap-2">
              <div class="flex gap-1.5">
                <button
                  class="playbtn"
                  :data-on="playingKey === 'pair' && abSide === abVisible.a"
                  @click="playAb(abVisible.a)"
                >
                  <AppIcon :name="playingKey === 'pair' && abSide === abVisible.a ? 'stop' : 'play'" :size="12" />
                  {{ abBlind ? "A" : t("sound.raw") }}
                </button>
                <button
                  class="playbtn"
                  :data-on="playingKey === 'pair' && abSide === abVisible.b"
                  @click="playAb(abVisible.b)"
                >
                  <AppIcon :name="playingKey === 'pair' && abSide === abVisible.b ? 'stop' : 'play'" :size="12" />
                  {{ abBlind ? "B" : t("sound.clean") }}
                </button>
              </div>

              <div class="flex items-center gap-3">
                <label class="flex items-center gap-2 text-[11.5px] whitespace-nowrap text-[var(--muted)]">
                  <AppSwitch v-model="abNormalize" />
                  {{ t("sound.normalize") }}
                </label>
                <button class="iconbtn" :title="t('sound.matchLevel')" @click="matchLevel">
                  <AppIcon name="sliders" :size="14" />
                </button>
                <button class="iconbtn" :title="t('sound.checkVoice')" @click="checkAbSpeaker">
                  <AppIcon name="mic" :size="14" />
                </button>
              </div>
            </div>

            <div class="mt-2 grid grid-cols-5 gap-3 border-t border-[var(--stroke)] pt-2 font-mono tabular-nums">
              <span class="min-w-0">
                <b class="block text-[12.5px] font-normal">
                  {{ abMetrics ? abMetrics.speechLevelDb.toFixed(0) + " " + u.db : "—" }}
                </b>
                <span class="mt-0.5 block truncate text-[10.5px] text-[var(--muted)]">{{ t("sound.metricSpeech") }}</span>
              </span>
              <span class="min-w-0">
                <b class="block text-[12.5px] font-normal">
                  {{ abMetrics ? Math.max(0, abMetrics.pauseContrastDb).toFixed(0) + " " + u.db : "—" }}
                </b>
                <span class="mt-0.5 block truncate text-[10.5px] text-[var(--muted)]">{{ t("sound.metricPauses") }}</span>
              </span>
              <span class="min-w-0">
                <b class="block text-[12.5px] font-normal">
                  {{ abDelta === null ? "—" : Math.abs(abDelta).toFixed(1) + " " + u.db }}
                </b>
                <span class="mt-0.5 block truncate text-[10.5px] text-[var(--muted)]">{{ t("sound.metricDelta") }}</span>
              </span>
              <span class="min-w-0">
                <b
                  class="block text-[12.5px] font-normal"
                  :class="abMetrics && abMetrics.longestDropoutS > 0.4 ? 'text-[var(--danger)]' : ''"
                >
                  {{ abMetrics ? abMetrics.longestDropoutS.toFixed(2) + " " + u.s : "—" }}
                </b>
                <span class="mt-0.5 block truncate text-[10.5px] text-[var(--muted)]">{{ t("sound.metricDropout") }}</span>
              </span>
              <span
                class="min-w-0"
                :title="abSpeaker
                  ? t('sound.voiceSimilarity', {
                      median: (abSpeaker.median * 100).toFixed(0),
                      min: (abSpeaker.min * 100).toFixed(0),
                      max: (abSpeaker.max * 100).toFixed(0),
                    })
                  : t('sound.checkVoice')"
              >
                <b class="block text-[12.5px] font-normal">
                  {{ abSpeaker ? (abSpeaker.median * 100).toFixed(0) + " %" : "—" }}
                </b>
                <span class="mt-0.5 block truncate text-[10.5px] text-[var(--muted)]">{{ t("sound.metricVoice") }}</span>
              </span>
            </div>
          </div>

          <div v-if="history.length" class="mt-3 flex flex-wrap items-center gap-1.5">
            <span class="mr-0.5 text-[11.5px] text-[var(--muted)]">{{ t("sound.history") }}</span>
            <div
              v-for="entry in history"
              :key="entry.stamp"
              class="stamp group"
              :data-on="abRaw === entry.raw"
              :title="entry.raw"
            >
              <button class="h-full cursor-pointer px-2" @click="selectRecording(entry)">
                {{ formatStamp(entry.stamp) }}
              </button>
              <button
                class="mr-0.5 grid h-4 w-4 cursor-pointer place-items-center rounded text-[var(--muted)] opacity-0 transition hover:text-[var(--danger)] group-hover:opacity-100"
                :title="t('sound.deleteTake')"
                @click="removeRecording(entry)"
              >
                <AppIcon name="close" :size="10" />
              </button>
            </div>
          </div>
          <div v-if="selectedTake" class="mt-2 flex flex-wrap items-center gap-1.5">
            <button :class="btnGhost" @click="askApplyTake(selectedTake)">
              <AppIcon name="sliders" :size="14" />
              {{ t("sound.applyTakeSettings") }}
            </button>
            <span class="text-[11.5px] text-[var(--muted)]">{{ t("sound.applyTakeSettingsHint") }}</span>
          </div>
        </section>
  </div>
</template>