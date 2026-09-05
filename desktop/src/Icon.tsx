const paths: Record<string, string> = {
  overview: "M3 3h7v7H3z M14 3h7v7h-7z M3 14h7v7H3z M14 14h7v7h-7z",
  live: "M2 12h4l3-8 5 16 3-8h5",
  top: "M5 20V12 M12 20V4 M19 20V8",
  activity: "M4 5h16 M4 12h10 M4 19h13 M19 10v5 M16.5 12.5h5",
  projects: "M3 7V5a1 1 0 0 1 1-1h5l2 3h9a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z",
  history: "M3 11a9 9 0 1 1 2.5 7 M3 4v7h7 M12 7v5l3 2",
  trust: "M12 3 4 6v6c0 4 4 7 8 9 4-2 8-5 8-9V6z M8 12l3 3 5-6",
  shield: "M12 3 4 6v6c0 4 4 7 8 9 4-2 8-5 8-9V6z M8 12l3 3 5-6",
  limits: "M4 18a9 9 0 1 1 16 0 M12 13l4-5 M5 18h14",
  diagnostics: "M4 7h16 M4 17h16 M8 4v6 M16 14v6",
  machines: "M3 4h18v13H3z M8 21h8 M12 17v4",
  refresh: "M20 7a8 8 0 0 0-14-1L3 9 M3 3v6h6 M4 17a8 8 0 0 0 14 1l3-3 M21 21v-6h-6",
  arrow: "M5 12h14 M14 7l5 5-5 5",
};

export function Icon({ name }: { name: string }) {
  return <svg className="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={paths[name]} /></svg>;
}
