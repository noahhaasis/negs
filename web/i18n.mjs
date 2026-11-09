// The modules' side of the interface language (web/i18n.js runs first, in
// index.html, and holds how the language was picked and its dictionary):
// tr() is what every user-facing string passes through. Its first argument
// is the English source — the dictionary (web/locales/zh-CN.js) maps it to
// the language, and without one (English, or a check running in Node) it
// returns it as it is, so the English in the sources is all there ever was.
// `params` fills {name} holes: tr("Couldn't open {name}", { name }).
const state = globalThis.NEGS_I18N ?? { lang: "en", dict: null };
export const lang = state.lang;
export function tr(text, params) {
  let s = state.dict?.[text] ?? text;
  if (params) for (const [name, value] of Object.entries(params)) s = s.split(`{${name}}`).join(String(value));
  return s;
}
