// M0 smoke page: list profiles and open example.com in one with CDP.
import { useEffect, useState } from "react";
import { Badge, Button } from "@proxyshard/shardx-ui-kit";
import { PageBody, PageHeader } from "../widgets/PageHeader";
import { call } from "./api";
import { useCt } from "./i18n";

type Row = { id: string; name: string; running: boolean };

export function CreatorPage() {
  const ct = useCt();
  const [rows, setRows] = useState<Row[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [log, setLog] = useState("");

  const reload = () => call<Row[]>("profiles").then(setRows, (e) => setLog(String(e)));
  useEffect(() => { void reload(); }, []);

  const open = async (id: string) => {
    setBusy(id);
    try {
      const r = await call<{ title: string; url: string }>("open_example", { profile_id: id });
      setLog(ct("creator.smoke.opened", { title: r.title, url: r.url }));
    } catch (e) {
      setLog(String(e));
    } finally {
      setBusy(null);
      void reload();
    }
  };

  return (
    <>
      <PageHeader title={ct("creator.nav.accounts")} />
      <PageBody>
        <pre data-creator-log className="whitespace-pre-wrap text-secondary">{log}</pre>
        <ul className="flex flex-col gap-2">
          {rows.map((p) => (
            <li key={p.id} className="flex items-center gap-3">
              <span className="flex-1">{p.name}</span>
              {p.running && <Badge>{ct("creator.smoke.running")}</Badge>}
              <Button size="small" disabled={busy !== null} onClick={() => void open(p.id)}>
                {busy === p.id ? ct("creator.smoke.working") : ct("creator.smoke.open")}
              </Button>
            </li>
          ))}
        </ul>
      </PageBody>
    </>
  );
}
