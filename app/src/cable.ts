// What counts as the virtual cable, by device name.
//
// The Rust side stays the authority: `devices::is_cable_name` refuses to open
// the cable's capture side as a microphone and `Engine::start` returns a clear
// error if it is asked to. This module only mirrors the rule so the window
// offers the same devices the engine will accept.
//
// It exists because three slightly different regexes used to live in App.vue:
// auto-selection matched "CABLE Input|VB-Audio", the check that a rendered
// device is the cable matched "CABLE Input|VB-Audio|VB-CABLE", and the input
// check matched "CABLE Output|...". A device named only "VB-CABLE" was
// therefore treated as the cable in one place and as a real device in another.

/** Playback side of the cable: what the app renders the cleaned voice into. */
export function isCableOutput(name: string): boolean {
  return /CABLE Input|VB-Audio|VB-CABLE/i.test(name);
}

/** Capture side of the cable: using it as a microphone is a feedback loop. */
export function isCableInput(name: string): boolean {
  return /CABLE Output|VB-Audio|VB-CABLE/i.test(name);
}
