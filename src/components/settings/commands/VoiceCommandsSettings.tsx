import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { Mic, Trash2, Plus, Globe, AppWindow, Type, Terminal } from "lucide-react";
import { ToggleSwitch } from "@/components/ui";
import { commands } from "@/bindings";
import type { VoiceCommand } from "@/bindings"; // auto-generated from Rust

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
        setCmds(res.data);
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
    const next: VoiceCommand = {
      id: uuid(),
      phrase: "",
      action_type: "open_url",
      action_payload: "",
      enabled: true,
    };
    save([...cmds, next]);
  };

  const update = (id: string, patch: Partial<VoiceCommand>) => {
    const next = cmds.map((c) => (c.id === id ? { ...c, ...patch } : c));
    save(next);
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
                <option value="type_text">
                  {t("commands.action.typeText", "Type Text")}
                </option>
                <option value="run_script">
                  {t("commands.action.runScript", "Run Script")}
                </option>
              </select>
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
                      : cmd.action_type === "run_script"
                        ? r"start notepad || open -a Notes"
                        : t("commands.payloadPlaceholder", "Text to type…")
                }
                className="flex-1 bg-transparent border-b border-mid-gray/30 focus:border-logo-primary outline-none text-sm py-1 px-0 text-foreground"
              />
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
  if (type === "run_script") return <Terminal className="w-4 h-4 opacity-60" />;
  return <Type className="w-4 h-4 opacity-60" />;
};
