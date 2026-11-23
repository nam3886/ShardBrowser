// Our tab in Shard's Settings page, laid out as its Sync and API & MCP tabs
// (titled SettingsGroups of SettingRows, a Test button at a row's end). Only
// settings every platform shares live here; a platform's own options are in
// its Edit dialog on our Platforms page. A key is write-only: a
// password field with the key icon, as ShardX's API key form draws one, and
// a Saved / Not set badge after its title (a title with a mark, as the Sync
// row's). Changes wait for Save: ours joins Shard's one draft (seam), so
// its Save bar, Cmd/Ctrl+S and leave question are Shard's own.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { create } from "zustand";
import { Badge, Button, Input, Switch, Tag } from "@proxyshard/shardx-ui-kit";
import { KeyIcon } from "../../shared/icons";
import { SettingRow, SettingsGroup } from "../../pages/settings/SettingRow";
import { NumField } from "../../shared/ui/NumField";
import { SelectField } from "../../shared/ui/SelectField";
import { LoadError, LoadingRows } from "../../shared/ui/ListStates";
import { toast } from "../../shared/model/toast";
import { open } from "@tauri-apps/plugin-dialog";
import { call, MOCK } from "../api";
import { t } from "../../shared/i18n";
import { useCt } from "../i18n";
import type { SaveState } from "../../pages/settings/useSettingsDraft";
import type { CreatorSettings } from "../types";

type Keys = { ele_api_key: string; yescaptcha_key: string; relay_key: string };
const NO_KEYS: Keys = { ele_api_key: "", yescaptcha_key: "", relay_key: "" };

/** The backend's limit (`proxy::MAX_REST_SECONDS`, 90 days). */
const PROXY_REST_MAX_DAYS = 90;

/** A save with no answer by then is a failed one (Shard's SAVE_TIMEOUT_MS). */
const SAVE_TIMEOUT_MS = 10_000;
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/** Our tab's draft, joined to Shard's by `useCreatorDraft`. */
const useDraft = create<{
  saved: CreatorSettings | null; s: CreatorSettings | null; keys: Keys; saving: boolean; error: string | null; loadError: string | null;
  load: () => void; set: (patch: Partial<CreatorSettings>) => void; setKey: (k: keyof Keys, v: string) => void; save: () => Promise<boolean>; discard: () => void;
}>((set, get) => ({
  saved: null, s: null, keys: NO_KEYS, saving: false, error: null, loadError: null,
  load: () => {
    if (get().saved) return;
    set({ loadError: null });
    void call<CreatorSettings>("settings").then((v) => set({ saved: v, s: v }), (e) => set({ loadError: String(e) }));
  },
  // An edit drops a failed save's words, as Shard's draft does.
  set: (patch) => set({ s: { ...get().s!, ...patch }, error: null }),
  setKey: (k, v) => set({ keys: { ...get().keys, [k]: v }, error: null }),
  /** Writes the draft (10 s at most). What was typed while it saved stays a
   *  change over what is now saved. A failure says so as Shard's does. */
  save: async () => {
    const sent = get().s!;
    const keys = get().keys;
    set({ saving: true, error: null });
    let late: ReturnType<typeof setTimeout> | undefined;
    try {
      const timeout = new Promise<never>((_, reject) => { late = setTimeout(() => reject(new Error(t("settings.saveTimedOut"))), SAVE_TIMEOUT_MS); });
      const next = await Promise.race([call<CreatorSettings>("settings_set", { patch: sent, keys }), timeout]);
      const cur = get();
      const typed = Object.fromEntries(Object.entries(cur.s!).filter(([k, v]) => !same(v, sent[k as keyof CreatorSettings])));
      const keysLeft = Object.fromEntries(Object.entries(cur.keys).map(([k, v]) => [k, v === keys[k as keyof Keys] ? "" : v])) as Keys;
      set({ saved: next, s: { ...next, ...typed }, keys: keysLeft });
      return true;
    } catch (e) {
      set({ error: t("settings.saveFailedKept") });
      toast.fail({ title: t("settings.saveFailed"), detail: e });
      return false;
    } finally {
      clearTimeout(late);
      set({ saving: false });
    }
  },
  discard: () => set({ s: get().saved, keys: NO_KEYS, error: null }),
}));
const isDirty = (d: { saved: CreatorSettings | null; s: CreatorSettings | null; keys: Keys }) =>
  !same(d.s, d.saved) || Object.values(d.keys).some(Boolean);

