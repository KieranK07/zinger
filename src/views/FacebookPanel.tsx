import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../lib/api";
import type { FbProgress, FbStatus } from "../lib/types";

// One-time warning the user must explicitly accept before FB can be enabled.
function TosDialog({ onAccept, onCancel }: { onAccept: () => void; onCancel: () => void }) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-6">
      <div className="max-w-lg rounded-lg border border-zinc-700 bg-zinc-900 p-5">
        <h3 className="mb-2 text-base font-semibold text-amber-300">
          Facebook Marketplace — read before enabling
        </h3>
        <ul className="mb-4 list-disc space-y-1 pl-5 text-sm text-zinc-300">
          <li>Facebook has <strong>no public API</strong> for Marketplace.</li>
          <li>
            Automated access <strong>violates Facebook's Terms of Service</strong>. This uses
            <strong> your own account</strong>, logged in by you, entirely at your own risk.
          </li>
          <li>
            Nexus minimizes automation signals and rate-limits hard, but this{" "}
            <strong>lowers, not eliminates</strong>, the risk of detection or account
            restriction.
          </li>
          <li>
            Enabling downloads a private browser (~341 MB). Your session stays on this machine.
          </li>
        </ul>
        <div className="flex justify-end gap-2">
          <button
            onClick={onCancel}
            className="rounded bg-zinc-800 px-4 py-1.5 text-sm text-zinc-300 hover:bg-zinc-700"
          >
            Cancel
          </button>
          <button
            onClick={onAccept}
            className="rounded bg-amber-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-amber-500"
          >
            I understand and accept the risk
          </button>
        </div>
      </div>
    </div>
  );
}

export function FacebookPanel() {
  const [status, setStatus] = useState<FbStatus | null>(null);
  const [showTos, setShowTos] = useState(false);
  const [progress, setProgress] = useState<FbProgress | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const refresh = useCallback(() => {
    api.fbStatus().then(setStatus);
  }, []);

  useEffect(() => {
    refresh();
    const unlisten = listen<FbProgress>("fb-progress", (e) => {
      setProgress(e.payload);
      if (e.payload.kind === "done" || e.payload.kind === "error") refresh();
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [refresh]);

  if (!status) return null;

  const enable = async () => {
    if (!status.tos_accepted) {
      setShowTos(true);
      return;
    }
    await api.fbSetEnabled(true);
    refresh();
  };

  const acceptTos = async () => {
    await api.fbAcceptTos();
    setShowTos(false);
    await api.fbSetEnabled(true);
    refresh();
  };

  const disable = async () => {
    await api.fbSetEnabled(false);
    refresh();
  };

  const download = async () => {
    setBusy("download");
    setMessage(null);
    setProgress({ kind: "download", pct: 0, message: null });
    try {
      await api.fbInstallChromium();
      setMessage("Chromium installed.");
    } catch (e) {
      setMessage(`Download failed: ${e}. Check your connection and retry.`);
    } finally {
      setBusy(null);
      setProgress(null);
      refresh();
    }
  };

  const login = async () => {
    setBusy("login");
    setMessage("A browser window will open — log in to Facebook there.");
    try {
      await api.fbLogin();
      setMessage("Logged in.");
    } catch (e) {
      setMessage(`Login: ${e}`);
    } finally {
      setBusy(null);
      refresh();
    }
  };

  const needsLogin = status.session_expired || !status.logged_in;
  const disabledUntil = status.disabled_until
    ? new Date(status.disabled_until).toLocaleString()
    : null;

  return (
    <section className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-4">
      {showTos && <TosDialog onAccept={acceptTos} onCancel={() => setShowTos(false)} />}
      <div className="mb-1 flex items-center justify-between">
        <h2 className="text-sm font-semibold text-zinc-300">
          Facebook Marketplace <span className="text-xs font-normal text-amber-400">experimental</span>
        </h2>
        {status.enabled ? (
          <button
            onClick={disable}
            className="rounded bg-zinc-800 px-3 py-1 text-xs text-zinc-300 hover:bg-zinc-700"
          >
            Disable
          </button>
        ) : (
          <button
            onClick={enable}
            className="rounded bg-amber-600 px-3 py-1 text-xs font-medium text-white hover:bg-amber-500"
          >
            Enable
          </button>
        )}
      </div>
      <p className="mb-3 text-xs text-zinc-500">
        Off by default. Uses your own logged-in session via a private, isolated browser. Violates
        Facebook's ToS — your account, your risk. See the README.
      </p>

      {status.enabled && (
        <div className="flex flex-col gap-3">
          {/* Step 1: Chromium */}
          {!status.chromium_installed && (
            <div>
              <button
                onClick={download}
                disabled={busy === "download"}
                className="rounded bg-emerald-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-emerald-500 disabled:opacity-50"
              >
                {busy === "download" ? "Downloading…" : "Download browser (~341 MB)"}
              </button>
              {progress && progress.kind === "download" && (
                <div className="mt-2">
                  <div className="h-2 w-full overflow-hidden rounded bg-zinc-800">
                    <div
                      className="h-full bg-emerald-500 transition-all"
                      style={{ width: `${progress.pct ?? 0}%` }}
                    />
                  </div>
                  <div className="mt-1 text-xs text-zinc-500">{progress.pct ?? 0}%</div>
                </div>
              )}
            </div>
          )}

          {/* Step 2: login / re-login */}
          {status.chromium_installed && needsLogin && (
            <button
              onClick={login}
              disabled={busy === "login"}
              className="self-start rounded bg-emerald-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-emerald-500 disabled:opacity-50"
            >
              {busy === "login"
                ? "Waiting for login…"
                : status.session_expired
                  ? "Re-login required — open Facebook"
                  : "Log in to Facebook"}
            </button>
          )}

          {/* Ready state + page cap */}
          {status.chromium_installed && !needsLogin && !disabledUntil && (
            <div className="text-sm text-emerald-400">Connected and ready.</div>
          )}
          {disabledUntil && (
            <div className="text-sm text-amber-400">
              Auto-disabled after repeated challenges until {disabledUntil}.
            </div>
          )}

          {status.chromium_installed && (
            <label className="flex items-center gap-2 text-xs text-zinc-400">
              Max pages per poll
              <input
                type="number"
                min={1}
                max={10}
                defaultValue={status.max_pages}
                onBlur={(e) => api.fbSetMaxPages(Math.max(1, Number(e.target.value) || 3))}
                className="w-16 rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-sm"
              />
            </label>
          )}
        </div>
      )}

      {message && <div className="mt-2 text-xs text-zinc-400">{message}</div>}
    </section>
  );
}
