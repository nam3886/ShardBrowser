// Example data for `npm run dev:mock` (not real accounts). `call()` in api.ts
// answers from here when VITE_MOCK is set; a release build never loads it.
// localStorage "creator:mock" = "empty" | "error" shows a page's empty or
// load-error state.
import type {
  AccountList, AccountRow, AccountStatus, CreatorSettings, EmailDetail, EmailList, EmailRow, EventRow, MailMessage,
  MailboxType, NavCounts, PlatformCard, ProxyDetail, ProxyList, ProxyRow, ProxyTest, RunState, StatusGroup,
} from "./types";

/** A Shard profile id (8 hex, as Shard's), one per mailbox. */
const profileId = (mailbox: number) => ((mailbox * 2654435761) >>> 0).toString(16).padStart(8, "0");
const ago = (min: number) => new Date(Date.now() - min * 60_000).toISOString();
const ELE = "@ele-mail.example";

// Tuples, not `label: "…"`: check:i18n reads that as UI text; this is seed data.
const named = (key: string, name: string) => ({ key, label: name });
export const PLATFORMS: PlatformCard[] = [
  { ...named("google", "Google"), category: "signin", ready: true, routes: ["email"], account_rule: "one", rule_fixed: true, starred: true, secrets: [], free: 0, options: [] },
  { ...named("qwen", "Qwen"), category: "chat", ready: true, routes: ["google", "email"], account_rule: "one", rule_fixed: true, starred: true, secrets: [], free: 0, options: [{ key: "qwen_captcha_retries", value: 3, min: 1, max: 10 }] },
  { ...named("chatgpt", "ChatGPT"), category: "chat", ready: true, routes: ["google"], account_rule: "one", rule_fixed: false, starred: true, secrets: [], free: 0, options: [{ key: "chatgpt_signin_retries", value: 3, min: 1, max: 10 }, { key: "chatgpt_age_retries", value: 3, min: 1, max: 10 }, { key: "chatgpt_chat_wait_s", value: 60, min: 20, max: 300 }] },
  { ...named("grok", "Grok"), category: "chat", ready: true, routes: ["google", "email"], account_rule: "one", rule_fixed: true, starred: false, secrets: [], free: 0, options: [{ key: "grok_age_retries", value: 3, min: 1, max: 10 }, { key: "grok_chat_wait_s", value: 60, min: 20, max: 300 }] },
  { ...named("elevenlabs", "ElevenLabs"), category: "voice", ready: true, routes: ["google", "email"], account_rule: "one", rule_fixed: false, starred: false, secrets: ["api_key"], free: 0, options: [] },
  { ...named("breezeblue", "Breeze Blue"), category: "voice", ready: true, routes: ["email", "google"], account_rule: "separate", rule_fixed: false, starred: true, secrets: ["api_key"], free: 0, options: [] },
  { ...named("fish", "Fish Audio"), category: "voice", ready: false, routes: ["email", "google"], account_rule: "one", rule_fixed: false, starred: false, secrets: ["api_key"], free: 0, options: [{ key: "fish_submit_retries", value: 5, min: 1, max: 10 }] },
  { ...named("noiz", "Noiz"), category: "voice", ready: false, routes: ["email", "google"], account_rule: "one", rule_fixed: false, starred: false, secrets: ["api_key"], free: 0, options: [] },
];
const PLAT = Object.fromEntries(PLATFORMS.map((p) => [p.key, p]));

