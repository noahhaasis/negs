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
// observer, which the panel's scroll clips). `starred`: the favourite ids in
// the order starred (macOS Previews.favoriteFilms), which `starredChanged`
// hears of. The favourites come first, untitled, and each stays in its group
// too: a copy of its tile showing the same picture. `search`: the prompt of
// a search field over the grid (macOS StockFoldout, Film only), or none.
export function foldout(title, groups, choose, shown, open = false, starred = [], starredChanged = () => {}, search = null) {
  const current = el("span", { className: "stock-current" });
  const summary = el("summary", {}, el("span", { className: "stock-title", textContent: title }), current);
  // In the sidebar a fixed window onto the grid keeps the tabs below in
  // reach (macOS StockFoldout.gridHeight); on the sheet the panel scrolls.
  const body = el("div", { className: "stock-scroll" });
  const details = el("details", { className: "stock-fold", open }, summary, body);
  const tiles = new Map();
  // The buttons on screen (or nearly); a tile is when either copy is.
  const visible = new Set();
  const observer = new IntersectionObserver((entries) => {
    for (const e of entries) e.isIntersecting ? visible.add(e.target) : visible.delete(e.target);
    shown();
  }, { rootMargin: "120px 0px" });
  let favorites = [];
  // The search field's words; while there are any, no favourites.
  let query = "";
  // A tile's cell: the button that chooses, and the star over its corner,
  // outside it so a click stars rather than chooses.
  const cell = (tile) => {
    const canvas = el("canvas", { className: "stock-thumb", width: 3, height: 2 });
    const button = el("button", { type: "button", className: "stock-tile blank", title: tile.name }, canvas, el("span", { textContent: tile.name }));
    button.dataset.id = tile.id;
    button.addEventListener("click", () => choose(tile.id));
    const star = el("button", { type: "button", className: "stock-star" });
    star.addEventListener("click", () => toggle(tile.id));
    observer.observe(button);
    return { root: el("div", { className: "stock-cell" }, button, star), button, canvas, star };
  };
  const sections = [];
  for (const g of groups) {
    const grid = el("div", { className: "stock-grid" });
    const heading = el("h3", { textContent: g.title });
    const section = { heading, grid, tiles: [] };
    for (const s of g.stocks) {
      // key: what the picture shows; photo: whose; failed: the key it failed at.
      // text: what a search looks in (macOS StockGroup.matching).
      const tile = { id: s.id, name: s.name, key: null, photo: null, failed: null, copy: null,
        text: `${s.name} ${s.id.replaceAll("_", " ")} ${g.title}`.toLocaleLowerCase() };
      Object.assign(tile, cell(tile));
      grid.append(tile.root);
      tiles.set(s.id, tile);
      section.tiles.push(tile);
    }
    sections.push(section);
    body.append(heading, grid);
  }
  const favoriteGrid = el("div", { className: "stock-grid stock-favorites" });
  body.prepend(favoriteGrid);

  const markStars = () => {
    for (const t of tiles.values()) {
      const on = favorites.includes(t.id);
      for (const view of [t, t.copy].filter(Boolean)) {
        view.root.classList.toggle("favorite", on);
        view.star.title = on ? "Remove from favorites" : "Add to favorites";
        view.star.setAttribute("aria-label", `${on ? "Remove" : "Add"} ${t.name} ${on ? "from" : "to"} favorites`);
        view.star.setAttribute("aria-pressed", on);
      }
    }
  };
  // The favourites' copies, in the order starred; ids the catalog no
  // longer has are kept, in case it brings them back.
  const layOut = () => {
    for (const t of tiles.values()) {
      if (t.copy && !favorites.includes(t.id)) {
        observer.unobserve(t.copy.button);
        visible.delete(t.copy.button);
        t.copy.root.remove();
        t.copy = null;
      }
    }
    for (const id of favorites) {
      const t = tiles.get(id);
      if (!t) continue;
      if (!t.copy) {
        t.copy = cell(t);
        t.copy.button.className = t.button.className;
        copyPicture(t);
      }
      favoriteGrid.append(t.copy.root);
    }
    favoriteGrid.hidden = !favoriteGrid.childElementCount || !!query.trim();
    markStars();
  };
  const toggle = (id) => {
    favorites = favorites.includes(id) ? favorites.filter((f) => f !== id) : [...favorites, id];
    layOut();
    starredChanged(favorites.slice());
    shown();
  };

  // The window opens on the current stock, the copy among the favourites
  // if starred, and follows it when it changes from elsewhere (a film's paper).
  let selected = null;
  const center = () => {
    const tile = tiles.get(selected);
    if (!details.open || !tile || body.scrollHeight <= body.clientHeight) return;
    const button = (tile.copy ?? tile).button;
    body.scrollTop = button.offsetTop - (body.clientHeight - button.offsetHeight) / 2;
  };
  details.addEventListener("toggle", () => { center(); shown(); });
  favorites = starred.slice();
  layOut();

  // The search: every word somewhere in a stock's name, id or group. The
  // matches show once, under their groups (no favourites); each search
  // starts at the top of its matches, and clearing it goes back to the
  // current stock. Return takes the first match, Escape clears.
  const empty = el("p", { className: "stock-empty", hidden: true });
  body.append(empty);
  const matches = (query) => {
    const words = query.toLocaleLowerCase().split(/\s+/).filter(Boolean);
    return sections.flatMap((s) => s.tiles).filter((t) => words.every((w) => t.text.includes(w)));
  };
  const filter = (text) => {
    query = text;
    const searching = !!query.trim();
    const hits = new Set(searching ? matches(query) : tiles.values());
    for (const s of sections) {
      for (const t of s.tiles) t.root.hidden = !hits.has(t);
      s.heading.hidden = s.grid.hidden = !s.tiles.some((t) => hits.has(t));
    }
    favoriteGrid.hidden = searching || !favoriteGrid.childElementCount;
    empty.hidden = !searching || hits.size > 0;
    empty.textContent = `No ${title.toLowerCase()}s match “${query.trim()}”`;
    if (searching) body.scrollTop = 0; else center();
    shown();
  };
  if (search) {
    const field = el("input", { type: "search", className: "stock-search", placeholder: search, spellcheck: false, autocomplete: "off" });
    field.setAttribute("aria-label", search);
    field.addEventListener("input", () => filter(field.value));
    field.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        const first = field.value.trim() && matches(field.value)[0];
        if (first) choose(first.id);
      } else if (e.key === "Escape" && field.value) {
        e.preventDefault();
        e.stopPropagation();
        field.value = "";
        filter("");
      }
    });
    body.before(field);
  }
  return {
    root: details,
    tiles,
    // The tiles the scroll shows (or nearly), favourites first, then in
    // grid order; none while folded or hidden.
    onScreen: () => {
      if (!details.open || !details.isConnected) return [];
      const on = (t) => visible.has(t.button) || (t.copy && visible.has(t.copy.button));
      const first = favorites.map((id) => tiles.get(id)).filter((t) => t && on(t));
      return [...first, ...[...tiles.values()].filter((t) => on(t) && !favorites.includes(t.id))];
    },
    favorites: () => favorites.filter((id) => tiles.has(id)),
    select(id) {
      current.textContent = tiles.get(id)?.name ?? "";
      for (const t of tiles.values()) for (const view of [t, t.copy].filter(Boolean)) view.button.classList.toggle("selected", t.id === id);
      if (id !== selected) { selected = id; center(); }
    },
    center,
  };
}

