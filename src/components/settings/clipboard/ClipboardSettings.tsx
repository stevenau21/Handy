import React, { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, Copy, RefreshCw, Search, Star, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { commands, type ClipboardEntry, type ClipboardUpdatePayload } from "@/bindings";
import { formatDateTime } from "@/utils/dateFormat";

const IconButton: React.FC<{
  onClick: () => void;
  title: string;
  disabled?: boolean;
  active?: boolean;
  children: React.ReactNode;
}> = ({ onClick, title, disabled, active, children }) => (
  <button
    onClick={onClick}
    disabled={disabled}
    className={`p-1.5 rounded-md flex items-center justify-center transition-colors cursor-pointer disabled:cursor-not-allowed disabled:text-text/20 ${
      active
        ? "text-logo-primary hover:text-logo-primary/80"
        : "text-text/50 hover:text-logo-primary"
    }`}
    title={title}
  >
    {children}
  </button>
);

const PAGE_SIZE = 30;

export const ClipboardSettings: React.FC = () => {
  const { t, i18n } = useTranslation();
  const [entries, setEntries] = useState<ClipboardEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [hasMore, setHasMore] = useState(true);
  const [searchQuery, setSearchQuery] = useState("");
  const [filterSaved, setFilterSaved] = useState(false);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editText, setEditText] = useState("");
  const [addingNoteId, setAddingNoteId] = useState<number | null>(null);
  const [editingNoteId, setEditingNoteId] = useState<number | null>(null);
  const [noteText, setNoteText] = useState("");
  const sentinelRef = useRef<HTMLDivElement>(null);
  const entriesRef = useRef<ClipboardEntry[]>([]);
  const loadingRef = useRef(false);

  useEffect(() => {
    entriesRef.current = entries;
  }, [entries]);

  const loadPage = useCallback(
    async (cursor?: number) => {
      const isFirstPage = cursor === undefined;
      if (!isFirstPage && loadingRef.current) return;
      loadingRef.current = true;

      if (isFirstPage) setLoading(true);

      try {
        const result = await commands.getClipboardEntries(
          cursor ?? null,
          PAGE_SIZE,
          filterSaved || null,
          searchQuery || null,
        );
        if (result.status === "ok") {
          const { entries: newEntries, has_more } = result.data;
          setEntries((prev) =>
            isFirstPage ? newEntries : [...prev, ...newEntries],
          );
          setHasMore(has_more);
        }
      } catch (error) {
        console.error("Failed to load clipboard entries:", error);
      } finally {
        setLoading(false);
        loadingRef.current = false;
      }
    },
    [filterSaved, searchQuery],
  );

  // Initial load
  useEffect(() => {
    setEntries([]);
    loadPage();
  }, [loadPage]);


  // Infinite scroll via IntersectionObserver
  useEffect(() => {
    if (loading) return;

    const sentinel = sentinelRef.current;
    if (!sentinel || !hasMore) return;

    const observer = new IntersectionObserver(
      (observerEntries) => {
        const first = observerEntries[0];
        if (first.isIntersecting) {
          const lastEntry = entriesRef.current[entriesRef.current.length - 1];
          if (lastEntry) {
            loadPage(lastEntry.id);
          }
        }
      },
      { threshold: 0 },
    );

    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [loading, hasMore, loadPage]);

  // Listen for clipboard real-time events (requires event registration in lib.rs)
  // Note: ClipboardUpdatePayload events are registered in lib.rs collect_events
  // The frontend listens via Tauri's event system
  useEffect(() => {
    let unlisten: (() => void) | undefined;

    import("@tauri-apps/api/event")
      .then(({ listen }) => {
        listen<ClipboardUpdatePayload>("clipboard-update-payload", (event) => {
          const payload = event.payload;
          if (payload.action === "added") {
            setEntries((prev) => {
              if (prev.some((e) => e.id === payload.entry.id)) return prev;
              return [payload.entry, ...prev];
            });
          } else if (payload.action === "updated") {
            setEntries((prev) =>
              prev.map((e) =>
                e.id === payload.entry.id ? payload.entry : e,
              ),
            );
          } else if (payload.action === "deleted") {
            setEntries((prev) => prev.filter((e) => e.id !== payload.id));
          } else if (payload.action === "cleared") {
            setEntries([]);
          } else if (payload.action === "toggled") {
            setEntries((prev) =>
              prev.map((e) =>
                e.id === payload.id ? { ...e, saved: !e.saved } : e,
              ),
            );
          }
        }).then((fn) => {
          unlisten = fn;
        });
      })
      .catch((err) => {
        console.warn("Failed to listen for clipboard events:", err);
      });

    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  const toggleSaved = async (id: number) => {
    setEntries((prev) =>
      prev.map((e) => (e.id === id ? { ...e, saved: !e.saved } : e)),
    );
    try {
      const result = await commands.toggleClipboardEntrySaved(id);
      if (result.status !== "ok") {
        setEntries((prev) =>
          prev.map((e) => (e.id === id ? { ...e, saved: !e.saved } : e)),
        );
      }
    } catch (error) {
      console.error("Failed to toggle saved:", error);
      setEntries((prev) =>
        prev.map((e) => (e.id === id ? { ...e, saved: !e.saved } : e)),
      );
    }
  };

  const copyToClipboard = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
    } catch (error) {
      console.error("Failed to copy to clipboard:", error);
    }
  };

  const deleteEntry = async (id: number) => {
    setEntries((prev) => prev.filter((e) => e.id !== id));
    try {
      const result = await commands.deleteClipboardEntry(id);
      if (result.status !== "ok") {
        loadPage();
      }
    } catch (error) {
      console.error("Failed to delete entry:", error);
      loadPage();
    }
  };

  const clearAll = async () => {
    if (!window.confirm(t("clipboard.clearAllConfirm"))) return;
    try {
      await commands.clearAllClipboardEntries();
      setEntries([]);
    } catch (error) {
      console.error("Failed to clear all:", error);
    }
  };

  const handleRefresh = async () => {
    try {
      const result = await commands.forceRefreshClipboard();
      if (result.status === "ok") {
        toast.success(result.data);
      } else {
        toast.error(String(result.error));
      }
    } catch (error) {
      console.error("Failed to refresh clipboard:", error);
      toast.error("Refresh failed — see logs");
    }
  };

  const startEdit = (entry: ClipboardEntry) => {
    setEditingId(entry.id);
    setEditText(entry.text);
  };

  const saveEdit = async () => {
    if (editingId === null) return;
    try {
      const result = await commands.editClipboardEntry(editingId, editText.trim());
      if (result.status === "ok") {
        setEntries((prev) =>
          prev.map((e) =>
            e.id === editingId ? result.data : e,
          ),
        );
        setEditingId(null);
      } else {
        toast.error(String(result.error));
      }
    } catch (error) {
      console.error("Failed to edit entry:", error);
    }
  };

  const cancelEdit = () => {
    setEditingId(null);
  };

  const saveNote = async (id: number) => {
    try {
      const result = await commands.setClipboardEntryNote(id, noteText.trim() || null);
      if (result.status === "ok") {
        setEntries((prev) =>
          prev.map((e) => (e.id === id ? result.data : e)),
        );
        setAddingNoteId(null);
        setEditingNoteId(null);
        setNoteText("");
      }
    } catch (error) {
      console.error("Failed to set note:", error);
    }
  };

  const deleteNote = async (id: number) => {
    try {
      const result = await commands.setClipboardEntryNote(id, null);
      if (result.status === "ok") {
        setEntries((prev) =>
          prev.map((e) => (e.id === id ? result.data : e)),
        );
      }
    } catch (error) {
      console.error("Failed to delete note:", error);
    }
  };

  const startAddNote = (entry: ClipboardEntry) => {
    setAddingNoteId(entry.id);
    setNoteText(entry.note || "");
  };

  const startEditNote = (entry: ClipboardEntry) => {
    setEditingNoteId(entry.id);
    setNoteText(entry.note || "");
  };

  const getSourceLabel = (source: string) => {
    const key = `clipboard.source.${source}` as const;
    return t(key, { defaultValue: source });
  };

  let content: React.ReactNode;

  if (loading) {
    content = (
      <div className="px-4 py-3 text-center text-text/60">
        {t("clipboard.loading")}
      </div>
    );
  } else if (entries.length === 0) {
    content = (
      <div className="px-4 py-3 text-center text-text/60">
        {t("clipboard.empty")}
      </div>
    );
  } else {
    content = (
      <>
        <div className="divide-y divide-mid-gray/20">
          {entries.map((entry) => (
            <div key={entry.id} className="px-4 py-2 pb-4 flex flex-col gap-2">
              <div className="flex justify-between items-center">
                <div className="flex items-center gap-2">
                  <span className="text-xs text-text/40">
                    {formatDateTime(String(entry.timestamp), i18n.language)}
                  </span>
                  <span className="text-xs px-1.5 py-0.5 rounded bg-mid-gray/20 text-text/50">
                    {getSourceLabel(entry.source)}
                  </span>
                </div>
                <div className="flex items-center">
                  <IconButton
                    onClick={() => copyToClipboard(entry.text)}
                    title={t("clipboard.copyToClipboard")}
                  >
                    <Copy width={14} height={14} />
                  </IconButton>
                  {editingId !== entry.id && (
                    <IconButton
                      onClick={() => startEdit(entry)}
                      title={t("clipboard.edit")}
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/></svg>
                    </IconButton>
                  )}
                  <IconButton
                    onClick={() => toggleSaved(entry.id)}
                    active={entry.saved}
                    title={entry.saved ? t("clipboard.unstar") : t("clipboard.star")}
                  >
                    <Star
                      width={14}
                      height={14}
                      fill={entry.saved ? "currentColor" : "none"}
                    />
                  </IconButton>
                  <IconButton
                    onClick={() => deleteEntry(entry.id)}
                    title={t("clipboard.delete")}
                  >
                    <Trash2 width={14} height={14} />
                  </IconButton>
                </div>
              </div>

              {editingId === entry.id ? (
                <div className="flex flex-col gap-2">
                  <textarea
                    value={editText}
                    onChange={(e) => setEditText(e.target.value)}
                    className="w-full min-h-[60px] p-2 text-sm bg-background border border-mid-gray/20 rounded-md text-text/90 resize-y focus:outline-none focus:border-logo-primary/50"
                    autoFocus
                  />
                  <div className="flex gap-2">
                    <button
                      onClick={saveEdit}
                      className="px-3 py-1 text-xs bg-logo-primary text-white rounded-md hover:bg-logo-primary/80"
                    >
                      {t("clipboard.save")}
                    </button>
                    <button
                      onClick={cancelEdit}
                      className="px-3 py-1 text-xs bg-mid-gray/20 text-text/70 rounded-md hover:bg-mid-gray/30"
                    >
                      {t("clipboard.cancel")}
                    </button>
                  </div>
                </div>
              ) : (
                <>
                  <p className="text-sm text-text/90 select-text cursor-text whitespace-pre-wrap break-words">
                    {entry.text}
                  </p>

                  {/* Note section */}
                  {entry.note && editingNoteId !== entry.id ? (
                    <div className="flex items-start justify-between gap-2 p-2 bg-logo-primary/5 border border-logo-primary/10 rounded-md">
                      <p className="text-xs text-logo-primary/80 italic">
                        {entry.note}
                      </p>
                      <div className="flex items-center gap-1 shrink-0">
                        <button
                          onClick={() => startEditNote(entry)}
                          className="p-0.5 text-text/30 hover:text-text/60 transition-colors"
                          title={t("clipboard.noteEdit")}
                        >
                          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/></svg>
                        </button>
                        <button
                          onClick={() => deleteNote(entry.id)}
                          className="p-0.5 text-text/30 hover:text-red-400 transition-colors"
                          title={t("clipboard.noteDelete")}
                        >
                          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
                        </button>
                      </div>
                    </div>
                  ) : null}

                  {/* Add/Edit note inline */}
                  {(addingNoteId === entry.id || editingNoteId === entry.id) && (
                    <div className="flex flex-col gap-2">
                      <input
                        type="text"
                        value={noteText}
                        onChange={(e) => setNoteText(e.target.value)}
                        placeholder={t("clipboard.notePlaceholder")}
                        className="w-full px-2 py-1 text-xs bg-background border border-logo-primary/30 rounded-md text-text/90 focus:outline-none focus:border-logo-primary/50"
                        autoFocus
                        onKeyDown={(e) => {
                          if (e.key === "Enter") saveNote(entry.id);
                          if (e.key === "Escape") {
                            setAddingNoteId(null);
                            setEditingNoteId(null);
                          }
                        }}
                      />
                      <div className="flex gap-2">
                        <button
                          onClick={() => saveNote(entry.id)}
                          className="px-2 py-0.5 text-xs bg-logo-primary text-white rounded-md hover:bg-logo-primary/80"
                        >
                          {t("clipboard.save")}
                        </button>
                        <button
                          onClick={() => {
                            setAddingNoteId(null);
                            setEditingNoteId(null);
                          }}
                          className="px-2 py-0.5 text-xs bg-mid-gray/20 text-text/70 rounded-md hover:bg-mid-gray/30"
                        >
                          {t("clipboard.cancel")}
                        </button>
                      </div>
                    </div>
                  )}

                  {/* "Add note" button (only shown when no note and not currently adding/editing) */}
                  {!entry.note && addingNoteId !== entry.id && (
                    <button
                      onClick={() => startAddNote(entry)}
                      className="self-start text-xs text-text/30 hover:text-logo-primary/70 transition-colors flex items-center gap-1"
                    >
                      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>
                      {t("clipboard.noteAdd")}
                    </button>
                  )}
                </>
              )}
            </div>
          ))}
        </div>
        <div ref={sentinelRef} className="h-1" />
      </>
    );
  }

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <div className="space-y-2">
        {/* Header with title and action buttons */}
        <div className="px-4 flex items-center justify-between">
          <h2 className="text-xs font-medium text-mid-gray uppercase tracking-wide">
            {t("clipboard.title")}
          </h2>
          <div className="flex items-center gap-2">
            <span className="text-[10px] px-2 py-0.5 rounded-full bg-green-500/10 text-green-400 border border-green-500/20">
              tracking
            </span>
            <button
              onClick={handleRefresh}
              className="flex items-center gap-1.5 px-3 py-1.5 text-xs bg-logo-primary/10 text-logo-primary rounded-md hover:bg-logo-primary/20 transition-colors"
              title="Force clipboard refresh — clears dedup state and re-reads clipboard"
            >
              <RefreshCw width={14} height={14} />
              <span>Refresh</span>
            </button>
            <button
              onClick={clearAll}
              className="flex items-center gap-1.5 px-3 py-1.5 text-xs bg-mid-gray/20 text-text/70 rounded-md hover:bg-mid-gray/30 transition-colors"
              title={t("clipboard.clearAll")}
            >
              <Trash2 width={14} height={14} />
              <span>{t("clipboard.clearAll")}</span>
            </button>
          </div>
        </div>

        {/* Search bar and filters */}
        <div className="px-4 flex items-center gap-2">
          <div className="relative flex-1">
            <Search className="absolute left-2 top-1/2 -translate-y-1/2 w-4 h-4 text-text/30" />
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder={t("clipboard.searchPlaceholder")}
              className="w-full pl-8 pr-3 py-1.5 text-sm bg-background border border-mid-gray/20 rounded-md text-text/90 focus:outline-none focus:border-logo-primary/50"
            />
          </div>
          <button
            onClick={() => setFilterSaved(!filterSaved)}
            className={`px-2 py-1.5 text-xs rounded-md transition-colors ${
              filterSaved
                ? "bg-logo-primary/20 text-logo-primary"
                : "bg-mid-gray/20 text-text/50 hover:text-text/70"
            }`}
            title={t("clipboard.filter.favorites")}
          >
            <Star width={14} height={14} fill={filterSaved ? "currentColor" : "none"} />
          </button>
        </div>

        {/* Auto-track info */}
        <div className="px-4 flex items-center justify-between">
          <div className="flex flex-col">
            <span className="text-sm">{t("clipboard.autoTrack")}</span>
            <span className="text-xs text-text/40">{t("clipboard.autoTrackDescription")}</span>
          </div>
          <span className="text-[10px] px-2 py-0.5 rounded-full bg-logo-primary/10 text-logo-primary border border-logo-primary/20">
            {t("clipboard.active")}
          </span>
        </div>

        {/* Entries list */}
        <div className="bg-background border border-mid-gray/20 rounded-lg overflow-visible">
          {content}
        </div>
      </div>
    </div>
  );
};