const USERS = [
  "alice.nguyen", "bob.tran", "cara.le", "duy.pham", "em.vo", "phuong.do", "quang.ha", "linh.bui", "minh.ho", "ngoc.dang",
  "tam.ly", "vy.ta", "hoa.mai", "son.cao", "khanh.vu", "lan.trinh", "an.dinh", "bao.luu", "chi.kieu", "dat.ngo",
  "giang.ton", "hai.luong", "huy.mac", "kim.phan", "long.thai", "mai.quach", "nam.ly", "oanh.ta", "phat.vuong", "quyen.lam",
  // A very long address: every list and Peek must cut it, never wrap or overflow.
  "nguyen.thi.thanh.huong.marketing.department.team2026",
];
const emails: EmailRow[] = USERS.map((u, i) => {
  const provider: MailboxType = i % 3 === 2 ? "outlook" : "ele";
  return { id: i + 1, email: u + (provider === "outlook" ? "@outlook.example" : ELE), provider, status: i === 11 ? "gone" : "active", mails: i === 0 ? 120 : i % 4 === 3 ? 0 : 2 + (i % 4), created_at: ago(600 + i * 47), accounts: [] };
});

const FAILS: Record<string, { code: string; group: string; retryable: boolean }> = {
  "proxy refused the connection": { code: "PROXY_REFUSED", group: "proxy", retryable: true },
  "An account already exists for this email": { code: "ACCOUNT_EXISTS", group: "account", retryable: false },
  "This email address is not supported": { code: "EMAIL_REFUSED", group: "mail", retryable: false },
  "captcha not solved": { code: "CAPTCHA_UNSOLVED", group: "captcha", retryable: true },
  "Cloudflare check kept coming back": { code: "CLOUDFLARE_LOOP", group: "captcha", retryable: true },
  "Google refused the signup (BAD_REQUEST)": { code: "GOOGLE_REFUSED", group: "refused", retryable: true },
  "ChatGPT account deactivated": { code: "ACCOUNT_BANNED", group: "account", retryable: false },
  "Timed out waiting for the verification mail from no-reply@breezeblue.example after 90 seconds; the mailbox answered, but nothing new arrived": { code: "NO_MAIL", group: "mail", retryable: true },
};

// [email index, platform, status, extra]: 40 accounts over every status.
type Seed = [number, string, AccountStatus, Partial<AccountRow>?];
const SEEDS: Seed[] = [
  [0, "google", "linked"], [0, "qwen", "linked"], [0, "chatgpt", "linked"], [0, "grok", "running", { step: "Waiting for the age check" }], [0, "elevenlabs", "linked", { frozen: true, relay_id: "relay-1" }],
  [1, "google", "linked"], [1, "qwen", "linked"], [1, "chatgpt", "failed", { error: "ChatGPT account deactivated" }],
  [3, "google", "linked"], [3, "breezeblue", "linked", { relay_id: "relay-2" }],
  [4, "google", "linked", { verify_status: "expired" }], [4, "qwen", "linked"],
  [5, "google", "linked"], [5, "grok", "linked"], [5, "chatgpt", "queued"],
  [6, "google", "failed", { error: "Google refused the signup (BAD_REQUEST)" }],
  [7, "google", "linked"], [7, "chatgpt", "failed", { error: "Cloudflare check kept coming back" }],
  [8, "google", "linked"], [8, "qwen", "running", { step: "Solving the captcha" }],
  [9, "google", "linked"], [9, "chatgpt", "linked", { verify_status: "signed_out" }],
  [12, "google", "linked"], [12, "breezeblue", "running", { step: "Reading the verification mail" }],
  [13, "google", "linked"], [13, "grok", "queued"],
  [15, "google", "linked"], [15, "elevenlabs", "linked", { frozen: true }],
  [16, "breezeblue", "failed", { error: "Timed out waiting for the verification mail from no-reply@breezeblue.example after 90 seconds; the mailbox answered, but nothing new arrived" }],
  [17, "breezeblue", "linked"], [18, "google", "linked"], [18, "elevenlabs", "failed", { error: "captcha not solved" }],
  [19, "breezeblue", "linked", { relay_id: "relay-3" }], [21, "google", "linked"], [21, "qwen", "linked"],
  [22, "breezeblue", "running", { step: "Filling the sign-up form" }], [24, "google", "linked"], [24, "grok", "failed", { error: "captcha not solved" }],
  [27, "breezeblue", "linked"], [28, "google", "linked"],
  [26, "qwen", "failed", { error: "proxy refused the connection" }], [25, "grok", "failed", { error: "An account already exists for this email" }],
  [23, "breezeblue", "failed", { error: "This email address is not supported" }], [30, "google", "linked"], [30, "breezeblue", "queued"],
];
// Our proxies below (the Proxies page), the first five.
const PROXIES_ADDR = ["vn1.resi.example:1080", "vn2.resi.example:1080", "us-mob.example:8080", "198.51.100.17:3128", "198.51.100.18:3128"];
const CC = ["VN", "VN", "US", "SG", "SG"];
const accounts: AccountRow[] = SEEDS.map(([ei, platform, status, extra], i) => {
  const m = emails[ei];
  m.accounts.push({ platform, status, verify_status: status === "linked" ? (extra?.verify_status ?? "active") : null });
  const error = extra?.error ?? null;
  return {
    id: 1000 + i, mailbox_id: m.id, email: m.email, mailbox_type: m.provider, platform, status,
    route: platform === "google" || (platform === "breezeblue" && ei % 2) ? "email" : PLAT[platform].routes[0],
    frozen: false, busy: ["queued", "running"].includes(status), relay_id: null, error, fail: error ? FAILS[error] : null,
    verify_status: status === "linked" ? "active" : null, browser_id: m.id, profile_id: profileId(m.id), proxy: ei % 7 === 6 ? null : PROXIES_ADDR[ei % 5], proxy_cc: ei % 7 === 6 ? null : CC[ei % 5],
    step: null, created_at: ago(500 - i * 9), ...extra,
  };
});

