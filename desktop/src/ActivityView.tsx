import { useCallback, useEffect, useRef, useState } from "react";
import type { DesktopBridge, TurnToolBreakdown, UsageRange } from "./bridge";
import { errorMessage, formatTokens } from "./format";

function localTimestamp(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

export function ActivityView({ bridge, source, range }: {
  bridge: DesktopBridge;
  source: string;
  range: UsageRange;
}) {
  const [data, setData] = useState<TurnToolBreakdown | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const requestId = useRef(0);

  const load = useCallback(async () => {
    if (source === "all") {
      setData(null);
      setError(null);
      setLoading(false);
      return;
    }
    const currentRequest = ++requestId.current;
    setLoading(true);
    setError(null);
    try {
      const next = await bridge.turnToolBreakdown(source, range);
      if (requestId.current === currentRequest) setData(next);
    } catch (nextError) {
      if (requestId.current === currentRequest) {
        setData(null);
        setError(errorMessage(nextError));
      }
    } finally {
      if (requestId.current === currentRequest) setLoading(false);
    }
  }, [bridge, range, source]);

  useEffect(() => {
    void load();
    return () => { requestId.current += 1; };
  }, [load]);

  if (source === "all") {
    return <section className="state-pane empty-state"><span aria-hidden="true">↳</span><div><h2>Choose one source</h2><p>Turn evidence belongs to one source ledger. Choose a concrete source to inspect it.</p></div></section>;
  }
  if (loading) {
    return <section className="loading-state" aria-live="polite"><div><i /><i /><i /></div><strong>Resolving turns and tool calls…</strong><span>Parse · deduplicate · order</span></section>;
  }
  if (error) {
    return <section className="state-pane error-state" role="alert"><span aria-hidden="true">!</span><div><h2>Could not read activity evidence.</h2><p>{error}</p><button type="button" onClick={() => void load()}>Try again</button></div></section>;
  }
  if (!data) return null;

  const qualityWarning = data.quality.dedup_skipped_entries > 0 || data.quality.parse_error_entries > 0;
  const maxCalls = Math.max(data.tools[0]?.calls ?? 0, 1);

  return (
    <section className="activity-workbench" aria-labelledby="activity-title">
      <header className="work-pane activity-heading">
        <div>
          <h2 id="activity-title">Turn and tool evidence</h2>
          <p>One turn is one deduplicated usage-bearing model response.</p>
        </div>
        <button type="button" className="refresh-button" onClick={() => void load()}>↻ Refresh activity</button>
      </header>

      <dl className="activity-summary">
        <div><dt>Model turns</dt><dd data-testid="activity-turn-count">{formatTokens(data.total_turns)}</dd><small>{data.turns.length < data.total_turns ? `Latest ${data.turns.length} shown` : "Complete window"}</small></div>
        <div><dt>Tool calls</dt><dd data-testid="activity-tool-count">{data.tool_calls_supported ? formatTokens(data.tool_calls_total) : "—"}</dd><small>{data.tool_calls_supported ? `${data.tools.length} tools observed` : "Not reported by source"}</small></div>
        <div><dt>Data quality</dt><dd className={qualityWarning ? "unknown-cost" : "known-cost"}>{qualityWarning ? "Review" : "Clean"}</dd><small>{formatTokens(data.quality.parse_error_entries)} malformed · {formatTokens(data.quality.dedup_skipped_entries)} deduped</small></div>
      </dl>

      <div className="activity-grid">
        <section className="work-pane tool-evidence" aria-labelledby="tools-title">
          <header className="pane-heading"><div><h2 id="tools-title">Tool calls</h2><p>Call frequency only; no token cost is assigned to tools.</p></div><span>{data.tool_calls_supported ? `${data.tool_calls_total} total` : "Unsupported"}</span></header>
          {!data.tool_calls_supported ? (
            <div className="evidence-note"><strong>This source does not expose tool-call records.</strong><p>Turn tokens remain available below. ccstats will not infer tools from transcript text.</p></div>
          ) : data.tools.length === 0 ? (
            <div className="evidence-note"><strong>No tool calls in this window.</strong><p>Choose a wider period if you expected tool activity.</p></div>
          ) : (
            <ol className="tool-list">
              {data.tools.map((tool) => (
                <li key={tool.name}>
                  <div><strong>{tool.name}</strong><span>{formatTokens(tool.calls)} · {((tool.calls / data.tool_calls_total) * 100).toFixed(1)}%</span></div>
                  <i><span style={{ width: `${tool.calls / maxCalls * 100}%` }} /></i>
                </li>
              ))}
            </ol>
          )}
        </section>

        <aside className="work-pane evidence-method" aria-label="Evidence method">
          <header className="pane-heading"><div><h2>Method</h2><p>What this page can prove.</p></div></header>
          <dl>
            <div><dt>Turn identity</dt><dd>Session + message ID</dd></div>
            <div><dt>Streaming</dt><dd>Completed entry wins</dd></div>
            <div><dt>Tool identity</dt><dd>Session + message + tool-use ID</dd></div>
            <div><dt>Attribution limit</dt><dd>Tool tokens remain unknown</dd></div>
          </dl>
        </aside>
      </div>

      <section className="work-pane table-pane turn-table" aria-labelledby="turns-title">
        <header className="pane-heading"><div><h2 id="turns-title">Recent model turns</h2><p>Newest first, capped at 100 rows.</p></div><span>{data.turns.length} shown</span></header>
        {data.turns.length === 0 ? (
          <div className="evidence-note"><strong>No usage-bearing turns in this window.</strong><p>Run the source once or choose a wider period.</p></div>
        ) : (
          <div className="table-scroll">
            <table>
              <thead><tr><th>Turn</th><th>Model</th><th>Input</th><th>Output</th><th>Cache</th><th>Reasoning</th><th>Total</th></tr></thead>
              <tbody>
                {data.turns.map((turn, index) => (
                  <tr key={`${turn.session_id}:${turn.message_id ?? turn.timestamp}:${index}`}>
                    <td className="identity-cell turn-identity"><strong>{localTimestamp(turn.timestamp)}</strong><small>{turn.session_id || "No session ID"}{turn.project_path ? ` · ${turn.project_path}` : ""}</small></td>
                    <td>{turn.model || "Unknown"}{turn.model_call_count > 1 ? <small className="call-badge">{turn.model_call_count} calls</small> : null}</td>
                    <td>{formatTokens(turn.tokens.input_tokens)}</td>
                    <td>{formatTokens(turn.tokens.output_tokens)}</td>
                    <td>{formatTokens(turn.tokens.cache_read_tokens + turn.tokens.cache_creation_tokens)}</td>
                    <td>{formatTokens(turn.tokens.reasoning_tokens)}</td>
                    <td className="known-cost">{formatTokens(turn.tokens.total_tokens)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </section>
  );
}
