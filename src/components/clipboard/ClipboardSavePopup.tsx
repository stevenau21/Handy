import React, { useEffect, useRef, useState, useCallback } from "react";
import { commands, type InterceptEvent } from "@/bindings";

interface PendingIntercept {
  intercept_id: string;
  text: string;
  source: string; // "ocr" or "clipboard"
}

export const ClipboardSavePopup: React.FC = () => {
  const [queue, setQueue] = useState<PendingIntercept[]>([]);
  const [current, setCurrent] = useState<PendingIntercept | null>(null);
  const [editedText, setEditedText] = useState("");
  const [note, setNote] = useState("");
  const [saving, setSaving] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const noteRef = useRef<HTMLInputElement>(null);
  const currentRef = useRef<PendingIntercept | null>(null);

  // Keep ref in sync so event listener always sees latest
  useEffect(() => {
    currentRef.current = current;
  }, [current]);

  // Listen for intercept-event from Rust
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    import("@tauri-apps/api/event")
      .then(({ listen }) => {
        listen<InterceptEvent>("intercept-event", (event) => {
          const incoming: PendingIntercept = {
            intercept_id: event.payload.intercept_id,
            text: event.payload.text,
            source: event.payload.source || "clipboard",
          };

          const curr = currentRef.current;

          // OCR (screenshot) is always the primary — opens/reopens popup
          if (incoming.source === "ocr") {
            if (!curr) {
              setCurrent(incoming);
              setEditedText(incoming.text);
              setNote("");
            } else {
              // OCR comes in while popup is open — queue it
              setQueue((prev) => [...prev, incoming]);
            }
          } else {
            // clipboard (voice paste / text copy) → fills note if popup open, queues otherwise
            if (!curr) {
              setCurrent(incoming);
              setEditedText(incoming.text);
              setNote("");
            } else {
              // Popup is open — voice goes to note field
              setNote(incoming.text);
              // Discard the voice intercept since we're using it as note
              commands.discardClipboardIntercept(incoming.intercept_id).catch(() => {});
            }
          }
        }).then((fn) => {
          unlisten = fn;
        });
      })
      .catch(console.error);
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // Auto-focus note input when OCR popup opens (so voice dictation fills it directly)
  useEffect(() => {
    if (current && noteRef.current) {
      noteRef.current.focus();
    }
  }, [current?.intercept_id]);

  const advanceQueue = useCallback(() => {
    setQueue((q) => {
      if (q.length > 0) {
        const [next, ...rest] = q;
        setTimeout(() => {
          setCurrent(next);
          setEditedText(next.text);
          setNote("");
        }, 0);
        return rest;
      }
      setCurrent(null);
      setEditedText("");
      setNote("");
      return [];
    });
  }, []);

  const handleSave = async () => {
    if (!current) return;
    setSaving(true);
    try {
      const finalText = editedText.trim();
      if (!finalText) {
        // Nothing to save, just discard
        await commands.discardClipboardIntercept(current.intercept_id);
        advanceQueue();
        return;
      }

      if (note.trim()) {
        // Save with note
        const result = await commands.confirmClipboardInterceptWithText(
          current.intercept_id,
          finalText,
        );
        if (result.status === "ok" && result.data) {
          // Set the note on the saved entry
          await commands.setClipboardEntryNote(result.data.id, note.trim());
        }
      } else {
        // Save without note
        await commands.confirmClipboardInterceptWithText(
          current.intercept_id,
          finalText,
        );
      }
    } catch (e) {
      console.error("Failed to save intercept:", e);
    } finally {
      setSaving(false);
    }
    advanceQueue();
  };

  const handleDiscard = async () => {
    if (!current) return;
    try {
      await commands.discardClipboardIntercept(current.intercept_id);
    } catch (e) {
      console.error("Failed to discard intercept:", e);
    }
    advanceQueue();
  };

  // Don't render anything if no intercept is pending
  if (!current) return null;

  const queueCount = queue.length;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
      <div className="bg-background border border-mid-gray/20 rounded-xl shadow-2xl p-6 max-w-lg w-full mx-4 flex flex-col gap-4">
        {/* Header */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <h3 className="text-sm font-semibold text-text/90">
              Save to Clipboard?
            </h3>
            {queueCount > 0 && (
              <span className="text-[10px] px-1.5 py-0.5 rounded-full bg-logo-primary/20 text-logo-primary font-medium">
                {queueCount} more
              </span>
            )}
          </div>
          <button
            onClick={handleDiscard}
            className="text-text/30 hover:text-text/60 transition-colors"
          >
            <svg
              width="16"
              height="16"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>

        {/* Editable text area — the main content (screenshot OCR) */}
        <div className="flex flex-col gap-1.5">
          <label className="text-[10px] text-text/30 uppercase tracking-wide">
            Text (edit before saving)
          </label>
          <textarea
            ref={textareaRef}
            value={editedText}
            onChange={(e) => setEditedText(e.target.value)}
            rows={6}
            className="w-full p-3 text-xs bg-mid-gray/10 border border-mid-gray/20 rounded-lg text-text/80 whitespace-pre-wrap resize-y focus:outline-none focus:border-logo-primary/40"
            placeholder="Screenshot OCR text..."
            onKeyDown={(e) => {
              // Ctrl+Enter to save
              if (e.key === "Enter" && e.ctrlKey) {
                e.preventDefault();
                handleSave();
              }
            }}
          />
        </div>

        {/* Optional note (voice dictation) */}
        <div className="flex flex-col gap-1.5">
          <label className="text-[10px] text-text/30 uppercase tracking-wide">
            Note (voice dictation / quick tag)
          </label>
          <input
            ref={noteRef}
            type="text"
            value={note}
            onChange={(e) => setNote(e.target.value)}
            placeholder="Voice note appears here..."
            className="w-full px-3 py-1.5 text-xs bg-mid-gray/10 border border-mid-gray/20 rounded-md text-text/70 focus:outline-none focus:border-logo-primary/30"
          />
        </div>

        {/* Action buttons */}
        <div className="flex gap-2">
          <button
            onClick={handleSave}
            disabled={saving || !editedText.trim()}
            className="flex-1 px-4 py-2 text-sm bg-logo-primary text-white rounded-lg hover:bg-logo-primary/80 transition-colors font-medium disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {saving ? "Saving..." : "Save"}
          </button>
          <button
            onClick={handleDiscard}
            disabled={saving}
            className="flex-1 px-4 py-2 text-sm bg-mid-gray/20 text-text/70 rounded-lg hover:bg-mid-gray/30 transition-colors disabled:opacity-50"
          >
            Discard
          </button>
        </div>
      </div>
    </div>
  );
};

export default ClipboardSavePopup;