const groupOf = (a: AccountRow): StatusGroup =>
  a.status === "linked" && (a.verify_status === "signed_out" || a.verify_status === "expired") ? "signed_out"
  : a.status === "queued" || a.status === "running" ? "making" : a.status as StatusGroup;


const ev = (id: number, min: number, level: EventRow["level"], step: string, text: string): EventRow => ({ id, at: ago(min), level, step, message: text });
const EVENTS: Record<number, EventRow[]> = {
  1007: [
    ev(1, 42, "info", "profile", "Browser profile ready on 198.51.100.11:3128"),
    ev(2, 41, "info", "signin", "Signed in with Google"),
    ev(3, 39, "info", "age", "Age check passed"),
    ev(4, 38, "warn", "chat", "First chat took 41 s (limit 60 s)"),
    ev(5, 37, "error", "verify", "ChatGPT says the account is deactivated"),
  ],
  1003: [
    ev(6, 6, "info", "profile", "Browser profile ready on 203.0.113.24:8080"),
    ev(7, 5, "info", "signin", "Signed in with Google"),
    ev(8, 1, "info", "age", "Waiting for the age check"),
  ],
  1028: [
    ev(9, 18, "info", "form", "Sign-up form sent"),
    ev(10, 17, "warn", "mail", "Mailbox answered slowly; trying again in 4 s"),
    ev(11, 16, "error", "mail", "No verification mail after 90 s"),
  ],
};

const MAILS: MailMessage[] = [
  { id: 5, sender: "Breeze Blue <no-reply@breezeblue.example>", subject: "Confirm your email", body: "Click the link below to confirm your email address.\n\nhttps://breezeblue.example/verify?t=MOCK", received_at: ago(2), code: null, link: "https://breezeblue.example/verify?t=MOCK" },
  { id: 4, sender: "Qwen <noreply@qwen.ai>", subject: "Welcome to Qwen Chat", body: "Hi,\n\nYour Qwen account is ready.\n\nThe Qwen team", received_at: ago(30), code: null, link: null },
  { id: 3, sender: "Google <noreply@google.com>", subject: "Your verification code", body: "Your Google verification code is: 482913\n\nIf you did not request this, ignore this email.", received_at: ago(40), code: "482913", link: null },
  { id: 1, sender: "Google <noreply@google.com>", subject: "Security alert", body: "A new sign-in on Windows.\nIf this was you, no action is needed.", received_at: ago(61), code: null, link: null },
];