/** Shard's settings draft with ours joined in (seam: the page's one
 *  `useSettingsDraft()` call): Shard's own Save bar, Cmd/Ctrl+S and leave
 *  question then cover both, exactly as they cover Shard's draft. Ours is
 *  written first: if it fails, nothing of Shard's is saved and the bar and a
 *  toast say so (Shard's words), so "Settings saved" only ever means all of
 *  it. Shard's save says "Settings saved" when it had changes; else we do. */
export function useCreatorDraft<D extends { dirty: boolean; state: SaveState; error: string | null; save: () => Promise<boolean>; discard: () => void }>(shard: D): D {
  const d = useDraft();
  const latest = useRef(shard);
  useLayoutEffect(() => { latest.current = shard; });
  const save = async () => {
    const shardDirty = shard.dirty;
    const ours = isDirty(useDraft.getState());
    if (ours && !(await useDraft.getState().save())) return false;
    if (!(await shard.save())) {
      // Shard saved nothing with no error of its own (the risky launch
      // arguments were declined): our half is saved, so say so.
      // ponytail: waits one tick for the page to re-render with Shard's error; a store read if Shard ever exports one.
      await new Promise((r) => setTimeout(r, 0));
      if (ours && !latest.current.error) toast.saved(t("settings.saved"));
      return false;
    }
    if (!shardDirty) toast.saved(t("settings.saved"));
    return true;
  };
  return {
    ...shard,
    dirty: shard.dirty || isDirty(d),
    state: d.saving ? "saving" : d.error && shard.state === "idle" ? "error" : shard.state,
    error: shard.error ?? d.error,
    save,
    discard: () => { shard.discard(); d.discard(); },
  };
}

