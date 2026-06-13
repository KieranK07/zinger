#!/usr/bin/env node
// Facebook Marketplace sidecar for Nexus.
//
// Spawned on demand by the Rust FB adapter; exits when its task is done. It
// is the ONLY place FB session cookies and an automation-driven browser
// exist — neither touches the main Tauri app/webview. When the adapter is
// disabled this process is never started, so the Playwright/Chromium
// dependency genuinely does not load.
//
// Protocol: newline-delimited JSON on stdout, one event per line. Diagnostics
// go to stderr so they never corrupt the event stream. Subcommands:
//   status                         -> {type:"status", chromiumInstalled}
//   install                        -> {type:"progress",pct} ... {type:"installed"}
//   login   --profile DIR          -> {type:"logged_in"} | {type:"login_timeout"}
//   search  --profile DIR --query Q [--max-pages N --min-delay S --max-delay S]
//                                  -> {type:"page",index,html} ... {type:"done"}
//                                     | {type:"not_logged_in"} | {type:"challenge"}
//
// Anti-detection posture: we rely on the real persistent profile and minimize
// automation flags (drop --enable-automation). A login wall or checkpoint is
// reported and the run stops immediately — the caller never retries.

import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);

function emit(obj) {
  process.stdout.write(JSON.stringify(obj) + "\n");
}
function log(...args) {
  process.stderr.write(args.join(" ") + "\n");
}

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i++) {
    if (argv[i].startsWith("--")) {
      const key = argv[i].slice(2);
      const val = argv[i + 1] && !argv[i + 1].startsWith("--") ? argv[++i] : "true";
      out[key] = val;
    }
  }
  return out;
}

// Realistic, current desktop UA. Kept here so it travels with the profile.
const USER_AGENT =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 " +
  "(KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

function chromiumExecutable() {
  const { chromium } = require("playwright");
  try {
    return chromium.executablePath();
  } catch {
    return null;
  }
}

async function cmdStatus() {
  const exe = chromiumExecutable();
  emit({ type: "status", chromiumInstalled: !!exe && existsSync(exe) });
}

// Runs `playwright install chromium` and streams coarse percentage progress
// parsed from its output. We do not download anything ourselves.
async function cmdInstall() {
  const cliPath = require.resolve("playwright/cli.js");
  const child = spawn(process.execPath, [cliPath, "install", "chromium"], {
    stdio: ["ignore", "pipe", "pipe"],
  });
  let lastPct = -1;
  const scan = (buf) => {
    const text = buf.toString();
    const m = [...text.matchAll(/(\d{1,3})%/g)];
    if (m.length) {
      const pct = Math.min(100, parseInt(m[m.length - 1][1], 10));
      if (pct !== lastPct) {
        lastPct = pct;
        emit({ type: "progress", phase: "download", pct });
      }
    }
  };
  child.stdout.on("data", scan);
  child.stderr.on("data", scan); // playwright prints the progress bar to stderr
  child.on("close", (code) => {
    if (code === 0) emit({ type: "installed" });
    else emit({ type: "error", message: `playwright install exited ${code}` });
  });
}

async function launch(profileDir, headless) {
  const { chromium } = require("playwright");
  return chromium.launchPersistentContext(profileDir, {
    headless,
    userAgent: USER_AGENT,
    viewport: { width: 1280, height: 900 },
    // Drop the banner/flag that most plainly marks an automated session.
    ignoreDefaultArgs: ["--enable-automation"],
    args: ["--disable-blink-features=AutomationControlled"],
  });
}

async function isLoggedIn(ctx) {
  // FB sets the persistent `c_user` cookie (the user id) only when logged in.
  const cookies = await ctx.cookies("https://www.facebook.com");
  return cookies.some((c) => c.name === "c_user" && c.value);
}

function looksLikeChallenge(url, body) {
  return (
    url.includes("/checkpoint/") ||
    url.includes("/captcha/") ||
    /\/recover\//.test(url) ||
    body.includes("Please confirm it's you") ||
    body.includes("temporarily blocked")
  );
}

async function cmdLogin(args) {
  const profile = args.profile;
  if (!profile) return emit({ type: "error", message: "login: --profile required" });
  const ctx = await launch(profile, /* headless */ false);
  const page = ctx.pages()[0] || (await ctx.newPage());
  emit({ type: "login_started" });
  await page.goto("https://www.facebook.com/login", { waitUntil: "domcontentloaded" });

  // Poll for the c_user cookie; the user logs in by hand in the visible window.
  const deadlineMs = Date.now() + 5 * 60 * 1000; // 5 minute window
  while (Date.now() < deadlineMs) {
    if (await isLoggedIn(ctx)) {
      emit({ type: "logged_in" });
      await ctx.close();
      return;
    }
    await page.waitForTimeout(1500);
  }
  emit({ type: "login_timeout" });
  await ctx.close();
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const jitter = (minS, maxS) => (minS + Math.random() * (maxS - minS)) * 1000;

async function cmdSearch(args) {
  const profile = args.profile;
  const query = args.query || "";
  const maxPages = Math.max(1, parseInt(args["max-pages"] || "3", 10));
  const minDelay = parseFloat(args["min-delay"] || "20");
  const maxDelay = parseFloat(args["max-delay"] || "45");
  if (!profile) return emit({ type: "error", message: "search: --profile required" });

  const ctx = await launch(profile, /* headless */ true);
  const page = ctx.pages()[0] || (await ctx.newPage());

  const url = `https://www.facebook.com/marketplace/search?query=${encodeURIComponent(query)}`;
  await page.goto(url, { waitUntil: "domcontentloaded" });
  await sleep(jitter(minDelay, maxDelay));

  const body = await page.content();
  if (looksLikeChallenge(page.url(), body)) {
    emit({ type: "challenge" });
    await ctx.close();
    return;
  }
  if (!(await isLoggedIn(ctx))) {
    emit({ type: "not_logged_in" });
    await ctx.close();
    return;
  }

  // FB Marketplace is infinite-scroll: one "page" == one scroll-and-settle.
  for (let i = 0; i < maxPages; i++) {
    if (i > 0) {
      await page.evaluate(() => window.scrollBy(0, window.innerHeight * 2));
      await sleep(jitter(minDelay, maxDelay));
      const u = page.url();
      const b = await page.content();
      if (looksLikeChallenge(u, b)) {
        emit({ type: "challenge" });
        await ctx.close();
        return;
      }
    }
    const html = await page.content();
    emit({ type: "page", index: i, html });
  }
  emit({ type: "done" });
  await ctx.close();
}

const [, , subcommand, ...rest] = process.argv;
const args = parseArgs(rest);

const commands = {
  status: cmdStatus,
  install: cmdInstall,
  login: cmdLogin,
  search: cmdSearch,
};

(async () => {
  const fn = commands[subcommand];
  if (!fn) {
    emit({ type: "error", message: `unknown subcommand: ${subcommand}` });
    process.exit(2);
  }
  try {
    await fn(args);
  } catch (e) {
    log("sidecar error:", e && e.stack ? e.stack : String(e));
    emit({ type: "error", message: String((e && e.message) || e) });
    process.exit(1);
  }
})();
