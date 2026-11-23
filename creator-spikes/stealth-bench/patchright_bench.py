"""Stealth bench, patchright side (the production Python stack).

Same suites, waits and rebrowser actions as `stealth_bench` in
src-tauri/src/creator/web/probe.rs, on the same running profile.

    cd <ghidorah-creator> && uv run python <this file> <ws url> <out dir>

Writes <out>/patchright.json and patchright-<suite>.png.
"""

import asyncio
import functools
import http.server
import json
import pathlib
import sys
import threading

from patchright.async_api import async_playwright

FIXTURES = pathlib.Path(__file__).resolve().parents[2] / "src-tauri/src/creator/web/fixtures"
EXTRACT = (FIXTURES / "bench_extract.js").read_text()
SUITES = [
    ("rebrowser", "https://bot-detector.rebrowser.net/", 4),
    ("deviceandbrowserinfo", "https://deviceandbrowserinfo.com/are_you_a_bot", 12),
    ("creepjs", "https://abrahamjuliot.github.io/creepjs/", 25),
    ("browserscan", "https://www.browserscan.net/bot-detection", 15),
    ("sannysoft", "https://bot.sannysoft.com", 8),
]
RB_MAIN = ["() => window.dummyFn()", "() => !!document.getElementById('detections-json')"]
RB_ISOLATED = "() => document.getElementsByClassName('div').length"


def serve_fixtures() -> str:
    handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(FIXTURES))
    httpd = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    return f"http://127.0.0.1:{httpd.server_address[1]}"


async def main(ws: str, out: pathlib.Path) -> None:
    out.mkdir(parents=True, exist_ok=True)
    site = serve_fixtures()
    suites = SUITES + [("detect", site + "/detect.html", 2), ("interact", site + "/interact.html", 1)]
    results = {}
    async with async_playwright() as p:
        browser = await p.chromium.connect_over_cdp(ws)
        page = await browser.contexts[0].new_page()
        for name, url, wait in suites:
            await page.goto(url, wait_until="domcontentloaded", timeout=60_000)
            await asyncio.sleep(wait)
            if name == "rebrowser":
                for js in RB_MAIN:
                    try:
                        await page.evaluate(js, isolated_context=False)
                    except Exception:
                        pass
                await page.evaluate(RB_ISOLATED)
                await asyncio.sleep(2)
            if name == "interact":
                # As the production flows do: locator click / fill.
                await page.locator("#a").click()
                await page.locator("#name").fill("hello")
                await page.locator("#b").click()
            results[name] = await page.evaluate(EXTRACT)
            await page.screenshot(path=str(out / f"patchright-{name}.png"))
        await page.close()
        await browser.close()
    (out / "patchright.json").write_text(json.dumps(results, indent=2))


if __name__ == "__main__":
    asyncio.run(main(sys.argv[1], pathlib.Path(sys.argv[2])))
