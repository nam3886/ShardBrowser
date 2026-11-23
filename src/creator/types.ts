// Row types our backend commands return. Field names follow the Python app's
// API (ghidorah_creator/api.py): list rows never carry a secret; a secret is
// read one at a time (`account_secret`) when a person copies it.

export type MailboxType = "outlook" | "ele";
/** Account status as stored; `signed_out` is a linked account whose last check said so (list group). */
export type AccountStatus = "queued" | "running" | "linked" | "failed";
export type StatusGroup = "linked" | "signed_out" | "making" | "failed";
export type VerifyStatus = "active" | "signed_out" | "expired" | "banned" | null;

/** engine/errors.py fail_info: the code only; its words are ours (`creator.error.<CODE>`, en/ru/zh). */
export type FailInfo = { code: string; group: string; retryable: boolean };

export type AccountRow = {
  id: number;
  mailbox_id: number;
  email: string;
  mailbox_type: MailboxType;
  platform: string;
  status: AccountStatus;
  route: string;
  frozen: boolean;
  busy: boolean;
  relay_id: string | null;
  error: string | null;
  fail: FailInfo | null;
  verify_status: VerifyStatus;
  browser_id: number | null;
  /** Its browser: a Shard profile in the "Creator" folder (D17); null until the mailbox is first used. */
  profile_id: string | null;
  proxy: string | null;
  proxy_cc: string | null;
  /** The step a running account is on (live activity). */
  step?: string | null;
  created_at: string;
};

export type AccountList = {
  items: AccountRow[];
  total: number;
  offset: number;
  tabs: { platform: string; count: number }[];
  counts: Record<"all" | StatusGroup, number>;
};

export type EmailRow = {
  id: number;
  email: string;
  provider: MailboxType;
  status: "active" | "gone";
  mails: number;
  created_at: string;
  accounts: { platform: string; status: AccountStatus; verify_status: VerifyStatus }[];
};

export type EmailList = {
  items: EmailRow[];
  total: number;
  offset: number;
  counts: { all: number; outlook: number; ele: number };
};

export type MailMessage = {
  id: number;
  sender: string;
  subject: string;
  body: string;
  received_at: string;
  code: string | null;
  link: string | null;
};

export type EmailDetail = EmailRow & {
  items: MailMessage[];
  more: boolean;
  technical: { browser_id: number | null; profile: string | null; proxy: string | null };
};

export type PlatformCard = {
  key: string;
  label: string;
  category: "signin" | "chat" | "voice";
  ready: boolean;
  routes: string[];
  account_rule: "one" | "separate";
  rule_fixed: boolean;
  starred: boolean;
  secrets: string[];
  free: number;
  /** The platform's own options (tries, waits in seconds), set in its Edit dialog; shared ones are in Settings. */
  options: { key: string; value: number; min: number; max: number }[];
};

export type EventLevel = "info" | "warn" | "error";
export type EventRow = { id: number; at: string; level: EventLevel; step: string; message: string };

/** Jobs that are running, and queues a failure streak halted (`activity` + `get_state`). */
export type RunState = {
  jobs: { job_id: number; platform: string; total: number; linked: number; making: number; failed: number; step: string | null }[];
  halted: { platform: string; n: number; error: string; code: string }[];
  /** What a run needs that is missing: first run or a lost key. The engine's
   *  own states (download, update, swap) are Shard's to show (FirstRunGate,
   *  EngineUpdateBanner); runs only wait for it (`engine_busy`). */
  setup: { ele: boolean; proxies: number; captcha: boolean; engine_busy: boolean };
};

/** Why Make cannot start (`make_check`), in the order they are said. */
export type MakeBlock = "NO_ELE_KEY" | "ENGINE_BUSY" | "NO_PROXY" | "NO_CAPTCHA_KEY";

/** Found from the exit IP after a test, never chosen by hand (see ProxyRow.ip_type). */
export type IpType = "residential" | "datacenter" | "mobile";
export type ProxyKindName = "socks5" | "http" | "https";

/** The last test, as Shard's snapshot carries it. */
export type ProxyTest = {
  ok: boolean;
  tcp_ms: number | null;
  ip: string | null;
  country_code: string | null;
  region: string | null;
  city: string | null;
  isp: string | null;
  timezone: string | null;
  at: string;
  error: string | null;
};

/** One of our sign-up proxies (our own store, not Shard's). */
export type ProxyRow = {
  id: number;
  name: string;
  kind: ProxyKindName;
  host: string;
  port: number;
  username: string;
  /** A country the person gave; the test's exit country wins when there is one. */
  country: string;
  notes: string;
  /** Off: no new browser gets it (Python's per-proxy "On" switch). */
  enabled: boolean;
  test: ProxyTest | null;
  /** A test is running now. */
  testing: boolean;
  /** Accounts made through it (Shard's "Profiles" column). */
  accounts: number;
  /** After each test our backend asks ip-api.com through the proxy for
   *  `mobile`, `hosting` and `proxy`: mobile gives Mobile, hosting gives
   *  Datacenter, otherwise Residential. No answer: the test's ISP name
   *  against a hosting-company list. Never tested: null. */
  ip_type: IpType | null;
  /** The exit IP changed between two tests (its IP history). */
  rotating: boolean;
  signups_ok: number;
  signups_total: number;
  /** Failed sign-ups in a row; at the limit (Settings) the proxy rests. */
  fail_streak: number;
  rest_until: string | null;
  created_at: string;
};

export type ProxyList = {
  items: ProxyRow[];
  total: number;
  offset: number;
  counts: { all: number; working: number; failed: number; untested: number; off: number; residential: number; datacenter: number; mobile: number; resting: number; static: number; rotating: number };
  /** Turned-on proxies whose last test failed, newest test first (the
   *  "Problems to fix" card). Nothing moves by itself: an account on one fails
   *  with its proxy error until it is retested or given another proxy. */
  failing: { id: number; name: string; error: string; tested_at: string; accounts: number }[];
};

export type ProxyDetail = ProxyRow & {
  password: string;
  /** Newest first: each exit IP and when it was seen. */
  history: { ip: string; country_code: string | null; city: string | null; first_seen: string; last_seen: string }[];
  using: { id: number; email: string; platform: string; status: AccountStatus; verify_status: VerifyStatus }[];
  /** How IP type was found: ip-api's flags, or the ISP name when ip-api did not answer. */
  classified_by: "ip-api" | "isp" | null;
};

/** The sidebar group's counts. */
export type NavCounts = { accounts: number; emails: number; platforms: number; proxies: number };

export type CreatorSettings = {
  ele_api_url: string;
  /** Write-only keys: the backend says whether one is saved, never what it is. */
  ele_key_saved: boolean;
  yescaptcha_key_saved: boolean;
  relay_url: string;
  relay_key_saved: boolean;
  delete_failed_browser: boolean;
  /** New Ele emails on these domains are skipped. */
  blocked_domains: string[];
  otp_timeout: number;
  max_browsers: number;
  fail_streak_stop: number;
  proxy_rest_days: number;
};
