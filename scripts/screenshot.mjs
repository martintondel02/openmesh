// Screenshot the OpenMesh web client for README/marketing use.
// Usage: node scripts/screenshot.mjs http://127.0.0.1:5173 apps/web/public/screenshots
import { chromium } from "playwright";
import { mkdirSync } from "node:fs";
import { resolve } from "node:path";

const url = process.argv[2] ?? "http://127.0.0.1:5173/?demo=1";
const outDir = resolve(process.argv[3] ?? "apps/web/public/screenshots");
mkdirSync(outDir, { recursive: true });

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 });
await page.goto(url, { waitUntil: "networkidle", timeout: 60_000 });
await page.waitForTimeout(2500);

const shots = [
  { name: "chats", click: "Chats" },
  { name: "map", click: "Map" },
  { name: "nodes", click: "Nodes" },
  { name: "settings", click: "Settings" },
];
for (const { name, click } of shots) {
  try {
    const tab = page.getByRole("link", { name: click }).or(page.getByRole("button", { name: click })).first();
    await tab.click({ timeout: 5000 });
  } catch {
    try { await page.getByText(click, { exact: true }).first().click({ timeout: 3000 }); } catch { /* tab may be visible already */ }
  }
  await page.waitForTimeout(1200);
  await page.screenshot({ path: `${outDir}/${name}.png`, fullPage: false });
  console.log("saved", name);
}

await page.setViewportSize({ width: 414, height: 896 });
await page.waitForTimeout(600);
await page.screenshot({ path: `${outDir}/mobile.png` });
await browser.close();
console.log("screenshots written to", outDir);
