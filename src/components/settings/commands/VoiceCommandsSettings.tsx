import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { Mic, Trash2, Plus, Globe, AppWindow, Type, Terminal, Search, Send, FolderOpen, Layers } from "lucide-react";
import { ToggleSwitch } from "@/components/ui";
import { commands } from "@/bindings";
import type { VoiceCommand } from "@/bindings";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

const uuid = () =>
  `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`;

export const VoiceCommandsSettings: React.FC = () => {
  const { t } = useTranslation();
  const [cmds, setCmds] = useState<VoiceCommand[]>([]);
  const [loading, setLoading] = useState(true);

  React.useEffect(() => {
    commands
      .getVoiceCommands()
      .then((res) => {
        if (res.status === "ok") {
          setCmds(res.data);
        }
        setLoading(false);
      })
      .catch(() => setLoading(false));
  }, []);

  const save = (next: VoiceCommand[]) => {
    setCmds(next);
    commands.setVoiceCommands(next).catch(console.error);
  };

  const toggle = (id: string) => {
    const next = cmds.map((c) =>
      c.id === id ? { ...c, enabled: !c.enabled } : c,
    );
    save(next);
  };

  const remove = (id: string) => {
    save(cmds.filter((c) => c.id !== id));
  };

  const add = () => {
    const newCmd: VoiceCommand = {
      id: uuid(),
      phrase: "",
      action_type: "open_url",
      action_payload: "",
      enabled: true,
    };
    // If workspace is enabled, auto-add the new command to it
    if (workspaceEnabled && workspaceCmd) {
      const ids = new Set(workspaceCommandIds);
      ids.add(newCmd.id);
      newCmd.action_payload = JSON.stringify([...ids]);
      // Update workspace payload too
      update(workspaceCmd.id, { action_payload: JSON.stringify([...ids]) });
    }
    save([...cmds, newCmd]);
  };

  const update = (id: string, patch: Partial<VoiceCommand>) => {
    const next = cmds.map((c) => (c.id === id ? { ...c, ...patch } : c));
    save(next);
  };

  const browseFolder = async (cmdId: string) => {
    try {
      const selected = await openDialog({ directory: true, multiple: false });
      if (selected && typeof selected === "string") {
        update(cmdId, { action_type: "open_folder", action_payload: selected });
      }
    } catch (e) {
      console.error("Folder picker failed:", e);
    }
  };

  // Workspace: find or create the workspace command
  const workspaceCmd = cmds.find((c) => c.action_type === "run_workspace");
  const workspaceEnabled = workspaceCmd?.enabled ?? false;
  const workspacePhrase = workspaceCmd?.phrase ?? "";
  // Parse workspace payload as an array of command IDs
  const workspaceCommandIds: string[] = (() => {
    if (!workspaceCmd || !workspaceCmd.action_payload) return [];
    try {
      return JSON.parse(workspaceCmd.action_payload);
    } catch {
      return [];
    }
  })();
  // All commands except the workspace command itself
  const nonWorkspaceCmds = cmds.filter((c) => c.action_type !== "run_workspace");

  const toggleWorkspaceCommand = (cmdId: string) => {
    if (!workspaceCmd) return;
    const ids = new Set(workspaceCommandIds);
    if (ids.has(cmdId)) {
      ids.delete(cmdId);
    } else {
      ids.add(cmdId);
    }
    update(workspaceCmd.id, { action_payload: JSON.stringify([...ids]) });
  };

  const toggleWorkspaceEnabled = () => {
    if (!workspaceCmd) {
      // Create a new workspace command
      const newCmd: VoiceCommand = {
        id: uuid(),
        phrase: "activate workspace",
        action_type: "run_workspace",
        action_payload: JSON.stringify(nonWorkspaceCmds.map((c) => c.id)),
        enabled: true,
      };
      save([...cmds, newCmd]);
    } else {
      toggle(workspaceCmd.id);
    }
  };

  const updateWorkspacePhrase = (phrase: string) => {
    if (!workspaceCmd) return;
    update(workspaceCmd.id, { phrase });
  };

  if (loading) return <p className="text-sm opacity-60">Loading…</p>;

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-center gap-3">
        <Mic className="w-5 h-5 text-logo-primary" />
        <h2 className="text-lg font-semibold">
          {t("commands.title", "Voice Commands")}
        </h2>
      </div>

      <p className="text-sm text-foreground/70 -mt-3">
        {t(
          "commands.description",
          "Phrases spoken after your wake word that trigger actions instead of pasting text.",
        )}
      </p>

      {/* Workspace Section */}
      <div className="rounded-xl border border-logo-primary/20 bg-logo-primary/5 p-4 flex flex-col gap-3">
        <div className="flex items-center gap-3">
          <Layers className="w-5 h-5 text-logo-primary" />
          <h3 className="text-base font-semibold">
            {t("commands.workspace.title", "Workspace")}
          </h3>
          <ToggleSwitch
            checked={workspaceEnabled}
            onChange={toggleWorkspaceEnabled}
            label=""
            description=""
          />
        </div>
        <p className="text-sm text-foreground/70">
          {t(
            "commands.workspace.description",
            "Run multiple commands at once. Toggle on, select which commands to include, then say the trigger phrase.",
          )}
        </p>
        {workspaceEnabled && (
          <>
            <div className="flex items-center gap-2">
              <label className="text-xs text-foreground/60">Trigger phrase:</label>
              <input
                value={workspacePhrase}
                onChange={(e) => updateWorkspacePhrase(e.target.value)}
                placeholder={t(
                  "commands.workspace.phrasePlaceholder",
                  'e.g. "activate workspace"',
                )}
                className="flex-1 bg-transparent border-b border-mid-gray/30 focus:border-logo-primary outline-none text-sm py-1 px-0 text-foreground"
              />
            </div>
            <div className="flex flex-col gap-2 mt-1">
              <span className="text-xs text-foreground/60">
                {t(
                  "commands.workspace.selectCommands",
                  "Select commands to include in the workspace:",
                )}
              </span>
              {nonWorkspaceCmds.map((cmd) => (
                <label
                  key={cmd.id}
                  className="flex items-center gap-2 text-sm cursor-pointer"
                >
                  <input
                    type="checkbox"
                    checked={workspaceCommandIds.includes(cmd.id)}
                    onChange={() => toggleWorkspaceCommand(cmd.id)}
                    className="accent-logo-primary"
                  />
                  <ActionIcon type={cmd.action_type} />
                  <span className="text-foreground/80">
                    {cmd.phrase || "(unnamed)"}
                  </span>
                  <span className="text-foreground/40 text-xs ml-auto">
                    {cmd.action_type.replace("_", " ")}
                  </span>
                </label>
              ))}
            </div>
          </>
        )}
      </div>

      {/* Regular Commands Section */}
      <div className="flex flex-col gap-3">
        {cmds.map((cmd) => (
          <div
            key={cmd.id}
            className="flex flex-col gap-2 p-3 rounded-xl bg-mid-gray/10 border border-mid-gray/20"
          >
            <div className="flex items-center gap-2 justify-between">
              <div className="flex items-center gap-2 flex-1 min-w-0">
                <ToggleSwitch
                  checked={cmd.enabled}
                  onChange={() => toggle(cmd.id)}
                  label=""
                  description=""
                />
                <input
                  value={cmd.phrase}
                  onChange={(e) =>
                    update(cmd.id, { phrase: e.target.value })
                  }
                  placeholder={t(
                    "commands.phrasePlaceholder",
                    'e.g. "open youtube"',
                  )}
                  className="flex-1 min-w-0 bg-transparent border-b border-mid-gray/30 focus:border-logo-primary outline-none text-sm py-1 px-0 text-foreground"
                />
              </div>
              <button
                onClick={() => remove(cmd.id)}
                className="text-red-500/70 hover:text-red-500 transition-colors p-1"
                title={t("commands.remove", "Remove")}
              >
                <Trash2 className="w-4 h-4" />
              </button>
            </div>

            <div className="flex items-center gap-2 pl-10">
              <ActionIcon type={cmd.action_type} />
              <select
                value={cmd.action_type}
                onChange={(e) =>
                  update(cmd.id, { action_type: e.target.value })
                }
                className="bg-mid-gray/20 rounded-md px-2 py-1 text-xs border-none outline-none text-foreground cursor-pointer"
              >
                <option value="open_url">
                  {t("commands.action.openUrl", "Open URL")}
                </option>
                <option value="open_app">
                  {t("commands.action.openApp", "Open App")}
                </option>
                <option value="open_folder">
                  {t("commands.action.openFolder", "Open Folder")}
                </option>
                <option value="type_text">
                  {t("commands.action.typeText", "Type Text")}
                </option>
                <option value="search_url">
                  {t("commands.action.searchUrl", "Search URL")}
                </option>
                <option value="send_message">
                  {t("commands.action.sendMessage", "Send Message")}
                </option>
                <option value="run_script">
                  {t("commands.action.runScript", "Run Script")}
                </option>
              </select>

              <div className="flex items-center gap-1 flex-1">
                <input
                  value={cmd.action_payload}
                  onChange={(e) =>
                    update(cmd.id, { action_payload: e.target.value })
                  }
                  placeholder={
                    cmd.action_type === "open_url"
                      ? "youtube.com"
                      : cmd.action_type === "open_app"
                        ? "chrome"
                        : cmd.action_type === "open_folder"
                          ? "C:\\Users\\steve\\Downloads"
                          : cmd.action_type === "search_url"
                            ? "https://youtube.com/results?search_query="
                            : cmd.action_type === "send_message"
                              ? "hermes_opss_bot"
                              : cmd.action_type === "run_script"
                                ? "start notepad || open -a Notes"
                                : t("commands.payloadPlaceholder", "Text to type…")
                  }
                  className="flex-1 bg-transparent border-b border-mid-gray/30 focus:border-logo-primary outline-none text-sm py-1 px-0 text-foreground"
                />
                {cmd.action_type === "open_folder" && (
                  <button
                    type="button"
                    onClick={() => browseFolder(cmd.id)}
                    className="text-logo-primary hover:opacity-80 px-1 py-0.5 rounded text-xs border border-logo-primary/30"
                  >
                    Browse…
                  </button>
                )}
              </div>
            </div>
          </div>
        ))}

        <button
          onClick={add}
          className="flex items-center gap-2 text-sm text-logo-primary hover:opacity-80 transition-opacity mt-1"
        >
          <Plus className="w-4 h-4" />
          {t("commands.add", "Add Command")}
        </button>
      </div>
    </div>
  );
};

const ActionIcon: React.FC<{ type: string }> = ({ type }) => {
  if (type === "open_url") return <Globe className="w-4 h-4 opacity-60" />;
  if (type === "open_app") return <AppWindow className="w-4 h-4 opacity-60" />;
  if (type === "open_folder") return <FolderOpen className="w-4 h-4 opacity-60" />;
  if (type === "search_url") return <Search className="w-4 h-4 opacity-60" />;
  if (type === "send_message") return <Send className="w-4 h-4 opacity-60" />;
  if (type === "run_script") return <Terminal className="w-4 h-4 opacity-60" />;
  if (type === "run_workspace") return <Layers className="w-4 h-4 opacity-60" />;
  return <Type className="w-4 h-4 opacity-60" />;
};
