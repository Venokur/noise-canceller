#!/usr/bin/env python
"""Metrics for an A/B pair, used by scripts/regression.ps1.

Prints JSON and exits non-zero when the processed take looks broken: long
dropouts, no speech-to-pause contrast, or a level far from the raw take.

Every level is a median over 20 ms frames and the contrast is measured inside
one file, so changing the output volume cannot make a take look cleaner.

Requires numpy.
"""

import json
import sys
import wave

import numpy as np


def load(path):
    with wave.open(path, "rb") as handle:
        rate = handle.getframerate()
        data = np.frombuffer(handle.readframes(handle.getnframes()), dtype=np.int16)
    return data.astype(np.float32) / 32768.0, rate


def env(x, rate, ms=20):
    window = max(1, int(rate * ms / 1000))
    frames = len(x) // window
    if not frames:
        return np.array([-120.0])
    rms = np.sqrt((x[: frames * window].reshape(frames, window) ** 2).mean(axis=1) + 1e-12)
    return 20 * np.log10(rms + 1e-12)


def align(raw, clean, rate):
    """Compensate the capture/processor offset (100-200 ms).

    The raw take is written from the capture callback and the clean one from
    the processing thread, so they start at different moments. Without this,
    a speech frame is compared with a pause frame and the metrics lie: the
    last take reported -33 dB "level moved" purely because of the offset.
    """
    er = env(raw, rate)
    ec = env(clean, rate)
    frames = min(len(er), len(ec))
    if frames < 20:
        return raw, clean, 0
    best = (0, -1.0)
    # +/-600 ms in 20 ms frames
    for lag in range(-30, 31):
        if lag >= 0:
            a = er[: frames - lag]
            b = ec[lag:frames]
        else:
            a = er[-lag:frames]
            b = ec[: frames + lag]
        if len(a) < 20:
            continue
        corr = float(np.corrcoef(a, b)[0, 1])
        if corr > best[1]:
            best = (lag, corr)
    lag, corr = best
    if corr < 0.3:
        return raw, clean, 0
    span = max(1, int(rate * 0.02))
    if lag > 0:
        # clean is later by `lag` frames: drop its head
        clean = clean[lag * span :]
        raw = raw[: len(clean)]
    elif lag < 0:
        raw = raw[(-lag) * span :]
        clean = clean[: len(raw)]
    return raw, clean, lag * 20


def main(raw_path, clean_path):
    raw, rate = load(raw_path)
    clean, _ = load(clean_path)
    raw, clean, align_ms = align(raw, clean, rate)
    n = min(len(raw), len(clean))
    raw, clean = raw[:n], clean[:n]

    er = env(raw, rate)
    ec = env(clean, rate)
    frames = min(len(er), len(ec))
    er, ec = er[:frames], ec[:frames]

    # speech is relative to this take's own noise floor: in a room with a
    # television the background sits at -45 dB, and an absolute -50 dB threshold
    # would call it speech and turn correct suppression into a "dropout"
    floor = float(np.percentile(er, 10))
    speech = er > max(floor + 18.0, -55.0)
    pause = er < floor + 5.0
    # a 10 s take of continuous speech has almost no real pauses; measuring
    # "contrast" on word gaps then fails a perfectly good take
    has_pauses = float(pause.mean()) >= 0.03

    clean_speech = float(np.median(ec[speech])) if speech.any() else -120.0
    raw_speech = float(np.median(er[speech])) if speech.any() else -120.0
    clean_pause = float(np.median(ec[pause])) if has_pauses else None

    muted = speech & (ec < clean_speech - 30.0)
    runs = []
    i = 0
    while i < len(muted):
        if muted[i]:
            j = i
            while j < len(muted) and muted[j]:
                j += 1
            runs.append((j - i) * 0.02)
            i = j
        else:
            i += 1

    # how much the *audible* voice level moves between phrases: the complaint
    # was "the voice jumps in volume" even when the take is otherwise clean
    audible = speech & (ec > -70.0)
    if audible.sum() >= 5:
        level_spread = float(np.percentile(ec[audible], 90) - np.percentile(ec[audible], 10))
    else:
        level_spread = None

    metrics = {
        "speech_level_db": round(clean_speech, 1),
        "level_delta_db": round(clean_speech - raw_speech, 1),
        "pause_contrast_db": round(clean_speech - clean_pause, 1) if has_pauses else None,
        "muted_total_s": round(float(muted.sum()) * 0.02, 2),
        "longest_dropout_s": round(max(runs) if runs else 0.0, 2),
        "level_spread_db": round(level_spread, 1) if level_spread is not None else None,
        "align_ms": round(align_ms, 0),
    }
    print(json.dumps(metrics))

    problems = []
    if metrics["longest_dropout_s"] > 0.4:
        problems.append(f"dropout {metrics['longest_dropout_s']}s > 0.4s")
    if metrics["pause_contrast_db"] is not None and metrics["pause_contrast_db"] < 12.0:
        problems.append(f"pauses only {metrics['pause_contrast_db']} dB below speech < 12 dB")
    if abs(metrics["level_delta_db"]) > 20.0:
        problems.append(f"level moved {metrics['level_delta_db']} dB from the raw take")
    if not (-55.0 < metrics["speech_level_db"] < -10.0):
        problems.append(f"speech level {metrics['speech_level_db']} dBFS out of range")
    if problems:
        print("FAIL: " + "; ".join(problems), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 3:
        print("usage: ab_metrics.py <raw.wav> <clean.wav>", file=sys.stderr)
        sys.exit(2)
    sys.exit(main(sys.argv[1], sys.argv[2]))
