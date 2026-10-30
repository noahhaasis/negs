// A one-click start, then numbered samples and Next. Only metadata is kept
// here: the app owns one photograph's bytes, and aborts superseded downloads.
export const DEFAULT_SAMPLE = { id: "DSC_6377", url: "sample/DSC_6377.dng" };

export function createSamplePicker({ container, select, next, note, current, load, onError,
  online = () => navigator.onLine !== false,
  fetchCatalog = async () => {
    const res = await fetch("sample/catalog.json");
    if (!res.ok) throw new Error(`Sample choices: ${res.status}`);
    return res.json();
  },
}) {
  let entries = [DEFAULT_SAMPLE], catalog = null, fetching = false;
  let pending = null, generation = 0;

  function fill() {
    select.replaceChildren(...entries.map((entry, i) => Object.assign(select.ownerDocument.createElement("option"), {
      value: entry.id, textContent: `Sample ${i + 1}`,
    })));
    sync();
  }

  function sync() {
    const id = pending?.id ?? current();
    container.hidden = !id;
    container.setAttribute("aria-busy", String(!!pending));
    for (const option of select.options) option.disabled = !online() && option.value !== DEFAULT_SAMPLE.id;
    select.value = id ?? DEFAULT_SAMPLE.id;
    select.setAttribute("aria-describedby", online() ? "" : note.id);
    next.disabled = !online() || fetching;
    next.title = online() ? "Open the next sample photo" : "Connect to switch to another sample";
    note.hidden = online();
  }

  function choices() {
    if (catalog) return catalog;
    fetching = true;
    sync();
    catalog = Promise.resolve().then(fetchCatalog).then((data) => {
      if (data.default !== DEFAULT_SAMPLE.id || !Array.isArray(data.samples) ||
          data.samples[0]?.id !== DEFAULT_SAMPLE.id || data.samples.some((s) => !s.id || !s.url)) {
        throw new Error("Invalid sample catalog");
      }
      entries = data.samples;
      fill();
      return entries;
    }).catch((error) => {
      catalog = null; // Next can retry; failure must not block the default RAW.
      throw error;
    }).finally(() => {
      fetching = false;
      sync();
    });
    return catalog;
  }

  function cancel() {
    generation++;
    pending?.controller.abort();
    pending = null;
    sync();
  }

  async function open(id = DEFAULT_SAMPLE.id) {
    const index = entries.findIndex((s) => s.id === id);
    if (index < 0) return;
    if (!online() && id !== DEFAULT_SAMPLE.id) {
      sync();
      return;
    }
    if (id === (pending?.id ?? current())) return;
    cancel();
    const token = generation, controller = new AbortController();
    pending = { id, controller };
    sync();
    // Do not wait for metadata to start the first photograph's download.
    void choices().catch((error) => {
      if (token === generation) onError(error);
    });
    try {
      await load(entries[index], index + 1, controller.signal);
    } catch (error) {
      if (token === generation && error.name !== "AbortError") onError(error);
    } finally {
      if (token === generation) {
        pending = null;
        sync(); // On failure, return to the photograph that actually opened.
      }
    }
  }

  async function advance() {
    const token = generation;
    try {
      await choices();
      if (token !== generation || !online()) return;
      const index = entries.findIndex((s) => s.id === (pending?.id ?? current()));
      return open(entries[(index + 1) % entries.length].id);
    } catch (error) {
      if (token === generation) onError(error);
    }
  }

  select.addEventListener("change", () => { void open(select.value); });
  next.addEventListener("click", () => { void advance(); });
  fill();
  return { open, cancel, sync };
}
