// Film and paper previews (docs/stock-previews.md): Film and Paper as
// foldouts, collapsed by default, of tiles grouped as the menus are, each
// the photograph through that stock. app.js asks for the tiles' pictures one
// at a time while the photograph has nothing else to render (`pumpPreviews`).

const el = (tag, props = {}, ...children) => {
  const e = Object.assign(document.createElement(tag), props);
  e.append(...children);
  return e;
};

// `groups`: [{ title, stocks: [{ id, name }] }]. `choose(id)` is the menu's
// choice. `shown()` says which tiles are on screen changed (the viewport's
// observer, which the panel's scroll clips).
export function foldout(title, groups, choose, shown, open = false) {
  const current = el("span", { className: "stock-current" });
  const summary = el("summary", {}, el("span", { className: "stock-title", textContent: title }), current);
  // In the sidebar a fixed window onto the grid keeps the tabs below in
  // reach (macOS StockFoldout.gridHeight); on the sheet the panel scrolls.
  const body = el("div", { className: "stock-scroll" });
  const details = el("details", { className: "stock-fold", open }, summary, body);
  const tiles = new Map();
  const visible = new Set();
  const observer = new IntersectionObserver((entries) => {
    for (const e of entries) e.isIntersecting ? visible.add(e.target.dataset.id) : visible.delete(e.target.dataset.id);
    shown();
  }, { rootMargin: "120px 0px" });
  for (const g of groups) {
    const grid = el("div", { className: "stock-grid" });
    for (const s of g.stocks) {
      const canvas = el("canvas", { className: "stock-thumb", width: 3, height: 2 });
      const button = el("button", { type: "button", className: "stock-tile blank", title: s.name }, canvas, el("span", { textContent: s.name }));
      button.dataset.id = s.id;
      button.addEventListener("click", () => choose(s.id));
      grid.append(button);
      // key: what the picture shows; photo: whose; failed: the key it failed at.
      tiles.set(s.id, { id: s.id, name: s.name, button, canvas, key: null, photo: null, failed: null });
      observer.observe(button);
    }
    body.append(el("h3", { textContent: g.title }), grid);
  }
  // The window opens on the current stock, and follows it when it changes
  // from elsewhere (a film's paper).
  let selected = null;
  const center = () => {
    const tile = tiles.get(selected);
    if (!details.open || !tile || body.scrollHeight <= body.clientHeight) return;
    body.scrollTop = tile.button.offsetTop - (body.clientHeight - tile.button.offsetHeight) / 2;
  };
  details.addEventListener("toggle", () => { center(); shown(); });
  return {
    root: details,
    tiles,
    // The tiles the scroll shows (or nearly), in grid order; none while
    // folded or hidden.
    onScreen: () => (details.open && details.isConnected ? [...tiles.values()].filter((t) => visible.has(t.id)) : []),
    select(id) {
      const name = [...tiles.values()].find((t) => t.id === id)?.name ?? "";
      current.textContent = name;
      for (const t of tiles.values()) t.button.classList.toggle("selected", t.id === id);
      if (id !== selected) { selected = id; center(); }
    },
    center,
  };
}

// A tile for `photo` at `key`: the photograph's other edits keep the old
// picture dimmed until the new one comes; another photograph blanks it.
export function markStale(tile, photo, key) {
  if (tile.photo !== photo) {
    tile.photo = photo;
    tile.key = null;
    tile.canvas.width = 3;
    tile.canvas.height = 2;
  }
  tile.button.classList.toggle("blank", tile.key === null);
  tile.button.classList.toggle("stale", tile.key !== null && tile.key !== key);
}

export function paint(tile, photo, key, { width, height, rgba }) {
  tile.photo = photo;
  tile.key = key;
  tile.canvas.width = width;
  tile.canvas.height = height;
  tile.canvas.getContext("2d").putImageData(new ImageData(new Uint8ClampedArray(rgba.buffer, rgba.byteOffset, rgba.length), width, height), 0, 0);
  tile.button.classList.remove("blank", "stale");
}

// The frame a tile develops: large enough that the crop fills `longSide`,
// within 1280 px (native `thumbnail_dimensions`).
export function frameSide(width, height, crop, longSide) {
  const cropped = Math.max(width * crop.width, height * crop.height, 1);
  return Math.round(Math.min(1280, Math.max(width, height) * longSide / cropped));
}
