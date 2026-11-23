// Our own words, kept out of upstream's locale files (high-churn, merge
// conflicts). Keys by name, as upstream does; the language is Shard's own
// (useLang), falling back to English, then to the key. Counts read as
// Shard's do: grouped in the UI language ("1,010 selected"), a `…One` key
// for one where English needs it, Russian forms as `{n|файл|файла|файлов}`.
import { useMemo } from "react";
import { formatCount, resolvePlurals, useLang, type Lang } from "../shared/i18n";
import en from "./i18n/en.json";
import ru from "./i18n/ru.json";
import zh from "./i18n/zh.json";

type Dict = Record<string, string>;
// Record<Lang, …>: tsc fails when upstream adds a language we have no words for.
const DICTS: Record<Lang, Dict> = { en, ru, zh };

type Vars = Record<string, string | number>;

/** The placeholders that count things (Shard's COUNT_VARS). */
const COUNT_VARS = new Set(["n", "total", "count", "done", "failed", "ok"]);

function translate(lang: Lang, key: string, vars?: Vars): string {
  const k = vars?.n === 1 && (DICTS[lang][`${key}One`] ?? DICTS.en[`${key}One`]) !== undefined ? `${key}One` : key;
  const from: Lang = DICTS[lang][k] === undefined && DICTS.en[k] !== undefined ? "en" : lang;
  let out = resolvePlurals(from, DICTS[from][k] ?? k, vars);
  for (const [name, v] of Object.entries(vars ?? {})) out = out.split(`{${name}}`).join(typeof v === "number" && COUNT_VARS.has(name) ? formatCount(lang, v) : String(v));
  return out;
}

/** Outside React. */
export const ct = (key: string, vars?: Vars): string => translate(useLang.getState().lang, key, vars);

/** Inside a component: re-renders when Shard's language changes. */
export function useCt(): (key: string, vars?: Vars) => string {
  const lang = useLang((s) => s.lang);
  return useMemo(() => (key: string, vars?: Vars) => translate(lang, key, vars), [lang]);
}