export function CreatorSettingsSection() {
  const ct = useCt();
  const d = useDraft();
  useEffect(() => { useDraft.getState().load(); }, []);
  const { s, keys } = d;
  if (!s) return d.loadError
    ? <LoadError what={ct("creator.settings.what")} error={d.loadError} onRetry={d.load} headingLevel={3} />
    : <LoadingRows rows={4} rowHeight={56} />;
  const set = d.set;
  const setKeys = (k: Keys) => (Object.keys(k) as (keyof Keys)[]).forEach((key) => { if (k[key] !== keys[key]) d.setKey(key, k[key]); });
  const keyRow = (id: keyof Keys, savedFlag: boolean, title: string, help: string) => (
    <SettingRow
      id={id}
      title={<span className="inline-flex items-center gap-2">{title}<Badge size="small" color={savedFlag ? "success" : "gray"}>{savedFlag ? ct("creator.settings.keySet") : ct("creator.settings.keyNotSet")}</Badge></span>}
      help={help}
      control={({ labelId, helpId }) => (
        <div className="w-[240px]">
          <Input
            aria-labelledby={labelId}
            aria-describedby={helpId}
            type="password"
            secret
            leftIcon={<KeyIcon className="size-3.5" />}
            inputSize="xsmall"
            placeholder={savedFlag ? ct("creator.settings.keyReplace") : ct("creator.settings.keyPaste")}
            value={keys[id]}
            onChange={(e) => setKeys({ ...keys, [id]: e.target.value })}
          />
        </div>
      )}
    />
  );

  return (
    <div data-creator-settings className="flex flex-col gap-[32px]">
      <SettingsGroup title={ct("creator.settings.mailTitle")} help={ct("creator.settings.mailHelp")}>
        <SettingRow
          id="ele-url"
          title={ct("creator.settings.eleUrl")}
          control={({ labelId }) => (
            <div className="w-[240px]"><Input aria-labelledby={labelId} technical inputSize="xsmall" value={s.ele_api_url} onChange={(e) => set({ ele_api_url: e.target.value })} /></div>
          )}
        />
        {keyRow("ele_api_key", s.ele_key_saved, ct("creator.settings.eleKey"), ct("creator.settings.eleKeyHelp"))}
        <SettingRow
          id="ele-test"
          title={ct("creator.settings.eleTest")}
          help={ct("creator.settings.eleTestHelp")}
          control={() => <TestButton<{ domains: number }> cmd="test_ele" ok={(r) => ct("creator.settings.eleOk", { n: r.domains })} failed={ct("creator.settings.eleFailed")} />}
        />
      </SettingsGroup>
      {/* Relay, captcha, runs and blocked domains are mock-only until M5-M7;
          the real backend answers the Breeze Blue settings (Ele, proxy rest). */}
      {MOCK && <SettingsGroup title={ct("creator.settings.relayTitle")} help={ct("creator.settings.relayHelp")}>
        <SettingRow
          id="relay-url"
          title={ct("creator.settings.relayUrl")}
          control={({ labelId }) => (
            <div className="w-[240px]"><Input aria-labelledby={labelId} technical inputSize="xsmall" value={s.relay_url} onChange={(e) => set({ relay_url: e.target.value })} /></div>
          )}
        />
        {keyRow("relay_key", s.relay_key_saved, ct("creator.settings.relayKey"), ct("creator.settings.relayKeyHelp"))}
        <SettingRow
          id="relay-test"
          title={ct("creator.settings.relayTest")}
          help={ct("creator.settings.relayTestHelp")}
          control={() => <TestButton<unknown> cmd="test_relay" ok={() => ct("creator.settings.relayOk")} failed={ct("creator.settings.relayFailed")} />}
        />
      </SettingsGroup>}
      {MOCK && <SettingsGroup title={ct("creator.settings.captchaTitle")}>
        {keyRow("yescaptcha_key", s.yescaptcha_key_saved, ct("creator.settings.yescaptcha"), ct("creator.settings.yescaptchaHelp"))}
      </SettingsGroup>}
      {MOCK && <SettingsGroup title={ct("creator.settings.engineTitle")}>
        <SettingRow
          id="max-browsers"
          title={ct("creator.settings.maxBrowsers")}
          help={ct("creator.settings.maxBrowsersHelp")}
          control={({ labelId, helpId }) => <div className="w-[120px]"><NumField aria-labelledby={labelId} aria-describedby={helpId} value={s.max_browsers} min={1} max={20} onChange={(v) => set({ max_browsers: v })} /></div>}
        />
        <SettingRow
          id="fail-streak"
          title={ct("creator.settings.failStreak")}
          help={ct("creator.settings.failStreakHelp")}
          control={({ labelId, helpId }) => <div className="w-[120px]"><NumField aria-labelledby={labelId} aria-describedby={helpId} value={s.fail_streak_stop} min={0} max={50} onChange={(v) => set({ fail_streak_stop: v })} /></div>}
        />
        <SettingRow
          id="otp-timeout"
          title={ct("creator.settings.otpTimeout")}
          control={({ labelId }) => (
            <div className="w-[120px]">
              <SelectField aria-labelledby={labelId} value={s.otp_timeout} onChange={(v) => set({ otp_timeout: v })} options={[60, 90, 120].map((v) => ({ value: v, label: ct("creator.settings.seconds", { n: v }) }))} />
            </div>
          )}
        />
        <SettingRow
          id="delete-failed"
          title={ct("creator.settings.deleteFailed")}
          help={ct("creator.settings.deleteFailedHelp")}
          control={({ labelId, helpId }) => <Switch aria-labelledby={labelId} aria-describedby={helpId} checked={s.delete_failed_browser} onChange={(v) => set({ delete_failed_browser: v })} />}
        />
      </SettingsGroup>}
      <SettingsGroup title={ct("creator.settings.proxyTitle")}>
        <SettingRow
          id="proxy-rest"
          title={ct("creator.settings.proxyRest")}
          help={ct("creator.settings.proxyRestHelp")}
          control={({ labelId, helpId }) => <div className="w-[120px]"><NumField aria-labelledby={labelId} aria-describedby={helpId} value={s.proxy_rest_days} min={0} max={PROXY_REST_MAX_DAYS} onChange={(v) => set({ proxy_rest_days: v })} /></div>}
        />
      </SettingsGroup>
      {MOCK && <SettingsGroup title={ct("creator.settings.blockedTitle")} help={ct("creator.settings.blockedHelp")}>
        <BlockedDomains list={s.blocked_domains} onChange={(blocked_domains) => set({ blocked_domains })} />
      </SettingsGroup>}
      {/* The Python app's proxies, blocked domains and Ele settings (M2: a
          fresh store seeded from a backup copy). The picker is Tauri's. */}
      {!MOCK && (
        <SettingsGroup title={ct("creator.settings.importTitle")}>
          <SettingRow
            id="import"
            title={ct("creator.settings.importRow")}
            help={ct("creator.settings.importHelp")}
            control={() => <ImportButton />}
          />
        </SettingsGroup>
      )}
    </div>
  );
}

