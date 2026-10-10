/** The status bar: the number, and where it came from.
 *
 * The config path is shown because a count that changed because of a file the
 * user did not open is the single most confusing thing this tool can do. Saying
 * which `.sephera.toml` was read is the whole defence.
 */
export function StatusBar({
  summary,
  config,
  busy,
}: {
  summary: string | null;
  config: string | null;
  busy: boolean;
}) {
  return (
    <footer className="workbench__status">
      {busy ? <span aria-live="polite">Working…</span> : null}
      <span>{summary ?? "ready"}</span>
      {config ? (
        <span className="workbench__spacer">config: {config}</span>
      ) : (
        <span className="workbench__spacer" />
      )}
    </footer>
  );
}
