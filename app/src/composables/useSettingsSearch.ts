// Settings search. While a query is typed every section is shown, so a match is
// never hidden behind the selected tab.
//
// The rows are hand-written template markup, so the filter walks the rendered
// DOM: every row carries `srow`, every section `sblock`. The `nextTick` is
// needed because `v-if="searching || …"` has to patch the sections in before
// the walk sees the fresh rows.
import { computed, nextTick, ref } from "vue";

export function useSettingsSearch() {
  const settingsQuery = ref("");
  const searching = computed(() => settingsQuery.value.trim().length > 0);

  function applyFilter() {
    const query = settingsQuery.value.trim().toLowerCase();
    document.querySelectorAll<HTMLElement>(".srow").forEach((row) => {
      row.style.display =
        !query || (row.textContent ?? "").toLowerCase().includes(query) ? "" : "none";
    });
    // a section whose rows all fell out of the query should disappear instead of
    // leaving an empty heading behind; the diagnostics table has no rows at all,
    // so it is matched by its own text
    document.querySelectorAll<HTMLElement>(".sblock").forEach((block) => {
      if (!query) {
        block.style.display = "";
        return;
      }
      const rows = Array.from(block.querySelectorAll<HTMLElement>(".srow"));
      const hit = rows.length
        ? rows.some((row) => row.style.display !== "none")
        : (block.textContent ?? "").toLowerCase().includes(query);
      block.style.display = hit ? "" : "none";
    });
  }

  async function filterSettings(value: string) {
    settingsQuery.value = value;
    await nextTick();
    applyFilter();
  }

  return { settingsQuery, searching, filterSettings };
}