const views = (tile) => [tile, tile.copy].filter(Boolean);
const copyPicture = (tile) => {
  if (!tile.copy) return;
  const { canvas } = tile.copy;
  canvas.width = tile.canvas.width;
  canvas.height = tile.canvas.height;
  canvas.getContext("2d").drawImage(tile.canvas, 0, 0);
};

// A tile for `photo` at `key`: the photograph's other edits keep the old
// picture dimmed until the new one comes; another photograph blanks it.
export function markStale(tile, photo, key) {
  if (tile.photo !== photo) {
    tile.photo = photo;
    tile.key = null;
    for (const view of views(tile)) {
      view.canvas.width = 3;
      view.canvas.height = 2;
    }
  }
  for (const view of views(tile)) {
    view.button.classList.toggle("blank", tile.key === null);
    view.button.classList.toggle("stale", tile.key !== null && tile.key !== key);
  }
}

export function paint(tile, photo, key, { width, height, rgba }) {
  tile.photo = photo;
  tile.key = key;
  tile.canvas.width = width;
  tile.canvas.height = height;
  tile.canvas.getContext("2d").putImageData(new ImageData(new Uint8ClampedArray(rgba.buffer, rgba.byteOffset, rgba.length), width, height), 0, 0);
  copyPicture(tile);
  for (const view of views(tile)) view.button.classList.remove("blank", "stale");
}

// The frame a tile develops: large enough that the crop fills `longSide`,
// within 1280 px (native `thumbnail_dimensions`).
export function frameSide(width, height, crop, longSide) {
  const cropped = Math.max(width * crop.width, height * crop.height, 1);
  return Math.round(Math.min(1280, Math.max(width, height) * longSide / cropped));
}
