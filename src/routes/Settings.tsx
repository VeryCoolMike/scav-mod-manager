import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, errMessage } from "../lib/api";
import type { BepInExStatus, DetectedGame } from "../lib/types";
import { Button, Spinner, useToast } from "../lib/ui";
import { useApp } from "../App";

export default function SettingsView() {
  const { settings, reloadSettings } = useApp();
  const toast = useToast();
  const [bepinex, setBepinex] = useState<BepInExStatus | null>(null);
  const [keyInput, setKeyInput] = useState(settings.nexus_api_key ?? "");
  const [linux, setLinux] = useState(settings.linux_launch ?? "");
  const [busy, setBusy] = useState<string | null>(null);
  const [candidates, setCandidates] = useState<DetectedGame[]>([]);

  async function acceptCandidate(c: DetectedGame) {
    await run("detect-accept", async () => {
      await api.setGamePath(c.path, c.source, c.steam_appid);
      await reloadSettings();
      setCandidates([]);
      toast.push("success", "Game folder updated.");
    });
  }

  function rejectCandidate(c: DetectedGame) {
    setCandidates((prev) => prev.filter((x) => x.path !== c.path));
  }

  const refreshBepinex = () => api.bepinexStatus().then(setBepinex).catch(() => {});
  useEffect(() => {
    refreshBepinex();
  }, [settings.game_path]);

  async function run(label: string, fn: () => Promise<void>) {
    setBusy(label);
    try {
      await fn();
    } catch (e) {
      toast.push("error", errMessage(e));
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="flex max-w-2xl flex-col gap-6">
      <h2 className="text-xl font-semibold">Settings</h2>

      {/* Game */}
      <Section title="Game folder">
        <div className="mb-2 break-all text-xs text-neutral-400">
          {settings.game_path ?? "not set"}{" "}
          {settings.game_source && <span className="text-neutral-600">({settings.game_source})</span>}
        </div>
        <div className="flex gap-2">
          <Button
            disabled={busy === "detect"}
            onClick={() =>
              run("detect", async () => {
                setCandidates([]);
                const found = await api.detectGames();
                if (found.length > 0) {
                  setCandidates(found);
                } else {
                  toast.push("info", "No install auto-detected.");
                }
              })
            }
          >
            {busy === "detect" ? <Spinner /> : "🔍"} Auto-detect
          </Button>
          <Button
            onClick={() =>
              run("browse", async () => {
                const dir = await open({ directory: true });
                if (typeof dir === "string") {
                  await api.setGamePath(dir, "manual");
                  await reloadSettings();
                  toast.push("success", "Game folder updated.");
                }
              })
            }
          >
            📁 Change…
          </Button>
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
                  disabled={busy === "detect-accept"}
                  onClick={() => acceptCandidate(c)}
                >
                  ✓ Yes
                </Button>
                <Button variant="ghost" onClick={() => rejectCandidate(c)}>
                  ✕ No
                </Button>
              </div>
            ))}
          </div>
        )}
      </Section>

      {/* Nexus */}
      <Section title="Nexus Mods account">
        <div className="mb-2 text-xs text-neutral-400">
          {settings.nexus_user
            ? `Signed in as ${settings.nexus_user}${settings.is_premium ? " · Premium ★" : " · Free"}`
            : "Not signed in"}
        </div>
        <div className="mb-3 flex gap-2">
          {settings.nexus_user ? (
            <Button
              variant="danger"
              disabled={busy === "logout"}
              onClick={() =>
                run("logout", async () => {
                  await api.nexusLogout();
                  await reloadSettings();
                  toast.push("success", "Signed out.");
                })
              }
            >
              Sign out
            </Button>
          ) : (
            <Button
              variant="primary"
              disabled={busy === "sso"}
              onClick={() =>
                run("sso", async () => {
                  toast.push("info", "Opening Nexus in your browser — click Authorize.");
                  const res = await api.nexusSsoLogin();
                  await reloadSettings();
                  toast.push(
                    res.valid ? "success" : "error",
                    res.valid ? `Signed in as ${res.name}` : "Login failed"
                  );
                })
              }
            >
              {busy === "sso" ? <Spinner /> : "🔗"} Login with Nexus
            </Button>
          )}
        </div>
        <details className="text-xs text-neutral-500">
          <summary className="cursor-pointer">Advanced: paste an API key manually</summary>
          <div className="mt-2 flex gap-2">
            <input
              type="password"
              value={keyInput}
              onChange={(e) => setKeyInput(e.target.value)}
              placeholder="Nexus personal API key"
              className="flex-1 rounded-md border border-neutral-700 bg-neutral-900 px-3 py-1.5 text-sm text-neutral-200 outline-none focus:border-green-600"
            />
            <Button
              disabled={busy === "key" || !keyInput.trim()}
              onClick={() =>
                run("key", async () => {
                  const res = await api.nexusValidate(keyInput.trim());
                  await reloadSettings();
                  toast.push(
                    res.valid ? "success" : "error",
                    res.valid ? `Signed in as ${res.name}` : "Invalid API key"
                  );
                })
              }
            >
              {busy === "key" ? <Spinner /> : "Save"}
            </Button>
          </div>
          <button
            className="mt-1 text-green-400 underline"
            onClick={() => openUrl("https://www.nexusmods.com/users/myaccount?tab=api")}
          >
            get API key
          </button>
        </details>
      </Section>

      {/* BepInEx */}
      <Section title="Mod loader (BepInEx)">
        <div className="mb-2 text-xs text-neutral-400">
          {bepinex?.installed
            ? `Installed${bepinex.version ? ` · v${bepinex.version}` : ""} · ${
                bepinex.enabled ? "enabled" : "disabled"
              }`
            : "Not installed"}
        </div>
        <div className="flex gap-2">
          <Button
            variant="primary"
            disabled={busy === "bep-install"}
            onClick={() =>
              run("bep-install", async () => {
                const st = await api.bepinexInstall();
                setBepinex(st);
                toast.push("success", "BepInEx installed / repaired.");
              })
            }
          >
            {busy === "bep-install" ? <Spinner /> : bepinex?.installed ? "Repair / update" : "Install"}
          </Button>
          {bepinex?.installed && (
            <Button
              variant="danger"
              disabled={busy === "bep-uninstall"}
              onClick={() =>
                run("bep-uninstall", async () => {
                  if (!confirm("Remove BepInEx from the game folder?")) return;
                  const st = await api.bepinexUninstall();
                  setBepinex(st);
                  toast.push("success", "BepInEx removed.");
                })
              }
            >
              Uninstall
            </Button>
          )}
        </div>
        {bepinex?.needs_proton_setup && bepinex.proton_launch_option && (
          <div className="mt-3 rounded-md border border-amber-800 bg-amber-950/40 p-3 text-xs text-amber-200">
            <b>Linux / Proton:</b> Steam → game Properties → Launch Options:
            <code className="mt-1 block rounded bg-black/40 px-2 py-1 text-amber-100">
              {bepinex.proton_launch_option}
            </code>
          </div>
        )}
      </Section>

      {/* Linux launch */}
      <Section title="Linux launch command (optional)">
        <p className="mb-2 text-xs text-neutral-500">
          For non-Steam Linux setups, provide a custom command to run the game (leave blank to use
          Steam or Wine).
        </p>
        <div className="flex gap-2">
          <input
            value={linux}
            onChange={(e) => setLinux(e.target.value)}
            placeholder='e.g. ./run_bepinex.sh'
            className="flex-1 rounded-md border border-neutral-700 bg-neutral-900 px-3 py-1.5 text-sm outline-none focus:border-green-600"
          />
          <Button
            disabled={busy === "linux"}
            onClick={() =>
              run("linux", async () => {
                await api.saveSettings({ ...settings, linux_launch: linux || null });
                await reloadSettings();
                toast.push("success", "Saved.");
              })
            }
          >
            Save
          </Button>
        </div>
      </Section>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="rounded-lg border border-neutral-800 bg-neutral-900/40 p-4">
      <div className="mb-2 font-medium">{title}</div>
      {children}
    </div>
  );
}
