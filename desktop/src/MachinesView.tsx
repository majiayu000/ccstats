import { useCallback, useEffect, useState } from "react";
import type { ChangeEvent } from "react";
import type { DesktopBridge, MachineRollup, SourceDescriptor } from "./bridge";
import { errorMessage, formatCost, formatTokens } from "./format";

const MAX_BUNDLE_BYTES = 10 * 1024 * 1024;

export function MachinesView({ bridge, sources }: { bridge: DesktopBridge; sources: SourceDescriptor[] }) {
  const [rollup, setRollup] = useState<MachineRollup | null>(null);
  const [machineName, setMachineName] = useState("This machine");
  const [loading, setLoading] = useState(true);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const next = await bridge.machineRollup();
      setRollup(next);
      setMachineName(next.local_machine_name ?? "This machine");
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setLoading(false);
    }
  }, [bridge]);

  useEffect(() => { void load(); }, [load]);

  async function capture() {
    setWorking(true);
    setError(null);
    try {
      const findings = await bridge.sourceDiagnostics();
      const ready = new Set(findings.filter((row) => (row.status === "detected" || row.status === "configured")).map((row) => row.name));
      const names = sources.filter((source) => source.name !== "all" && ready.has(source.name)).map((source) => source.name);
      if (names.length === 0) throw new Error("No registered sources are available to capture.");
      setRollup(await bridge.saveMachineSnapshot(machineName, await bridge.usageOverviews(names)));
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setWorking(false);
    }
  }

  async function exportBundle() {
    setWorking(true);
    setError(null);
    try {
      const content = await bridge.exportMachineBundle();
      const url = URL.createObjectURL(new Blob([content], { type: "application/json" }));
      const link = document.createElement("a");
      link.href = url;
      link.download = "ccstats-machine-snapshots.json";
      link.click();
      URL.revokeObjectURL(url);
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setWorking(false);
    }
  }

  async function importBundle(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    setWorking(true);
    setError(null);
    try {
      if (file.size > MAX_BUNDLE_BYTES) throw new Error("Machine snapshot bundle must be 10 MB or smaller.");
      setRollup(await bridge.importMachineBundle(await file.text()));
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setWorking(false);
    }
  }

  if (loading) return <section className="loading-state" aria-live="polite"><div><i /><i /><i /></div><strong>Opening machine snapshots…</strong><span>Local SQLite store</span></section>;
  if (!rollup && error) return <section className="state-pane error-state" role="alert"><span aria-hidden="true">!</span><div><h2>Could not open machine snapshots.</h2><p>{error}</p><button type="button" onClick={() => void load()}>Try again</button></div></section>;
  if (!rollup) return null;

  const currency = rollup.currency ?? "USD";
  const hasStaleRange = [rollup.today_current_machines, rollup.week_current_machines, rollup.month_current_machines].some((count) => count < rollup.machines.length);
  return (
    <section className="machines-workbench" aria-labelledby="machines-title">
      <header className="work-pane pane-heading machines-heading"><div><h2 id="machines-title">Machine rollup</h2><p>Persist aggregate snapshots locally, then exchange one JSON file between devices.</p></div><span>T {rollup.today_current_machines}/{rollup.machines.length} · W {rollup.week_current_machines}/{rollup.machines.length} · M {rollup.month_current_machines}/{rollup.machines.length}</span></header>
      {error ? <p className="inline-warning" role="alert">{error}</p> : null}
      {hasStaleRange ? <p className="inline-warning" role="status">Stale ranges remain visible below but are excluded from their current totals. Capture and exchange a fresh snapshot to include them.</p> : null}
      <div className="machine-totals"><article className="work-pane"><span>Today</span><strong data-testid="machines-today">{formatTokens(rollup.totals.today_tokens)}</strong><small>{formatCost(rollup.totals.today_cost, currency)}</small></article><article className="work-pane"><span>This week</span><strong>{formatTokens(rollup.totals.week_tokens)}</strong><small>{formatCost(rollup.totals.week_cost, currency)}</small></article><article className="work-pane"><span>This month</span><strong data-testid="machines-month">{formatTokens(rollup.totals.month_tokens)}</strong><small>{formatCost(rollup.totals.month_cost, currency)}</small></article></div>
      <section className="work-pane machine-actions" aria-label="Machine snapshot controls"><label>Local machine name<input value={machineName} maxLength={128} onChange={(event) => setMachineName(event.target.value)} /></label><button type="button" disabled={working || machineName.trim().length === 0} onClick={() => void capture()}>Capture this machine</button><button type="button" disabled={working} onClick={() => void exportBundle()}>Export snapshots</button><label className="file-action">Import snapshots<input type="file" accept="application/json,.json" disabled={working} aria-label="Import machine snapshots" onChange={(event) => void importBundle(event)} /></label></section>
      <p className="limit-note">Machine costs are combined in canonical USD. Only complete costs backed by recorded, live, or current cached pricing contribute; lower bounds and review-only prices remain unknown.</p>
      {rollup.machines.length === 0 ? <section className="state-pane empty-state"><span aria-hidden="true">○</span><div><h2>No machine snapshots yet.</h2><p>Capture this machine first, then import snapshot files from your other devices.</p></div></section> : <section className="work-pane table-pane machine-table"><div className="table-scroll"><table><thead><tr><th>Machine</th><th>Current ranges</th><th>Captured</th><th>Sources</th><th>Month tokens</th><th>Month cost</th></tr></thead><tbody>{rollup.machines.map((machine) => <tr key={machine.machine_id}><td className="identity-cell">{machine.machine_name}{machine.is_local ? <small>Local</small> : null}</td><td><span className={`diagnostic-status status-${machine.today_current && machine.week_current && machine.month_current ? "detected" : "missing"}`}>T {machine.today_current ? "✓" : "—"} · W {machine.week_current ? "✓" : "—"} · M {machine.month_current ? "✓" : "—"}</span></td><td>{new Date(machine.captured_at_ms).toLocaleString()}</td><td>{machine.source_count}</td><td>{formatTokens(machine.totals.month_tokens)}</td><td className={machine.totals.month_cost === null ? "unknown-cost" : "known-cost"}>{formatCost(machine.totals.month_cost, machine.currency ?? currency)}</td></tr>)}</tbody></table></div></section>}
    </section>
  );
}