const mailOf = (id: number, n: number): MailMessage[] => id !== 1 ? MAILS.slice(0, n)
  : [...MAILS, ...Array.from({ length: 116 }, (_, i) => ({ id: 100 + i, sender: "Qwen <noreply@qwen.ai>", subject: `Your weekly summary #${116 - i}`, body: "", received_at: ago(120 + i * 600), code: null, link: null }))];

let settings: CreatorSettings = {
  ele_api_url: "https://ele.example/api", ele_key_saved: true, yescaptcha_key_saved: false,
  relay_url: "https://relay.example", relay_key_saved: true, delete_failed_browser: false,
  blocked_domains: ["mailinator.example", "tempbox.example"],
  otp_timeout: 90, max_browsers: 3, fail_streak_stop: 3, proxy_rest_days: 7,
};

// Our sign-up proxies. Hosts are .example names and documentation ranges
// (RFC 5737); every secret is plainly fake.
const tested = (ok: boolean, ms: number | null, ip: string | null, cc: string, city: string, isp: string, minAgo: number, error: string | null = null): ProxyTest =>
  ({ ok, tcp_ms: ms, ip, country_code: cc, region: null, city, isp, timezone: null, at: ago(minAgo), error });
const px = (id: number, name: string, kind: ProxyRow["kind"], hp: string, rest: Partial<ProxyRow>): ProxyRow => {
  const [host, port] = hp.split(":");
  return {
    id, name, kind, host, port: Number(port), username: "user" + id, country: "", notes: "", enabled: true, test: null, testing: false, accounts: 0,
    ip_type: null, rotating: false, signups_ok: 0, signups_total: 0, fail_streak: 0, rest_until: null, created_at: ago(9000 - id * 300), ...rest,
  };
};
let proxies: ProxyRow[] = [
  px(1, "VN resi 01", "socks5", "vn1.resi.example:1080", { test: tested(true, 530, "203.0.113.8", "VN", "Ho Chi Minh City", "Viettel Group", 12), accounts: 14, ip_type: "residential", signups_ok: 12, signups_total: 14 }),
  px(2, "VN resi 02", "socks5", "vn2.resi.example:1080", { test: tested(true, 612, "203.0.113.17", "VN", "Hanoi", "VNPT Corp", 12), accounts: 9, ip_type: "residential", rotating: true, signups_ok: 8, signups_total: 9 }),
  px(3, "US mobile", "http", "us-mob.example:8080", { test: tested(true, 890, "198.51.100.140", "US", "Dallas", "T-Mobile USA", 40), accounts: 6, ip_type: "mobile", rotating: true, signups_ok: 5, signups_total: 6 }),
  px(4, "DC 17", "http", "198.51.100.17:3128", { test: tested(true, 88, "198.51.100.17", "SG", "Singapore", "Example Hosting", 25), accounts: 21, ip_type: "datacenter", signups_ok: 9, signups_total: 21, fail_streak: 3, rest_until: ago(-60 * 24 * 6) }),
  px(5, "DC 18", "http", "198.51.100.18:3128", { test: tested(true, 91, "198.51.100.18", "SG", "Singapore", "Example Hosting", 25), accounts: 11, ip_type: "datacenter", signups_ok: 8, signups_total: 11, fail_streak: 1 }),
  px(6, "Old datacenter", "https", "192.0.2.66:443", { test: tested(false, null, null, "NL", "", "", 35, "connection refused"), accounts: 4, ip_type: "datacenter", signups_ok: 2, signups_total: 7, fail_streak: 5, rest_until: ago(-60 * 24 * 7) }),
  px(7, "JP ISP", "socks5", "203.0.113.90:1080", { test: tested(true, 240, "203.0.113.90", "JP", "Tokyo", "NTT Communications", 60), accounts: 3, ip_type: "residential", signups_ok: 3, signups_total: 3, testing: true }),
  px(8, "New PL", "socks5", "192.0.2.44:1080", {}),
  // Slow (Shard has no Slow word: the time says it), last tested days ago, and one turned off.
  px(9, "BR resi slow", "socks5", "br.resi.example:1080", { test: tested(true, 2400, "203.0.113.49", "BR", "São Paulo", "Claro S.A.", 30), accounts: 2, ip_type: "residential", signups_ok: 1, signups_total: 2 }),
  px(10, "DE ISP old test", "http", "198.51.100.33:8080", { test: tested(true, 140, "198.51.100.33", "DE", "Frankfurt am Main", "Deutsche Telekom AG", 60 * 24 * 4), accounts: 1, ip_type: "residential", signups_ok: 1, signups_total: 1 }),
  px(11, "Spare DC", "http", "198.51.100.40:3128", { enabled: false, test: tested(true, 95, "198.51.100.40", "SG", "Singapore", "Example Hosting", 300), ip_type: "datacenter" }),
];
const PROXY_COUNT = (() => { try { return Number(localStorage.getItem("creator:mock.proxies") ?? 0); } catch { return 0; } })();
for (let i = 1; i <= PROXY_COUNT; i++) {
  proxies.push(px(100 + i, `Pool ${String(i).padStart(4, "0")}`, "http", `10.${Math.floor(i / 250)}.${i % 250}.1:3128`, i % 3 === 0 ? {} : {
    test: tested(true, 80 + (i % 40) * 11, `10.${Math.floor(i / 250)}.${i % 250}.1`, "VN", "Hanoi", i % 2 ? "Viettel Group" : "Example Hosting", 90), ip_type: i % 2 ? "residential" : "datacenter", accounts: i % 7, signups_ok: i % 5, signups_total: i % 7,
  }));
}
const proxyStatusOf = (p: ProxyRow) => (!p.enabled ? "off" : !p.test ? "untested" : p.test.ok ? "working" : "failed");
const restingNow = (p: ProxyRow) => !!p.rest_until && Date.parse(p.rest_until) > Date.now();

