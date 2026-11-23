// node --test: our dictionary (src/creator/i18n) covers every key our pages use, in every language.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const DIR = fileURLToPath(new URL(".", import.meta.url));
const dict = (lang: string): Record<string, string> => JSON.parse(readFileSync(join(DIR, "i18n", `${lang}.json`), "utf8"));
const en = dict("en");

test("every creator.* key named under src/creator exists in en.json", () => {
  // Any "creator.…" literal, not only ct("…"): keys also sit in tables (labelKey).
  const files = readdirSync(DIR, { recursive: true, encoding: "utf8" }).filter((f) => /\.tsx?$/.test(f) && !f.endsWith(".test.ts"));
  const used = files.flatMap((f) => [...readFileSync(join(DIR, f), "utf8").matchAll(/["'`](creator\.[A-Za-z0-9_.]+)["'`]/g)].map((m) => m[1]));
  assert.ok(used.length > 0, "found no creator.* keys");
  assert.deepEqual(used.filter((k) => !(k in en)), []);
});

// Keys built at run time (ct(`creator.platform.${k}`)): every value the
// backend or our types can give. A new built key needs its values here.
const RUNTIME: Record<string, string[]> = {
  // engine/errors.py: every code the backend can send.
  "creator.error.": ["INTERRUPTED", "QUIT_AFTER_OTP", "WINDOW_CLOSED", "PROXY_UNUSABLE", "NO_PROXY", "PROXY_REFUSED", "PAGE_LOAD_FAILED", "GEO_BLOCKED", "PROXY_CHECK_FAILED", "CAPTCHA_RATE_LIMITED", "CAPTCHA_UNSOLVED", "CLOUDFLARE_LOOP", "CAPTCHA_EXTENSION", "NO_CAPTCHA_KEY", "MAILBOX_GONE", "MAIL_API_DOWN", "NO_MAIL", "CODE_NOT_SENT", "CODE_REJECTED", "VERIFY_LINK_FAILED", "DOMAIN_BLOCKED", "EMAIL_REFUSED", "GOOGLE_REFUSED", "SIGNIN_REFUSED", "AGE_STEP_FAILED", "SIGNUP_NOT_COMPLETED", "NOT_SIGNED_IN", "API_KEY_FAILED", "NO_CHAT_REPLY", "ACCOUNT_EXISTS", "ACCOUNT_BANNED", "EMAIL_NOT_VERIFIED", "GOOGLE_SIGNED_OUT", "PARENT_FAILED", "SESSION_NOT_KEPT", "CONSOLE_CHECK", "BROWSER_HUNG", "PAGE_CHANGED", "STEP_FAILED", "APP_ERROR", "PROFILE_RUNNING", "NO_BROWSER", "ENGINE_UNAVAILABLE", "SIGNUP_FAILED", "OTHER"],
  "creator.platform.": ["google", "qwen", "chatgpt", "grok", "elevenlabs", "breezeblue", "fish", "noiz"],
  "creator.category.": ["signin", "chat", "voice"],
  "creator.emailState.": ["active", "gone"],
  "creator.events.level.": ["info", "warn", "error"],
  "creator.flag.": ["frozen", "pushed", "not_pushed"],
  "creator.ipType.": ["residential", "datacenter", "mobile"],
  "creator.knob.": ["qwen_captcha_retries", "chatgpt_signin_retries", "chatgpt_age_retries", "chatgpt_chat_wait_s", "grok_age_retries", "grok_chat_wait_s", "fish_submit_retries"],
  "creator.mailbox.": ["outlook", "ele"],
  "creator.proxy.foundBy.": ["ip-api", "isp"],
  "creator.emailPeek.gone.": ["outlook", "ele"],
  "creator.make.block.": ["NO_ELE_KEY", "ENGINE_BUSY", "NO_PROXY", "NO_CAPTCHA_KEY"],
  "creator.make.route.": ["outlook", "ele"],
  "creator.status.": ["linked", "signed_out", "making", "queued", "failed"],
};

test("every built ct key has its values listed, and each exists in en.json", () => {
  const files = readdirSync(DIR, { recursive: true, encoding: "utf8" }).filter((f) => f.endsWith(".tsx") || f.endsWith(".ts"));
  const prefixes = new Set(files.flatMap((f) => [...readFileSync(join(DIR, f), "utf8").matchAll(/\bct\(\s*`([^`$]*)\$\{/g)].map((m) => m[1])));
  assert.ok(prefixes.size > 0, "found no built ct() keys");
  assert.deepEqual([...prefixes].filter((p) => !(p in RUNTIME)), []);
  assert.deepEqual(Object.entries(RUNTIME).flatMap(([p, vs]) => vs.map((v) => p + v)).filter((k) => !(k in en)), []);
});

test("ru and zh have every en key, non-empty", () => {
  for (const lang of ["ru", "zh"]) {
    const d = dict(lang);
    assert.deepEqual(Object.keys(en).filter((k) => !d[k]?.trim()), [], lang);
  }
});

test("no key in ru/zh that en lacks", () => {
  for (const lang of ["ru", "zh"]) assert.deepEqual(Object.keys(dict(lang)).filter((k) => !(k in en)), [], lang);
});

test("our words follow Shard's copy rules (scripts/lib/copy-rules.mjs)", async () => {
  // @ts-expect-error: a plain .mjs module without types
  const { copyProblems } = await import("../../scripts/lib/copy-rules.mjs");
  // Names Shard's list does not know: services, mail providers, our app.
  const OURS = /^(?:Ele|Outlook|Breeze|Blue|Fish|Audio|YesCaptcha|CapSolver|ElevenLabs|Qwen|ChatGPT|Grok|Ghidorah|Creator|Shard’s|ShardBrowser|Google’s)$/;
  const problems = (copyProblems({ en, ru: dict("ru"), zh: dict("zh"), notes: [] }) as { rule: string; where: string; message: string }[])
    .filter((p) => !(p.rule === "sentence-case" && p.message.replace(/^.*: /, "").split(", ").every((w) => OURS.test(w))));
  assert.deepEqual(problems.map((p) => `${p.rule} ${p.where}: ${p.message}`), []);
});