/** Import from a ghidorah.sqlite copy, then the saved settings read again
 *  (kept as typed where the draft has changes). */
function ImportButton() {
  const ct = useCt();
  const [busy, setBusy] = useState(false);
  const run = async () => {
    const path = await open({ filters: [{ name: ct("creator.settings.importFilter"), extensions: ["sqlite", "db"] }] });
    if (typeof path !== "string") return;
    setBusy(true);
    try {
      const n = await call<{ proxies: number; blocked_domains: number; settings: number }>("import_basics", { path });
      toast.ok(ct("creator.settings.imported", { proxies: n.proxies, domains: n.blocked_domains, settings: n.settings }));
      const v = await call<CreatorSettings>("settings");
      useDraft.setState((d) => ({ saved: v, s: same(d.s, d.saved) ? v : d.s }));
    } catch (e) {
      toast.fail({ title: ct("creator.settings.importFailed"), detail: e });
    } finally {
      setBusy(false);
    }
  };
  return <Button variant="neutral" mode="stroke" size="xsmall" isLoading={busy} onClick={() => void run()}>{ct("creator.settings.import")}</Button>;
}

/** Test Ele / Test relay: busy while it asks, then Shard's toast with the answer. */
function TestButton<R>({ cmd, ok, failed }: { cmd: string; ok: (r: R) => string; failed: string }) {
  const ct = useCt();
  const [busy, setBusy] = useState(false);
  const run = () => {
    setBusy(true);
    call<R>(cmd)
      .then((r) => toast.ok(ok(r)), (e) => toast.fail({ title: failed, detail: e }))
      .finally(() => setBusy(false));
  };
  return <Button variant="neutral" mode="stroke" size="xsmall" isLoading={busy} onClick={run}>{ct("creator.settings.test")}</Button>;
}

/** Blocked domains: the kit's removable Tag per domain, then a field and Add
 *  (Enter adds too); part of the draft, saved with the bar. */
function BlockedDomains({ list, onChange }: { list: string[]; onChange: (next: string[]) => void }) {
  const ct = useCt();
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const add = () => {
    const d = text.trim().toLowerCase();
    if (!d) { setError(ct("creator.settings.blockedEmpty")); return; }
    if (list.includes(d)) { setError(ct("creator.settings.blockedDupe", { d })); return; }
    onChange([...list, d]);
    setText("");
    setError(null);
  };
  return (
    <SettingRow
      id="blocked-domains"
      title={list.length === 1 ? ct("creator.settings.blockedCountOne") : ct("creator.settings.blockedCount", { n: list.length })}
      control={({ labelId }) => (
        <div className="flex w-[360px] flex-col gap-2">
          {list.length > 0 && (
            <div className="flex flex-wrap gap-1.5">
              {list.map((d) => <Tag key={d} variant="gray" onRemove={() => onChange(list.filter((x) => x !== d))} removeLabel={ct("creator.settings.blockedRemove", { d })}>{d}</Tag>)}
            </div>
          )}
          <div className="flex gap-2">
            <Input aria-labelledby={labelId} technical inputSize="xsmall" placeholder={ct("creator.settings.blockedPlaceholder")} value={text}
              onChange={(e) => { setText(e.target.value); setError(null); }} onKeyDown={(e) => { if (e.key === "Enter") add(); }} />
            <Button variant="neutral" mode="stroke" size="xsmall" onClick={add}>{ct("creator.settings.blockedAdd")}</Button>
          </div>
          {error && <p className="m-0 text-secondary text-error-text">{error}</p>}
        </div>
      )}
    />
  );
}
