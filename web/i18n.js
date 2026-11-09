// The interface language (docs in web/i18n.mjs): this script runs before the
// page parses, picks the viewer's language (a ?lang= link or the Settings
// choice, kept in localStorage, else the browser's), and loads its dictionary —
// locales/<lang>.js, a plain script so it blocks parsing and the page never
// flashes English — before index.html renders. Once the body has parsed, the
// call at its end translates the static page; strings the modules build go
// through tr() (web/i18n.mjs) against the same dictionary, whose keys are
// the English sources: the English in the sources stays the app's own, and
// web/checks/i18n.test.mjs checks every key against them.
window.NEGS_I18N = (function () {
  // Languages with a dictionary in web/locales/, and their fallbacks.
  const LANGS = { "zh-CN": "en" };
  // The Settings switch keeps its choice as remember() writes it (JSON); a
  // hand-set raw string works too.
  let saved = null;
  try {
    const raw = localStorage.getItem("negs.lang");
    if (raw != null) {
      try { saved = JSON.parse(raw); } catch { saved = raw; }
      if (typeof saved !== "string") saved = null;
    }
  } catch {}
  const known = (l) => l === "en" || LANGS[l] !== undefined;
  // A ?lang= link (the Chinese site's links to /try/) chooses as Settings
  // does, from then on, and leaves the address: a reload or a later Settings
  // switch must not choose it again. The other parameters and the hash stay.
  const url = new URL(location.href);
  const query = url.searchParams.get("lang");
  if (query != null) {
    if (known(query)) {
      saved = query;
      try { localStorage.setItem("negs.lang", JSON.stringify(query)); } catch {}
    }
    url.searchParams.delete("lang");
    try { history.replaceState(history.state, "", url.pathname + url.search + url.hash); } catch {}
  }
  const nav = (navigator.languages?.[0] ?? navigator.language ?? "en").toLowerCase();
  const wanted = saved ?? (nav.startsWith("zh") ? "zh-CN" : "en");
  const lang = known(wanted) ? wanted : "en";
  const state = { lang, dict: null };
  // The website in each language, for the wordmark's link back to it.
  const HOME = { "zh-CN": "/zh/" };

  // The dictionary's value for `text`, or the text itself (English): what
  // every string passes through, here and in the modules' tr(). `params`
  // fills {name} holes in either language.
  state.tr = (text, params) => {
    let s = state.dict?.[text] ?? text;
    if (params) for (const [name, value] of Object.entries(params)) s = s.split(`{${name}}`).join(String(value));
    return s;
  };

  if (lang !== "en") {
    // onerror keeps the page English where the dictionary can't load.
    document.write(`<script src="locales/${lang}.js" onerror="window.NEGS_I18N.lang='en'"><\/script>`);
  }

  // index.html's own text, titles and labels, called once the body has
  // parsed (the script at its end): whole text nodes and whole attribute
  // values only — the dictionary holds the sources' exact strings.
  state.translate = () => {
    if (lang === "en" || !state.dict) return;
    document.documentElement.lang = lang;
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const source = node.nodeValue.trim();
      if (source && state.dict[source]) node.nodeValue = state.dict[source];
    }
    for (const el of document.body.querySelectorAll("[title], [aria-label], [placeholder]")) {
      for (const name of ["title", "aria-label", "placeholder"]) {
        const source = el.getAttribute(name);
        if (source && state.dict[source]) el.setAttribute(name, state.dict[source]);
      }
    }
    if (state.dict[document.title]) document.title = state.dict[document.title];
    // The wordmark goes back to the website in the same language (app.js's
    // leave confirmation follows its href too).
    const brand = document.getElementById("brand");
    if (brand && HOME[lang]) brand.setAttribute("href", HOME[lang]);
    // The CSS content label (index.html .view.dragging::after) reads this.
    const drop = state.dict["Drop to open"];
    if (drop) document.documentElement.style.setProperty("--drop", JSON.stringify(drop));
    // The installed app's name and description (locales/<lang>.webmanifest).
    const manifest = document.querySelector('link[rel="manifest"]');
    if (manifest) manifest.href = `locales/${lang}.webmanifest`;
  };
  return state;
})();
