import { expect, test, type Page } from "@playwright/test";
import { openApp, waitForIdle, type Theme } from "./support/app";

// The MD design gate: every Creator mockup state on mock data, saved as PNG
// for the owner (docs/creator/design). Not a visual baseline: nothing is
// compared, so a change here never fails a run. 1280x800 as Shard's pages.spec.
// Run: E2E_PORT=<free port> npx playwright test e2e/creator-mockups.spec.ts --project=dev
const OUT = "docs/creator/design";

type Opts = { mock?: "empty" | "error" | "setup" | "slow" | "big" | "engine"; lang?: "ru" | "zh" };
async function open(page: Page, section: string, theme: Theme, { mock, lang }: Opts = {}) {
  await page.addInitScript(({ mock, lang }) => {
    if (mock) localStorage.setItem("creator:mock", mock);
    if (lang) localStorage.setItem("shardx.lang", lang);
  }, { mock, lang });
  await openApp(page, { section, theme });
}
const shot = async (page: Page, name: string) => {
  await waitForIdle(page);
  // Our mock answers on a timer the idle wait cannot see.
  await page.waitForTimeout(400);
  await page.screenshot({ path: `${OUT}/${name}.png`, animations: "disabled", caret: "hide" });
};
/** Types in the page header's search (Shard's PageHeader filter). */
async function search(page: Page, q: string) {
  // "/" focuses the page's filter field, as in Shard.
  await page.keyboard.press("/");
  await page.keyboard.type(q);
  // The list reads again for the new search: wait for its answer.
  await page.waitForTimeout(500);
}
const row = (page: Page, id: number) => page.locator(`[data-row-id="${id}"]`);

