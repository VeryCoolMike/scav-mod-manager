import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { api, errMessage } from "../lib/api";
import type { BepInExStatus, DetectedGame } from "../lib/types";
import { Button, Spinner, useToast } from "../lib/ui";
import { useApp } from "../App";

interface LogEntry {
  step: string;
  detail: string;
  level: string;
  ts: string;
}

export default function Setup() {
  const { settings, reloadSettings } = useApp();
  const toast = useToast();
  const [detecting, setDetecting] = useState(false);
  const [candidates, setCandidates] = useState<DetectedGame[]>([]);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [bepinex, setBepinex] = useState<BepInExStatus | null>(null);
  const [installing, setInstalling] = useState(false);
  const [loggingIn, setLoggingIn] = useState(false);
  const [autoCreating, setAutoCreating] = useState(false);
  const [autoLogs, setAutoLogs] = useState<LogEntry[]>([]);

  useEffect(() => {
    const unlisten = listen<LogEntry>("auto-register-log", (event) => {
      setAutoLogs((prev) => [...prev.slice(-19), event.payload]);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const gameSet = !!settings.game_path;
  const loggedIn = !!settings.nexus_api_key;

  useEffect(() => {
    if (gameSet) api.bepinexStatus().then(setBepinex).catch(() => {});
  }, [gameSet, settings.game_path]);

  async function autodetect() {
    setDetecting(true);
    setCandidates([]);
    try {
      const found = await api.detectGames();
      if (found.length > 0) {
        setCandidates(found);
      } else {
        toast.push("info", "No install auto-detected — pick the folder manually.");
      }
    } catch (e) {
      toast.push("error", errMessage(e));
    } finally {
      setDetecting(false);
    }
  }

  async function acceptCandidate(c: DetectedGame) {
    setConfirming(c.path);
    try {
      await api.setGamePath(c.path, c.source, c.steam_appid);
      await reloadSettings();
      setCandidates([]);
      toast.push("success", `Using ${c.path}`);
    } catch (e) {
      toast.push("error", errMessage(e));
    } finally {
      setConfirming(null);
    }
  }

  function rejectCandidate(c: DetectedGame) {
    setCandidates((prev) => prev.filter((x) => x.path !== c.path));
  }

  async function browse() {
    const dir = await open({ directory: true, title: "Select the Casualties: Unknown folder" });
    if (typeof dir === "string") {
      try {
        await api.setGamePath(dir, "manual");
        await reloadSettings();
        toast.push("success", "Game folder set.");
      } catch (e) {
        toast.push("error", errMessage(e));
      }
    }
  }

  async function installBepinex() {
    setInstalling(true);
    try {
      const st = await api.bepinexInstall();
      setBepinex(st);
      toast.push("success", `BepInEx ${st.version ?? ""} installed.`);
    } catch (e) {
      toast.push("error", errMessage(e));
    } finally {
      setInstalling(false);
    }
  }

  async function login() {
    setLoggingIn(true);
    try {
      toast.push("info", "Opening Nexus in your browser — click Authorize.");
      const res = await api.nexusSsoLogin();
      await reloadSettings();
      toast.push(res.valid ? "success" : "error", res.valid ? `Signed in as ${res.name}` : "Login failed");
    } catch (e) {
      toast.push("error", errMessage(e));
    } finally {
      setLoggingIn(false);
    }
  }

  async function autoCreate() {
    setAutoCreating(true);
    setAutoLogs([]);
    try {
      toast.push("info", "Creating account — a browser window will open briefly…");
      const res = await api.autoFullRegister();
      await reloadSettings();
      if (res.valid) {
        toast.push("success", `Account created! Signed in as ${res.name}.`);
      } else {
        toast.push("error", "Account creation failed. Try manual login above.");
      }
    } catch (e) {
      toast.push("error", errMessage(e));
    } finally {
      setAutoCreating(false);
    }
  }

  async function finish() {
    await api.saveSettings({ ...settings, setup_complete: true });
    await reloadSettings();
  }

  return (
    <div className="mx-auto flex h-full max-w-2xl flex-col justify-center gap-6 p-8">
      <div>
        <h1 className="text-2xl font-bold text-green-400">Welcome to Scav Mod Manager</h1>
        <p className="text-sm text-neutral-400">
          Two quick steps and you're modding — no account or sign-in required.
        </p>
      </div>

      {/* Step 1 — game */}
      <Step n={1} title="Locate the game" done={gameSet}>
        {gameSet && <div className="mb-2 break-all text-xs text-green-300">{settings.game_path}</div>}
        <div className="flex gap-2">
          <Button variant="primary" disabled={detecting} onClick={autodetect}>
            {detecting ? <Spinner /> : "🔍"} Auto-detect
          </Button>
          <Button onClick={browse}>📁 Choose folder…</Button>
        </div>

        {candidates.length > 0 && (
          <div className="mt-3 flex flex-col gap-2">
            <p className="text-xs text-neutral-400">
              Found {candidates.length} possible install{candidates.length > 1 ? "s" : ""} — confirm
              the right one:
            </p>
            {candidates.map((c) => (
              <div
                key={c.path}
                className="flex items-center gap-3 rounded-md border border-neutral-800 bg-neutral-900/60 p-2"
              >
                <div className="min-w-0 flex-1">
                  <div className="break-all text-xs text-neutral-200">{c.path}</div>
                  <div className="mt-0.5 text-[11px] text-neutral-500">
                    {c.source}
                    {c.version ? ` · ${c.version}` : ""}
                  </div>
                </div>
                <Button
                  variant="primary"
                  disabled={confirming === c.path}
                  onClick={() => acceptCandidate(c)}
                >
                  {confirming === c.path ? <Spinner /> : "✓"} Yes
                </Button>
                <Button variant="ghost" onClick={() => rejectCandidate(c)}>
                  ✕ No
                </Button>
              </div>
            ))}
          </div>
        )}
      </Step>

      {/* Step 2 — bepinex */}
      <Step n={2} title="Install the mod loader (BepInEx)" done={!!bepinex?.installed} disabled={!gameSet}>
        {bepinex?.installed ? (
          <div className="text-xs text-green-300">
            Installed{bepinex.version ? ` (v${bepinex.version})` : ""}.
          </div>
        ) : (
          <Button variant="primary" disabled={!gameSet || installing} onClick={installBepinex}>
            {installing ? <Spinner /> : "⬇"} Install BepInEx
          </Button>
        )}
        {bepinex?.needs_proton_setup && bepinex.proton_launch_option && (
          <div className="mt-3 rounded-md border border-amber-800 bg-amber-950/40 p-3 text-xs text-amber-200">
            <b>Linux / Proton:</b> set this once in Steam → right-click the game → Properties →
            Launch Options:
            <code className="mt-1 block rounded bg-black/40 px-2 py-1 text-amber-100">
              {bepinex.proton_launch_option}
            </code>
          </div>
        )}
      </Step>

      {/* Step 3 — nexus login (SSO) */}
      <Step n={3} title="Connect Nexus (one click)" done={loggedIn} disabled={!gameSet}>
        {loggedIn ? (
          <div className="text-xs text-green-300">
            Signed in as {settings.nexus_user}
            {settings.is_premium ? " · Premium ★" : " · Free"}.
          </div>
        ) : (
          <>
            <p className="mb-2 text-xs text-neutral-400">
              Mods are hosted on Nexus. Click to log in — a browser tab opens, you hit{" "}
              <b>Authorize</b> once, and you're done. No keys to copy.
            </p>
            <Button variant="primary" disabled={!gameSet || loggingIn} onClick={login}>
              {loggingIn ? <Spinner /> : "🔗"} Login with Nexus
            </Button>
            <div className="mt-3 border-t border-neutral-800 pt-3">
              <p className="mb-2 text-xs text-neutral-500">
                No account yet? We'll create one fully automatically — just press the button.
              </p>
              {autoCreating ? (
                <div className="w-full rounded-md border border-amber-800 bg-amber-950/20 text-xs">
                  <div className="flex items-center gap-2 border-b border-amber-900/50 px-3 py-2 text-amber-300">
                    <Spinner /> Creating and authorizing account…
                  </div>
                  <div className="max-h-40 overflow-y-auto p-2 font-mono text-[11px] leading-relaxed">
                    {autoLogs.length === 0 && (
                      <div className="text-neutral-500 italic">Starting…</div>
                    )}
                    {autoLogs.map((l, i) => (
                      <div
                        key={i}
                        className={l.level === "error" ? "text-red-400" : "text-neutral-400"}
                      >
                        <span className="text-neutral-600">[{l.step}]</span> {l.detail}
                      </div>
                    ))}
                  </div>
                </div>
              ) : (
                <Button variant="ghost" disabled={loggingIn} onClick={autoCreate}>
                  {autoCreating ? <Spinner /> : "✨"} Create temporary account
                </Button>
              )}
            </div>
          </>
        )}
      </Step>

      <div className="flex items-center justify-between">
        <span className="text-xs text-neutral-500">
          You can also log in later from the Online tab.
        </span>
        <Button variant="primary" disabled={!gameSet} onClick={finish}>
          Start modding →
        </Button>
      </div>
    </div>
  );
}

function Step({
  n,
  title,
  children,
  done,
  disabled,
}: {
  n: number;
  title: string;
  children: React.ReactNode;
  done?: boolean;
  disabled?: boolean;
}) {
  return (
    <div
      className={`rounded-lg border p-4 ${
        disabled ? "border-neutral-800 opacity-50" : "border-neutral-700 bg-neutral-900/40"
      }`}
    >
      <div className="mb-2 flex items-center gap-2">
        <span
          className={`flex h-6 w-6 items-center justify-center rounded-full text-xs ${
            done ? "bg-green-600 text-white" : "bg-neutral-700 text-neutral-200"
          }`}
        >
          {done ? "✓" : n}
        </span>
        <span className="font-medium">{title}</span>
      </div>
      <div className="pl-8">{children}</div>
    </div>
  );
}
