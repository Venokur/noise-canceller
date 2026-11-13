// Small formatting helpers shared by the shell and the composables.

/// `C:\...\voice-1794518466.wav` → `voice-1794518466.wav`
export function shortName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] ?? path;
}
