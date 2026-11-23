"""Print the stealth bench as one table: python3 summarize.py <out dir>."""

import json
import pathlib
import re
import sys

out = pathlib.Path(sys.argv[1])
CLIENTS = ["baseline", "ours", "patchright"]
R = {c: json.loads((out / f"{c}.json").read_text()) for c in CLIENTS}
rows = []


def strip(s):
    return re.sub(r"<[^>]+>", "", s or "")


def rebrowser(c):
    d = json.loads((R[c].get("rebrowser") or {}).get("rebrowser") or "[]")
    return {x["type"]: f'{x["rating"]:g} {strip(x["note"])[:60]}' for x in d}


rb = {c: rebrowser(c) for c in CLIENTS}
for k in sorted(set().union(*rb.values())):
    rows.append((f"rebrowser {k}", *(rb[c].get(k, "-") for c in CLIENTS)))


def grab(c, suite, pattern):
    m = re.search(pattern, (R[c].get(suite) or {}).get("text", ""))
    return m.group(1).strip() if m else "-"


for c in CLIENTS:
    d = R[c]
    d["_dbi"] = grab(c, "deviceandbrowserinfo", r"Are you a bot\?\s*\S*\s*([^\n]+)")
    d["_dbi_true"] = ",".join(re.findall(r'"(\w+)": true', d["deviceandbrowserinfo"]["text"])) or "none"
    d["_bs"] = grab(c, "browserscan", r"Test Results:\s*([^\n]+)")
    t = d["creepjs"]["text"]
    d["_creep"] = " / ".join(re.findall(r"(\d+% (?:like headless|headless|stealth))", t)[:3])
    d["_creep_worker"] = grab(c, "creepjs", r"\n(\w+WorkerGlobalScope)")
    d["_sanny"] = str(len(d["sannysoft"]["sannysoftFailed"])) + " failed/warn"
    d["_detect"] = d["detect"]["title"]
for label, key in [("deviceandbrowserinfo verdict", "_dbi"), ("deviceandbrowserinfo true flags", "_dbi_true"),
                   ("browserscan verdict", "_bs"), ("creepjs headless/stealth", "_creep"),
                   ("creepjs worker scope", "_creep_worker"), ("sannysoft", "_sanny"), ("detect.html", "_detect")]:
    rows.append((label, *(R[c].get(key, "-") for c in CLIENTS)))
for c in ["ours", "patchright"]:
    i = json.loads(R[c]["interact"]["interact"])
    rows.append((f"interact ({c})", json.dumps({k: v for k, v in i.items() if k != "order"}), "", ""))
    rows.append((f"interact order ({c})", " ".join(i["order"]), "", ""))
print("| check | baseline | ours | patchright |\n|---|---|---|---|")
for r in rows:
    print("| " + " | ".join(str(x).replace("|", "/") for x in r) + " |")