/** localStorage "creator:mock": "empty" | "error" | "setup" (first run:
 *  no Ele key, no proxies, no accounts) | "engine" (Shard updating its engine) | "slow"
 *  (every answer takes 4 s: the loading states) | "big" (huge counts). */
const mockMode = () => { try { return localStorage.getItem("creator:mock"); } catch { return null; } };
const BIG = 512_340;

type Spec = { offset?: number; limit?: number; q?: string; tab?: string; status?: string; kind?: string };
const page = <T,>(rows: T[], spec: Spec) => {
  const offset = spec.offset ?? 0;
  return { items: rows.slice(offset, offset + (spec.limit ?? 25)), total: rows.length, offset };
};
const has = (s: string, q?: string) => !q?.trim() || s.toLowerCase().includes(q.trim().toLowerCase());

/** Display's order (the lists come newest first). */
const ordered = <T extends { email: string }>(rows: T[], order: unknown) =>
  order === "oldest" ? [...rows].reverse() : order === "email" ? [...rows].sort((x, y) => x.email.localeCompare(y.email)) : rows;

const freeFor = (k: string) => emails.filter((m) => m.status === "active" && !m.accounts.some((a) => a.platform === k && a.status !== "failed")).length;

const HANDLERS: Record<string, (args: Record<string, unknown>) => unknown> = {
  accounts: (a) => {
    const spec = a as Spec;
    const flag = (r: AccountRow) => (a.flag === "frozen" ? r.frozen : a.flag === "pushed" ? !!r.relay_id : a.flag === "not_pushed" ? !r.relay_id : true);
    const inTab = accounts.filter((r) => (!spec.tab || r.platform === spec.tab) && has(r.email, spec.q) && flag(r));
    const counts = { all: inTab.length } as AccountList["counts"];
    for (const g of ["linked", "signed_out", "making", "failed"] as const) counts[g] = inTab.filter((r) => groupOf(r) === g).length;
    const tabs = PLATFORMS.map((p) => ({ platform: p.key, count: accounts.filter((r) => r.platform === p.key).length })).filter((t) => t.count);
    const rows = ordered(inTab.filter((r) => !spec.status || spec.status === "all" || groupOf(r) === spec.status), a.order);
    // Huge counts: the same rows, the totals of a 500k store.
    if (mockMode() === "big") return { ...page(rows, spec), total: BIG, tabs: tabs.map((t) => ({ ...t, count: t.count * 12_000 })), counts: { ...counts, all: BIG, linked: 431_906, failed: 61_212 } } satisfies AccountList;
    return { ...page(rows, spec), tabs, counts } satisfies AccountList;
  },
  run_state: () => mockMode() === "setup" ? { jobs: [], halted: [], setup: { ele: false, proxies: 0, captcha: false, engine_busy: false } } satisfies RunState : ({
    setup: { ele: settings.ele_key_saved, proxies: proxies.length, captcha: settings.yescaptcha_key_saved, engine_busy: mockMode() === "engine" },
    // "engine": Shard is updating or swapping the engine; the job waits (its words are ours: creator.job.engineWait).
    jobs: [{ job_id: 7, platform: "breezeblue", total: 5, linked: 2, making: 2, failed: 1, step: mockMode() === "engine" ? null : "Reading the verification mail" }],
    halted: [{ platform: "elevenlabs", n: 5, error: "captcha not solved", code: "CAPTCHA_UNSOLVED" }],
  } satisfies RunState),
  nav_counts: () => (mockMode() === "big"
    ? { accounts: BIG, emails: 38_412, platforms: PLATFORMS.length, proxies: 1_010 }
    : { accounts: accounts.length, emails: emails.length, platforms: PLATFORMS.length, proxies: proxies.length }) satisfies NavCounts,
  /** Why Make cannot start now, for this service and route; none: it can. */
  make_check: (a) => {
    const out: string[] = [];
    const setup = mockMode() === "setup";
    if (a.platform === "elevenlabs" && String(a.route ?? "").startsWith("email") && !settings.yescaptcha_key_saved) out.push("NO_CAPTCHA_KEY");
    if (setup) out.push("NO_ELE_KEY", "NO_PROXY");
    if (mockMode() === "engine") out.push("ENGINE_BUSY");
    return out;
  },
  test_ele: () => {
    if (mockMode() === "setup" || !settings.ele_key_saved) throw new Error("Ele API URL and key are not set");
    return { domains: 12 };
  },
  test_relay: () => {
    if (mockMode() === "setup" || !settings.relay_key_saved) throw new Error("Set the relay address and admin key in Settings");
    return { ok: true };
  },
  test_mailbox: (a) => { const m = emails.find((x) => String(x.id) === String(a.id)); return !m ? "failed" : m.status === "gone" ? "gone" : m.id % 9 === 4 ? "failed" : "active"; },
  test_proxy: (a) => { const p = proxies.find((x) => String(x.id) === String(a.id)); return !!p?.test?.ok; },
  /** Our side table for Shard's proxies, made up from each id. */
  proxies: (a) => {
    const f = (p: ProxyRow) =>
      (!a.status || proxyStatusOf(p) === a.status) && (!a.kind || p.kind === a.kind) && (!a.ip_type || p.ip_type === a.ip_type)
      && (!a.resting || (a.resting === "resting") === restingNow(p))
      && (!a.country || (p.test?.country_code ?? p.country) === a.country)
      && (!a.rotation || (a.rotation === "rotating") === p.rotating)
      && has(`${p.name} ${p.host}:${p.port} ${p.test?.ip ?? ""}`, a.q as string);
    const rows = proxies.filter(f);
    const by = a.order === "name" ? (x: ProxyRow, y: ProxyRow) => x.name.localeCompare(y.name)
      : a.order === "latency" ? (x: ProxyRow, y: ProxyRow) => (x.test?.tcp_ms ?? 1e9) - (y.test?.tcp_ms ?? 1e9)
      : a.order === "accounts" ? (x: ProxyRow, y: ProxyRow) => y.accounts - x.accounts : null;
    const n = (k: (p: ProxyRow) => boolean) => proxies.filter(k).length;
    return {
      ...page(by ? [...rows].sort(by) : rows, a),
      counts: {
        all: proxies.length, working: n((p) => proxyStatusOf(p) === "working"), failed: n((p) => proxyStatusOf(p) === "failed"), untested: n((p) => !p.test),
        off: n((p) => !p.enabled), residential: n((p) => p.ip_type === "residential"), datacenter: n((p) => p.ip_type === "datacenter"), mobile: n((p) => p.ip_type === "mobile"), resting: n(restingNow),
        static: n((p) => !!p.test && !p.rotating), rotating: n((p) => p.rotating),
      },
      failing: proxies.filter((p) => proxyStatusOf(p) === "failed").sort((x, y) => Date.parse(y.test!.at) - Date.parse(x.test!.at))
        .map((p) => ({ id: p.id, name: p.name, error: p.test!.error ?? "", tested_at: p.test!.at, accounts: p.accounts })),
    } satisfies ProxyList;
  },
  proxy: (a) => {
    const p = proxies.find((x) => x.id === a.id)!;
    const t = p.test;
    const history = !t?.ip ? [] : p.rotating
      ? [{ ip: t.ip, country_code: t.country_code, city: t.city, first_seen: ago(60), last_seen: t.at }, { ip: "203.0.113.200", country_code: t.country_code, city: t.city, first_seen: ago(600), last_seen: ago(70) }]
      : [{ ip: t.ip, country_code: t.country_code, city: t.city, first_seen: ago(4000), last_seen: t.at }];
    const using = accounts.filter((x) => x.proxy === `${p.host}:${p.port}`).slice(0, 5)
      .map((x) => ({ id: x.id, email: x.email, platform: x.platform, status: x.status, verify_status: x.verify_status }));
    return { ...p, password: "mock-pass-" + p.id, history, using, classified_by: !p.ip_type ? null : p.id === 7 ? "isp" : "ip-api" } satisfies ProxyDetail;
  },
  proxies_add: (a) => (a.entries as unknown[]).length,
  /** A pasted line tested without keeping anything (the backend's measure). */
  proxy_test_entry: (a) => {
    const e = a.entry as { host: string };
    const ok = !e.host.startsWith("192.0.2.");
    return { ip: ok ? e.host : "", country_code: ok ? "US" : "", tcp_ms: ok ? 120 + e.host.length * 7 : null, udp_ms: null, last_seen: new Date().toISOString(), geo_error: ok ? null : "connection refused" };
  },
  /** The last page a failed sign-up saw (a drawn stand-in; the real one is a PNG). */
  account_screenshot: (a) => (Number(a.id) % 3 === 0 ? null : `data:image/svg+xml;utf8,${encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1280 800"><rect width="1280" height="800" fill="#f4f4f5"/><rect y="0" width="1280" height="56" fill="#e4e4e7"/><rect x="440" y="200" width="400" height="320" rx="12" fill="#fff" stroke="#d4d4d8"/><rect x="480" y="250" width="220" height="22" rx="4" fill="#a1a1aa"/><rect x="480" y="300" width="320" height="44" rx="6" fill="#e4e4e7"/><rect x="480" y="360" width="320" height="44" rx="6" fill="#e4e4e7"/><rect x="480" y="440" width="320" height="44" rx="8" fill="#52525b"/></svg>',
  )}`),
  account_events: (a) => EVENTS[Number(a.id)] ?? [],
  /** One secret of one account; null when the platform keeps none of that kind. */
  account_secret: (a) => {
    if (a.key === "password") return "Mock-Pass-2026!";
    const acc = accounts.find((x) => x.id === Number(a.id));
    return acc && PLAT[acc.platform].secrets.includes(String(a.key)) ? "sk_mock_0123456789abcdef" : null;
  },
  emails: (a) => {
    const spec = a as Spec & { free_for?: string };
    const pool = emails.filter((m) => has(m.email, spec.q) && (!spec.status || m.status === spec.status) && (!spec.free_for || (m.status === "active" && !m.accounts.some((x) => x.platform === spec.free_for))));
    const rows = ordered(pool.filter((m) => !spec.kind || spec.kind === "all" || m.provider === spec.kind), a.order);
    return { ...page(rows, spec), counts: { all: pool.length, outlook: pool.filter((m) => m.provider === "outlook").length, ele: pool.filter((m) => m.provider === "ele").length } } satisfies EmailList;
  },
  email: (a) => {
    const m = emails.find((x) => x.id === Number(a.id));
    if (!m) throw new Error("Email not found");
    // Mailbox 1 holds a long history: the Peek loads older mail as it scrolls (50 at a time).
    const all = mailOf(m.id, m.mails);
    return { ...m, items: all.slice(0, 50), more: all.length > 50, technical: { browser_id: m.id, profile: m.accounts.length ? profileId(m.id) : null, proxy: m.accounts.length ? PROXIES_ADDR[m.id % 5] : null } } satisfies EmailDetail;
  },
  email_mail: (a) => { const all = mailOf(Number(a.id), 0); const o = Number(a.offset); return { items: all.slice(o, o + 50), more: all.length > o + 50 }; },
  platforms: () => PLATFORMS.map((p) => ({ ...p, free: freeFor(p.key) })),
  /** How many free emails each sign-up route has for a platform ("" = Auto). */
  make_options: (a) => {
    const p = PLAT[String(a.platform)];
    const free = freeFor(p.key);
    const out: Record<string, number> = { "": free };
    if (p.routes.includes("email")) { out["email:outlook"] = Math.floor(free / 3); out["email:ele"] = free - Math.floor(free / 3); }
    for (const r of p.routes.filter((r) => r !== "email")) out[r] = Math.max(0, free - 4);
    return out;
  },
  parse_outlook: (a) => {
    const lines = String(a.text ?? "").split("\n").map((l) => l.trim()).filter(Boolean);
    const ok = lines.filter((l) => l.split("|").length === 5 && l.includes("@"));
    return { valid: ok.length, invalid: lines.length - ok.length };
  },
  /** A Make run (the real one makes Breeze Blue accounts one by one). */
  breeze_run: () => null,
  breeze_stop: () => null,
  settings: () => settings,
  settings_set: (a) => {
    const patch = a.patch as Partial<CreatorSettings>;
    if (patch.ele_api_url !== undefined && !/^https?:\/\//.test(patch.ele_api_url)) throw new Error("Ele API address must start with http:// or https://");
    settings = { ...settings, ...patch };
    return settings;
  },
};

export async function mockCall<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  const mode = mockMode();
  await new Promise((r) => setTimeout(r, mode === "slow" ? 4000 : 60));
  if (mode === "error") throw new Error("creator backend: database is locked (mock)");
  if ((mode === "empty" || mode === "setup") && ["accounts", "emails", "proxies"].includes(cmd)) {
    return { items: [], total: 0, offset: 0, tabs: [], counts: { all: 0 } } as T;
  }
  if ((mode === "empty" || mode === "setup") && cmd === "nav_counts") return { accounts: 0, emails: 0, platforms: PLATFORMS.length, proxies: 0 } as T;
  if (mode === "empty" && cmd === "run_state") return { jobs: [], halted: [] } as T;
  const h = HANDLERS[cmd];
  if (!h) throw new Error(`mock: unknown command ${cmd}`);
  return structuredClone(h(args)) as T;
}