for (const theme of ["light", "dark"] as const) {
  test.describe(`creator mockups, ${theme}`, () => {
    test("accounts: job running, halted queue", async ({ page }) => {
      await open(page, "c-accounts", theme);
      await expect(page.locator("[data-problem-card]").first()).toBeVisible();
      await shot(page, `accounts-${theme}`);
    });
    test("accounts: peek on a failed account with its log", async ({ page }) => {
      await open(page, "c-accounts", theme);
      await row(page, 1007).click();
      await expect(page.locator("[data-creator-events] li")).toHaveCount(5);
      await shot(page, `accounts-peek-failed-${theme}`);
    });
    test("accounts: peek on a ready account with copy fields", async ({ page }) => {
      await open(page, "c-accounts", theme);
      await row(page, 1009).click();
      await expect(page.locator("aside[data-peek]")).toBeVisible();
      await shot(page, `accounts-peek-linked-${theme}`);
    });
    test("accounts: make dialog", async ({ page }) => {
      await open(page, "c-accounts", theme);
      await page.getByRole("button", { name: "Make accounts" }).click();
      await expect(page.getByRole("dialog")).toBeVisible();
      await shot(page, `accounts-make-${theme}`);
    });
    test("accounts: bulk bar", async ({ page }) => {
      await open(page, "c-accounts", theme);
      for (const id of [1007, 1015, 1018]) await row(page, id).locator("input[type=checkbox]:not([role=switch])").check({ force: true });
      await expect(page.locator("[data-bulk-bar]")).toBeVisible();
      await shot(page, `accounts-bulk-${theme}`);
    });
    test("accounts: Shard's Filters menu, then one filter applied", async ({ page }) => {
      await open(page, "c-accounts", theme);
      await page.getByRole("button", { name: "Filters" }).click();
      await page.getByRole("option", { name: "Status" }).click();
      await shot(page, `accounts-filters-open-${theme}`);
      await page.getByRole("option", { name: /^Failed/ }).click();
      await expect(page.locator("[data-filter-chips]")).toBeVisible();
      await shot(page, `accounts-filtered-${theme}`);
    });
    test("accounts: empty", async ({ page }) => {
      await open(page, "c-accounts", theme, { mock: "empty" });
      await shot(page, `accounts-empty-${theme}`);
    });
    test("accounts: load error", async ({ page }) => {
      await open(page, "c-accounts", theme, { mock: "error" });
      await expect(page.getByRole("button", { name: "Retry" })).toBeVisible();
      await shot(page, `accounts-error-${theme}`);
    });
    test("emails: peek with mail", async ({ page }) => {
      await open(page, "c-emails", theme);
      await row(page, 1).click();
      await expect(page.locator("[data-mail]").first()).toBeVisible();
      await shot(page, `emails-peek-${theme}`);
    });
    test("emails: add emails dialog", async ({ page }) => {
      await open(page, "c-emails", theme);
      await page.getByRole("button", { name: "Add emails" }).click();
      await expect(page.locator("[data-parse-result]")).toBeVisible();
      await shot(page, `emails-add-${theme}`);
    });
    test("platforms: card grid", async ({ page }) => {
      await open(page, "c-platforms", theme);
      await expect(page.locator("[data-platform]").first()).toBeVisible();
      await shot(page, `platforms-${theme}`);
    });
    test("proxies: our page, a copy of Shard's; the failing proxy's card, Filters, Display, Peek", async ({ page }) => {
      await open(page, "c-proxies", theme);
      await expect(row(page, 1)).toBeVisible();
      await expect(page.locator("[data-creator-cards='failing'] [data-problem-card]")).toBeVisible();
      await shot(page, `proxies-${theme}`);
      await page.getByRole("button", { name: "Filters" }).click();
      await page.getByRole("option", { name: "IP type" }).click();
      await shot(page, `proxies-filters-${theme}`);
      await page.keyboard.press("Escape");
      await page.keyboard.press("Escape");
      await page.getByRole("button", { name: "Display" }).click();
      await shot(page, `proxies-display-${theme}`);
      await page.keyboard.press("Escape");
      await row(page, 1).click();
      await expect(page.locator("[data-test-result]")).toBeVisible();
      await shot(page, `proxies-peek-${theme}`);
    });
    test("proxies: Peek of a failing proxy that rests", async ({ page }) => {
      await open(page, "c-proxies", theme);
      await row(page, 6).click();
      await expect(page.locator("[data-test-result]")).toBeVisible();
      await shot(page, `proxies-peek-resting-${theme}`);
    });
    test("proxies: 1,010 selected, Shard's bulk bar", async ({ page }) => {
      // 8 proxies and 1,002 more, all selected.
      await page.addInitScript(() => localStorage.setItem("creator:mock.proxies", "1002"));
      await open(page, "c-proxies", theme);
      await row(page, 1).locator("input[type=checkbox]:not([role=switch])").check({ force: true });
      await expect(page.locator("[data-bulk-bar]")).toBeVisible();
      await page.getByRole("checkbox", { name: "Select all" }).check({ force: true });
      await shot(page, `proxies-bulk-${theme}`);
    });
    test("proxies: Shard's Add proxies flow, paste then preview with tests", async ({ page }) => {
      await open(page, "c-proxies", theme);
      await page.getByRole("button", { name: "Add proxies" }).first().click();
      await expect(page.getByRole("dialog")).toBeVisible();
      await shot(page, `proxies-add-${theme}`);
      await page.getByRole("dialog").locator("textarea").fill("203.0.113.40:8080\n203.0.113.41:8080:user:pass\nsocks5://user:pass@198.51.100.9:1080\nnot a proxy");
      await page.getByRole("button", { name: "Preview" }).click();
      await page.getByRole("button", { name: "Test all" }).click();
      await page.waitForTimeout(1200);
      await shot(page, `proxies-add-preview-${theme}`);
    });
    test("proxies: none yet", async ({ page }) => {
      await open(page, "c-proxies", theme, { mock: "empty" });
      await shot(page, `proxies-empty-${theme}`);
    });
    test("emails: Display menu", async ({ page }) => {
      await open(page, "c-emails", theme);
      await page.getByRole("button", { name: "Display" }).click();
      await shot(page, `emails-display-${theme}`);
    });
    test("settings: leaving with unsaved changes in our tab asks first", async ({ page }) => {
      await open(page, "settings&tab=creator", theme);
      await page.locator("[data-setting='max-browsers'] input").fill("5");
      await expect(page.locator("[data-save-bar]")).toBeVisible();
      await page.getByRole("link", { name: "Accounts" }).or(page.getByRole("button", { name: "Accounts" })).first().click();
      await expect(page.getByRole("alertdialog").or(page.getByRole("dialog"))).toBeVisible();
      await shot(page, `settings-leave-${theme}`);
    });
    test("accounts: Display menu", async ({ page }) => {
      await open(page, "c-accounts", theme);
      await page.getByRole("button", { name: "Display" }).click();
      await shot(page, `accounts-display-${theme}`);
    });
    test("settings: our tab failed to load: Shard's load error with Retry", async ({ page }) => {
      await open(page, "settings&tab=creator", theme, { mock: "error" });
      await expect(page.getByRole("button", { name: "Retry" })).toBeVisible();
      await shot(page, `settings-error-${theme}`);
    });
    test("settings: our Creator tab", async ({ page }) => {
      await open(page, "settings&tab=creator", theme);
      await expect(page.locator("[data-creator-settings]")).toBeVisible();
      await shot(page, `settings-${theme}`);
      // One change: Shard's Save bar comes up.
      await page.locator("[data-setting='max-browsers'] input").fill("5");
      await expect(page.locator("[data-save-bar]")).toBeVisible();
      await shot(page, `settings-unsaved-${theme}`);
    });
  });

  test.describe(`creator states, ${theme}`, () => {
    test("accounts: first run, Problems to fix says what is missing", async ({ page }) => {
      await open(page, "c-accounts", theme, { mock: "setup" });
      await expect(page.locator("[data-problem-card]").first()).toBeVisible();
      await shot(page, `accounts-setup-${theme}`);
      await page.getByRole("button", { name: "Make accounts" }).first().click();
      await expect(page.getByRole("dialog")).toBeVisible();
      await shot(page, `accounts-make-blocked-${theme}`);
    });
    test("accounts: Shard is updating the engine: the job waits, Make says why", async ({ page }) => {
      await open(page, "c-accounts", theme, { mock: "engine" });
      await page.getByRole("button", { name: "Make accounts" }).first().click();
      await expect(page.getByRole("dialog")).toBeVisible();
      await page.waitForTimeout(300);
      await shot(page, `accounts-engine-update-${theme}`);
    });
    test("accounts: failed Peek's last screenshot, large", async ({ page }) => {
      await open(page, "c-accounts", theme);
      await row(page, 1007).click();
      await page.getByRole("button", { name: "Show the last screenshot larger" }).click();
      await expect(page.getByRole("dialog")).toBeVisible();
      await shot(page, `accounts-screenshot-${theme}`);
    });
    test("emails: older mail loads as the Peek scrolls", async ({ page }) => {
      await open(page, "c-emails", theme);
      await row(page, 1).click();
      await expect(page.locator("[data-mail]").first()).toBeVisible();
      await page.locator("[data-mail]").last().scrollIntoViewIfNeeded();
      await page.waitForTimeout(400);
      await page.locator("[data-mail]").last().scrollIntoViewIfNeeded();
      await shot(page, `emails-peek-older-${theme}`);
    });
    test("emails: Free for a platform offers Make N", async ({ page }) => {
      await open(page, "c-emails", theme);
      await page.getByRole("button", { name: "Filters" }).click();
      await page.getByRole("option", { name: "Free for" }).click();
      await page.getByRole("option", { name: /^Qwen/ }).click();
      await page.waitForTimeout(300);
      await shot(page, `emails-free-for-${theme}`);
    });
    test("accounts: make, a route that needs the YesCaptcha key", async ({ page }) => {
      await page.addInitScript(() => localStorage.setItem("creator:make", JSON.stringify({ k: "elevenlabs" })));
      await open(page, "c-accounts", theme);
      await page.getByRole("button", { name: "Make accounts" }).click();
      await page.getByRole("radio", { name: "Temp mail (Ele)" }).check({ force: true });
      await shot(page, `accounts-make-captcha-${theme}`);
    });
    // Rows far down the list are not drawn yet (virtual list): found by search.
    for (const [id, name, q] of [[1021, "signedout", "ngoc.dang"], [1004, "frozen", "alice"], [1003, "running", "alice"], [1040, "proxyfail", "nam.ly"], [1041, "exists", "mai.quach"]] as const) {
      test(`accounts: peek, ${name}`, async ({ page }) => {
        await open(page, "c-accounts", theme);
        await search(page, q);
        await row(page, id).click();
        await expect(page.locator("aside[data-peek]")).toBeVisible();
        await shot(page, `accounts-peek-${name}-${theme}`);
      });
    }
    test("accounts: delete asks first (Shard's confirm)", async ({ page }) => {
      await open(page, "c-accounts", theme);
      for (const id of [1007, 1015]) await row(page, id).locator("input[type=checkbox]:not([role=switch])").check({ force: true });
      await page.locator("[data-bulk-bar]").getByRole("button", { name: "More actions" }).click();
      await page.getByRole("menuitem", { name: "Delete" }).click();
      await expect(page.getByRole("dialog").or(page.getByRole("alertdialog"))).toBeVisible();
      await shot(page, `accounts-delete-confirm-${theme}`);
    });
    test("accounts: a done action's toast", async ({ page }) => {
      await open(page, "c-accounts", theme);
      for (const id of [1007, 1015, 1018]) await row(page, id).locator("input[type=checkbox]:not([role=switch])").check({ force: true });
      await page.locator("[data-bulk-bar]").getByRole("button", { name: "Retry" }).click();
      await page.waitForTimeout(300);
      await shot(page, `accounts-toast-${theme}`);
    });
    test("accounts: search finds nothing", async ({ page }) => {
      await open(page, "c-accounts", theme);
      await search(page, "zzz-nobody");
      await expect(page.getByText("No accounts match")).toBeVisible();
      await shot(page, `accounts-nomatch-${theme}`);
    });
    test("accounts: first load (Shard's placeholder rows)", async ({ page }) => {
      await open(page, "c-accounts", theme, { mock: "slow" });
      await page.waitForTimeout(900);
      await page.screenshot({ path: `${OUT}/accounts-loading-${theme}.png`, animations: "disabled", caret: "hide" });
    });
    test("accounts: huge counts", async ({ page }) => {
      await open(page, "c-accounts", theme, { mock: "big" });
      await shot(page, `accounts-big-${theme}`);
    });
    test("emails: a gone mailbox in Peek", async ({ page }) => {
      await open(page, "c-emails", theme);
      await search(page, "vy.ta");
      await row(page, 12).click();
      await expect(page.locator("[data-mailbox-gone]")).toBeVisible();
      await shot(page, `emails-peek-gone-${theme}`);
    });
    test("emails: check result toast, delete asks first", async ({ page }) => {
      await open(page, "c-emails", theme);
      for (const id of [1, 2, 3, 4]) await row(page, id).locator("input[type=checkbox]").check({ force: true });
      await page.locator("[data-bulk-bar]").getByRole("button", { name: "Test", exact: true }).click();
      await page.waitForTimeout(400);
      await shot(page, `emails-test-toast-${theme}`);
      await page.locator("[data-bulk-bar]").getByRole("button", { name: "More actions" }).click();
      await page.getByRole("menuitem", { name: "Delete" }).click();
      await shot(page, `emails-delete-confirm-${theme}`);
    });
    test("emails: add Outlook combos with lines it can't read", async ({ page }) => {
      await open(page, "c-emails", theme);
      await page.getByRole("button", { name: "Add emails" }).first().click();
      await page.getByRole("dialog").locator("textarea").fill("a.b@outlook.example|pw|tok|cid|rec\nc.d@outlook.example|pw|tok|cid|rec\nnot a combo\ne.f@outlook.example|pw");
      await page.waitForTimeout(400);
      await shot(page, `emails-add-report-${theme}`);
    });
    test("platforms: Edit dialog", async ({ page }) => {
      await open(page, "c-platforms", theme);
      await page.locator("[data-platform='chatgpt']").click({ button: "right" });
      await page.getByRole("menuitem", { name: "Edit" }).click();
      await expect(page.getByRole("dialog")).toBeVisible();
      await shot(page, `platforms-edit-${theme}`);
    });
    test("proxies: delete asks first; a paste with nothing to read", async ({ page }) => {
      await open(page, "c-proxies", theme);
      await row(page, 6).locator("input[type=checkbox]:not([role=switch])").check({ force: true });
      await page.locator("[data-bulk-bar]").getByRole("button", { name: "Delete" }).click();
      await shot(page, `proxies-delete-confirm-${theme}`);
      await page.keyboard.press("Escape");
      await page.getByRole("button", { name: "Add proxies" }).first().click();
      await page.getByRole("button", { name: "Preview" }).click();
      await page.waitForTimeout(300);
      await shot(page, `proxies-add-error-${theme}`);
    });
    test("settings: Test Ele answers, a save the backend refuses", async ({ page }) => {
      await open(page, "settings&tab=creator", theme);
      await page.locator("[data-setting='ele-test']").getByRole("button", { name: "Test" }).click();
      await page.waitForTimeout(400);
      await shot(page, `settings-ele-ok-${theme}`);
      await page.locator("[data-setting='ele-url'] input").fill("ele.example/api");
      await page.locator("[data-save-bar]").getByRole("button", { name: "Save", exact: true }).last().click();
      await page.waitForTimeout(400);
      await shot(page, `settings-save-error-${theme}`);
    });
    test("settings: first run, Test Ele fails", async ({ page }) => {
      await open(page, "settings&tab=creator", theme, { mock: "setup" });
      await page.locator("[data-setting='ele-test']").getByRole("button", { name: "Test" }).click();
      await page.waitForTimeout(400);
      await shot(page, `settings-ele-fail-${theme}`);
    });
    test("settings: relay, the sign-up rows, blocked domains", async ({ page }) => {
      await open(page, "settings&tab=creator", theme);
      await page.locator("[data-setting='relay-test']").getByRole("button", { name: "Test" }).click();
      await page.locator("[data-setting='delete-failed']").scrollIntoViewIfNeeded();
      await page.waitForTimeout(300);
      await shot(page, `settings-relay-signup-${theme}`);
      const blocked = page.locator("[data-setting='blocked-domains']");
      await blocked.scrollIntoViewIfNeeded();
      await blocked.locator("input").fill("tempbox.example");
      await blocked.getByRole("button", { name: "Add" }).click();
      await shot(page, `settings-blocked-${theme}`);
    });
    test("proxies: Edit proxy", async ({ page }) => {
      await open(page, "c-proxies", theme);
      await row(page, 1).click();
      await page.locator("[data-peek-edit]").click();
      await expect(page.getByRole("dialog")).toBeVisible();
      await shot(page, `proxies-edit-${theme}`);
    });
    test("proxies: first load", async ({ page }) => {
      await open(page, "c-proxies", theme, { mock: "slow" });
      await page.waitForTimeout(900);
      await page.screenshot({ path: `${OUT}/proxies-loading-${theme}.png`, animations: "disabled", caret: "hide" });
    });
    test("proxies: load error", async ({ page }) => {
      await open(page, "c-proxies", theme, { mock: "error" });
      await expect(page.getByRole("button", { name: "Retry" })).toBeVisible();
      await shot(page, `proxies-error-${theme}`);
    });
    test("proxies: search finds nothing", async ({ page }) => {
      await open(page, "c-proxies", theme);
      await search(page, "zzz-none");
      await shot(page, `proxies-nomatch-${theme}`);
    });
    test("accounts: More in the bulk bar (Log in again, Change proxy, three exports)", async ({ page }) => {
      await open(page, "c-accounts", theme);
      for (const id of [1000, 1001, 1002]) await row(page, id).locator("input[type=checkbox]:not([role=switch])").check({ force: true });
      await page.locator("[data-bulk-bar]").getByRole("button", { name: "More actions" }).click();
      await shot(page, `accounts-bulk-more-${theme}`);
      await page.getByRole("menuitem", { name: "Export cookies (JSON)" }).click();
      await page.waitForTimeout(300);
      await shot(page, `accounts-export-toast-${theme}`);
      await page.locator("[data-bulk-bar]").getByRole("button", { name: "More actions" }).click();
      await page.getByRole("menuitem", { name: "Change proxy" }).click();
      await expect(page.getByRole("dialog")).toBeVisible();
      await page.waitForTimeout(300);
      await shot(page, `accounts-change-proxy-${theme}`);
    });
    test("accounts: Unfreeze a pushed account asks first; Close all browsers asks first", async ({ page }) => {
      await open(page, "c-accounts", theme);
      await row(page, 1004).click();
      await page.locator("aside[data-peek]").getByRole("button", { name: "More actions" }).click();
      await page.getByRole("menuitem", { name: "Unfreeze" }).click();
      await shot(page, `accounts-unfreeze-confirm-${theme}`);
      await page.keyboard.press("Escape");
      await page.keyboard.press("Escape");
      await page.getByRole("button", { name: "More actions" }).first().click();
      await page.getByRole("menuitem", { name: "Close all browsers" }).click();
      await shot(page, `accounts-close-all-confirm-${theme}`);
    });
    test("accounts: the row's hover action (Copy login on a ready account)", async ({ page }) => {
      await open(page, "c-accounts", theme);
      await row(page, 1000).hover();
      await page.waitForTimeout(200);
      await shot(page, `accounts-row-action-${theme}`);
    });
    test("proxies: Test in the bar, Shard's summary toast", async ({ page }) => {
      await open(page, "c-proxies", theme);
      for (const id of [1, 2, 6]) await row(page, id).locator("input[type=checkbox]:not([role=switch])").check({ force: true });
      await page.locator("[data-bulk-bar]").getByRole("button", { name: "Test", exact: true }).click();
      await page.waitForTimeout(1200);
      await shot(page, `proxies-test-toast-${theme}`);
    });
    test("palette: our pages and Settings: Creator", async ({ page }) => {
      await open(page, "c-accounts", theme);
      await page.keyboard.press("ControlOrMeta+k");
      await page.keyboard.type("creator");
      await page.waitForTimeout(300);
      await shot(page, `palette-${theme}`);
    });
  });

}

for (const theme of ["light", "dark"] as const) {
  test(`accounts in Russian, ${theme}`, async ({ page }) => {
    await open(page, "c-accounts", theme, { lang: "ru" });
    await shot(page, `accounts-ru-${theme}`);
  });
}

test("accounts at the compact width", async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 680 });
  await open(page, "c-accounts", "light");
  await shot(page, "accounts-compact-light");
});
