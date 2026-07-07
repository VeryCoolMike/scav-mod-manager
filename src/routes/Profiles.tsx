import { useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Copy, Download, Files, Plus, Trash2, Upload } from "lucide-react";
import { api, errMessage } from "../lib/api";
import { Button, Spinner, useToast } from "../lib/ui";
import { useApp } from "../App";

export default function Profiles() {
  const { settings, profiles, reloadProfiles, reloadSettings, bumpMods } = useApp();
  const toast = useToast();
  const [newName, setNewName] = useState("");
  const [busy, setBusy] = useState(false);
  const [codeInput, setCodeInput] = useState("");
  const [importProgress, setImportProgress] = useState<string | null>(null);

  async function withBusy(fn: () => Promise<void>) {
    setBusy(true);
    try {
      await fn();
    } catch (e) {
      toast.push("error", errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  const create = () =>
    withBusy(async () => {
      if (!newName.trim()) return;
      await api.createProfile(newName.trim());
      setNewName("");
      await reloadProfiles();
      toast.push("success", "Profile created.");
    });

  const switchTo = (name: string) =>
    withBusy(async () => {
      await api.switchProfile(name);
      await reloadSettings();
      bumpMods();
      toast.push("success", `Switched to "${name}".`);
    });

  const clone = (from: string) =>
    withBusy(async () => {
      const to = prompt(`Clone "${from}" as:`, `${from} copy`);
      if (!to) return;
      await api.cloneProfile(from, to);
      await reloadProfiles();
      toast.push("success", "Profile cloned.");
    });

  const del = (name: string) =>
    withBusy(async () => {
      if (!confirm(`Delete profile "${name}"? Its installed mod files will be removed.`)) return;
      await api.deleteProfile(name);
      await reloadProfiles();
      await reloadSettings();
      bumpMods();
      toast.push("success", "Profile deleted.");
    });

  const exportBundle = (name: string) =>
    withBusy(async () => {
      const dest = await save({
        title: "Export profile bundle",
        defaultPath: `${name}.smmprofile.zip`,
        filters: [{ name: "Profile bundle", extensions: ["zip"] }],
      });
      if (!dest) return;
      await api.exportProfileBundle(name, dest);
      toast.push("success", "Exported.");
    });

  const importBundle = () =>
    withBusy(async () => {
      const src = await open({
        title: "Import profile bundle",
        filters: [{ name: "Profile bundle", extensions: ["zip"] }],
      });
      if (typeof src !== "string") return;
      const name = await api.importProfileBundle(src);
      await reloadProfiles();
      toast.push("success", `Imported as "${name}".`);
    });

  const exportCode = (name: string) =>
    withBusy(async () => {
      const code = await api.exportProfileCode(name);
      await navigator.clipboard.writeText(code);
      toast.push("success", "Mod-list code copied to clipboard.");
    });

  const importCode = () =>
    withBusy(async () => {
      const code = codeInput.trim();
      if (!code) return;
      const start = await api.importProfileCode(code);
      setCodeInput("");
      await reloadProfiles();
      await reloadSettings();
      bumpMods();

      let installed = 0;
      const failed: string[] = [];
      for (let i = 0; i < start.mods.length; i++) {
        const m = start.mods[i];
        setImportProgress(`Installing "${m.name}" (${i + 1}/${start.mods.length})…`);
        try {
          if (m.source === "gamebanana") {
            await api.gbInstall(m.mod_id, m.file_id);
          } else if (settings.is_premium) {
            await api.installModFile(m.mod_id, m.file_id);
          } else {
            await api.nexusAutoDownload(m.mod_id, m.file_id);
          }
          installed++;
        } catch (e) {
          failed.push(`${m.name}: ${errMessage(e)}`);
        }
      }
      setImportProgress(null);
      bumpMods();
      toast.push(
        failed.length === 0 ? "success" : "error",
        `Imported "${start.profile}": ${installed}/${start.mods.length} mods installed` +
          (failed.length ? ` — ${failed.length} failed (${failed.join("; ")})` : "")
      );
    });

  return (
    <div className="flex max-w-3xl flex-col gap-4">
      <div className="flex items-center justify-between">
        <h2 className="text-xl font-semibold">Profiles</h2>
        {busy && <Spinner />}
      </div>

      <div className="flex gap-2">
        <input
          value={newName}
          onChange={(e) => setNewName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && create()}
          placeholder="New profile name…"
          className="flex-1 rounded-md border border-neutral-700 bg-neutral-900 px-3 py-1.5 text-sm outline-none focus:border-green-600"
        />
        <Button variant="primary" onClick={create} disabled={!newName.trim()}>
          <Plus className="h-4 w-4" /> Create
        </Button>
        <Button onClick={importBundle}>
          <Upload className="h-4 w-4" /> Import
        </Button>
      </div>

      <div className="flex gap-2 rounded-lg border border-neutral-800 bg-neutral-900/40 p-3">
        <input
          value={codeInput}
          onChange={(e) => setCodeInput(e.target.value)}
          placeholder="Paste a mod-list code to import it as a new profile…"
          className="flex-1 rounded-md border border-neutral-700 bg-neutral-900 px-3 py-1.5 text-sm outline-none focus:border-green-600"
        />
        <Button variant="primary" onClick={importCode} disabled={!codeInput.trim() || !!importProgress}>
          {importProgress ? <Spinner /> : <Copy className="h-4 w-4" />} Import code
        </Button>
      </div>
      {importProgress && (
        <div className="flex items-center gap-2 rounded-md border border-amber-800 bg-amber-950/20 px-3 py-2 text-xs text-amber-300">
          <Spinner /> {importProgress}
        </div>
      )}

      <div className="flex flex-col gap-2">
        {profiles.map((p) => {
          const active = p === settings.active_profile;
          return (
            <div
              key={p}
              className={`flex items-center gap-3 rounded-lg border p-3 ${
                active ? "border-green-700 bg-green-950/20" : "border-neutral-800 bg-neutral-900/40"
              }`}
            >
              <div className="flex-1 font-medium">
                {p}
                {active && <span className="ml-2 text-xs text-green-400">● active</span>}
              </div>
              {!active && (
                <Button variant="primary" onClick={() => switchTo(p)}>
                  Use
                </Button>
              )}
              <Button variant="ghost" onClick={() => exportCode(p)} title="Copy mod-list code">
                <Copy className="h-4 w-4" /> Code
              </Button>
              <Button variant="ghost" onClick={() => exportBundle(p)} title="Export zip bundle">
                <Download className="h-4 w-4" /> Export
              </Button>
              <Button variant="ghost" onClick={() => clone(p)}>
                <Files className="h-4 w-4" /> Clone
              </Button>
              {p !== "Default" && (
                <Button variant="danger" onClick={() => del(p)}>
                  <Trash2 className="h-4 w-4" />
                </Button>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
