import { useId, useState } from "react";
import type { SessionDrilldown, SessionTitle as SourceTitle } from "./bridge";
import { errorMessage } from "./format";

// App-owned labels. Source metadata and transcripts are never written here.
function readOverride(key: string): { title: string | null; error: string | null } {
  try {
    return { title: window.localStorage.getItem(key), error: null };
  } catch (error) {
    return { title: null, error: `Saved title could not be read: ${errorMessage(error)}` };
  }
}

export function SessionTitle({ source, projectName, session, sourceTitle }: {
  source: string;
  projectName: string;
  session: SessionDrilldown;
  sourceTitle?: SourceTitle;
}) {
  const storageKey = `ccstats.session-title:${JSON.stringify([source, session.session_id])}`;
  const [saved, setSaved] = useState(() => readOverride(storageKey));
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [writeError, setWriteError] = useState<string | null>(null);
  const inputId = useId();
  const timestamp = new Date(session.first_timestamp).toLocaleString();
  const fallback = `${projectName || source} · ${timestamp} · ${session.session_id.slice(0, 8)}`;
  const title = saved.title ?? sourceTitle?.text ?? fallback;
  const origin = saved.title !== null ? "Manual title" : sourceTitle?.origin === "source_title"
    ? "Source title" : sourceTitle?.origin === "source_summary" ? "Source summary" : "Session label";

  function persist(value: string | null) {
    try {
      if (value === null) window.localStorage.removeItem(storageKey);
      else window.localStorage.setItem(storageKey, value);
      setSaved({ title: value, error: null });
      setWriteError(null);
      setEditing(false);
    } catch (error) {
      setWriteError(`Title could not be saved: ${errorMessage(error)}`);
    }
  }

  return (
    <div className="session-title">
      <strong className="session-title-text">{title}</strong>
      <span>{origin} · Last active {new Date(session.last_timestamp).toLocaleString()}</span>
      <code className="session-identity">{session.session_id}</code>
      {saved.error ? <div role="alert" className="session-title-error">
        <p>{saved.error}</p><button type="button" onClick={() => setSaved(readOverride(storageKey))}>Retry saved title</button>
      </div> : editing ? (
        <form className="session-title-editor" onSubmit={(event) => {
          event.preventDefault();
          const value = draft.trim();
          if (!value) { setWriteError("Enter a title, or cancel to keep the current name."); return; }
          persist(value);
        }}>
          <label htmlFor={inputId}>Session title</label>
          <input id={inputId} autoFocus value={draft} onChange={(event) => setDraft(event.target.value)} />
          <p>Saved only in this app on this device. Original chats and usage totals stay unchanged.</p>
          <div className="session-title-actions">
            <button type="submit">Save title</button>
            <button type="button" onClick={() => { setEditing(false); setWriteError(null); }}>Cancel</button>
          </div>
        </form>
      ) : (
        <div className="session-title-actions">
          <button type="button" aria-label={`Rename session ${session.session_id}`} onClick={() => {
            setDraft(saved.title ?? sourceTitle?.text ?? ""); setWriteError(null); setEditing(true);
          }}>Rename</button>
          {saved.title !== null && <button type="button" onClick={() => persist(null)}>Use source title</button>}
        </div>
      )}
      {writeError && <p role="alert" className="session-title-error">{writeError}</p>}
    </div>
  );
}
