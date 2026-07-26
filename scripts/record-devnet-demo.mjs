#!/usr/bin/env node
/**
 * Drive real Chrome through Devnet Explorer + local demo UI while ffmpeg records.
 */
import { readFileSync } from "node:fs";
import { spawn } from "node:child_process";
import puppeteer from "puppeteer-core";

const evidence = JSON.parse(readFileSync("/workspace/demos/receive/last-run.json", "utf8"));
const OUT_RAW = "/opt/cursor/artifacts/solana-receive-devnet-raw.mkv";
const OUT_MP4 = "/opt/cursor/artifacts/solana-receive-devnet-demo.mp4";

const scenes = [
  {
    label: "program",
    url: evidence.explorers.program,
    waitText: "Account",
    dwellMs: 3500,
    clickTokens: false,
  },
  {
    label: "before",
    url: evidence.explorers.before,
    waitText: "Success",
    dwellMs: 5500,
    clickTokens: true,
  },
  {
    label: "credited",
    url: evidence.explorers.credited,
    waitText: "Success",
    dwellMs: 5500,
    clickTokens: true,
  },
  {
    label: "held",
    url: evidence.explorers.held,
    waitText: "Success",
    dwellMs: 6000,
    clickTokens: true,
  },
  {
    label: "claim",
    url: evidence.explorers.claim,
    waitText: "Success",
    dwellMs: 5000,
    clickTokens: true,
  },
  {
    label: "expiry",
    url: evidence.explorers.expiry,
    waitText: "Success",
    dwellMs: 5000,
    clickTokens: true,
  },
  {
    label: "ui",
    url: "http://127.0.0.1:8765/",
    waitText: "Successful Devnet evidence",
    dwellMs: 6500,
    clickTokens: false,
  },
];

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

async function waitForText(page, text, timeout = 25000) {
  await page.waitForFunction(
    (t) => document.body && document.body.innerText.includes(t),
    { timeout },
    text,
  );
}

async function clickTab(page, name) {
  const clicked = await page.evaluate((tabName) => {
    const nodes = [...document.querySelectorAll("a,button,div,span")];
    const el = nodes.find((n) => (n.textContent || "").trim() === tabName);
    if (el) {
      el.click();
      return true;
    }
    return false;
  }, name);
  if (clicked) await sleep(900);
  return clicked;
}

async function main() {
  // Launch Chrome visible on :1
  const browser = await puppeteer.launch({
    executablePath: "/usr/local/bin/google-chrome",
    headless: false,
    defaultViewport: { width: 1440, height: 900 },
    args: [
      "--no-first-run",
      "--disable-infobars",
      "--window-size=1440,900",
      "--window-position=40,40",
      "--disable-session-crashed-bubble",
      "--disable-features=TranslateUI",
    ],
    ignoreDefaultArgs: ["--enable-automation"],
  });

  const page = (await browser.pages())[0] || (await browser.newPage());
  await page.setViewport({ width: 1440, height: 900 });
  await page.goto("about:blank");
  await sleep(1500);

  // Start ffmpeg x11grab of the chrome region
  const ff = spawn(
    "ffmpeg",
    [
      "-y",
      "-nostdin",
      "-f",
      "x11grab",
      "-video_size",
      "1440x900",
      "-framerate",
      "30",
      "-i",
      ":1.0+40,40",
      "-c:v",
      "libx264",
      "-preset",
      "veryfast",
      "-pix_fmt",
      "yuv420p",
      "-crf",
      "18",
      OUT_RAW,
    ],
    { stdio: ["ignore", "ignore", "inherit"] },
  );
  await sleep(800);

  for (const scene of scenes) {
    console.log("scene", scene.label, scene.url);
    await page.goto(scene.url, { waitUntil: "domcontentloaded", timeout: 60000 });
    try {
      await waitForText(page, scene.waitText, 30000);
    } catch {
      console.warn("waitText timeout for", scene.label);
    }
    await sleep(1200);
    if (scene.clickTokens) {
      // Prefer Tokens tab (balance deltas / money flow)
      const tok = await clickTab(page, "Tokens");
      if (!tok) await clickTab(page, "Programs & Logs");
      await page.evaluate(() => window.scrollBy(0, 280));
      await sleep(1000);
      await page.evaluate(() => window.scrollBy(0, 320));
      await sleep(800);
    } else if (scene.label === "ui") {
      await page.evaluate(() => window.scrollTo(0, 0));
    } else {
      await page.evaluate(() => window.scrollBy(0, 220));
    }
    await sleep(scene.dwellMs);
  }

  await sleep(500);
  ff.kill("SIGINT");
  await new Promise((r) => ff.on("close", r));
  await browser.close();

  // Speed slightly to land ~50–55s if longer
  const probe = spawn(
    "ffprobe",
    ["-v", "error", "-show_entries", "format=duration", "-of", "default=nw=1:nk=1", OUT_RAW],
    { stdio: ["ignore", "pipe", "inherit"] },
  );
  let durStr = "";
  for await (const chunk of probe.stdout) durStr += chunk;
  await new Promise((r) => probe.on("close", r));
  const dur = parseFloat(durStr.trim());
  const target = 52;
  const speed = dur > target ? dur / target : 1;
  console.log("duration", dur, "speed", speed);

  const args =
    speed > 1.02
      ? [
          "-y",
          "-nostdin",
          "-i",
          OUT_RAW,
          "-filter:v",
          `setpts=PTS/${speed.toFixed(4)}`,
          "-an",
          "-c:v",
          "libx264",
          "-preset",
          "medium",
          "-pix_fmt",
          "yuv420p",
          "-crf",
          "20",
          "-movflags",
          "+faststart",
          OUT_MP4,
        ]
      : [
          "-y",
          "-nostdin",
          "-i",
          OUT_RAW,
          "-an",
          "-c:v",
          "libx264",
          "-preset",
          "medium",
          "-pix_fmt",
          "yuv420p",
          "-crf",
          "20",
          "-movflags",
          "+faststart",
          OUT_MP4,
        ];

  const tx = spawn("ffmpeg", args, { stdio: "inherit" });
  const code = await new Promise((r) => tx.on("close", r));
  if (code !== 0) process.exit(code);
  console.log("WROTE", OUT_MP4);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
