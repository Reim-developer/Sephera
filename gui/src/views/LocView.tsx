import { useCallback, useEffect, useState } from "react";
import type { LocView } from "../lib/ipc";
import { countLines } from "../lib/ipc";

/** The line-count view: one table, ordered the way the CLI orders it.
 *
 * The Rust side already sorted the rows by code lines and summed the totals, so
 * this renders in the order it is given rather than re-deriving either. A second
 * sort here would be a second opinion to disagree with.
 *
 * `reloadToken` is how the app asks for a recompute: a changed value means
 * "again". It is a token rather than a callback identity because the tree is
 * deep enough that a new function on every render would re-run this effect and
 * hit Rust on a state change that has nothing to do with the count.
 */
export function LocView({
  path,
  ignore,
  reloadToken,
}: {
  path: string;
  ignore: string[];
  reloadToken: number;
}) {
  const [view, setView] = useState<LocView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);

  const run = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      setView(await countLines(path, ignore));
    } catch (cause) {
      setError(String(cause));
      setView(null);
    } finally {
      setBusy(false);
    }
  }, [path, ignore]);

  useEffect(() => {
    void run();
  }, [run, reloadToken]);

  if (busy && !view) {
    return (
      <div className="placeholder">
        <span className="placeholder__title">Counting…</span>
      </div>
    );
  }
  if (error) {
    return (
      <div className="banner">
        <span className="banner__icon" aria-hidden="true">
          ⚠
        </span>
        <span>{error}</span>
      </div>
    );
  }
  if (!view) return null;

  const rows = [...view.rows].sort((left, right) => right.code - left.code);

  return (
    <div className="view">
      <header className="view__header">
        <h2 className="view__title">Lines of code</h2>
        <p className="view__subtitle">
          {view.files_scanned.toLocaleString()} files ·{" "}
          {rows.length.toLocaleString()} languages · {view.elapsed_ms} ms
        </p>
      </header>

      <table className="table">
        <thead>
          <tr>
            <th scope="col">Language</th>
            <th scope="col" className="numeric">
              Files
            </th>
            <th scope="col" className="numeric">
              Code
            </th>
            <th scope="col" className="numeric">
              Comment
            </th>
            <th scope="col" className="numeric">
              Empty
            </th>
            <th scope="col" className="numeric">
              Bytes
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.language}>
              <td>{row.language}</td>
              <td className="numeric">{row.files.toLocaleString()}</td>
              <td className="numeric">{row.code.toLocaleString()}</td>
              <td className="numeric">{row.comment.toLocaleString()}</td>
              <td className="numeric">{row.empty.toLocaleString()}</td>
              <td className="numeric">{row.size_bytes.toLocaleString()}</td>
            </tr>
          ))}
          <tr className="table__totals">
            <td>Totals</td>
            <td className="numeric">{view.totals.files.toLocaleString()}</td>
            <td className="numeric">{view.totals.code.toLocaleString()}</td>
            <td className="numeric">{view.totals.comment.toLocaleString()}</td>
            <td className="numeric">{view.totals.empty.toLocaleString()}</td>
            <td className="numeric">
              {view.totals.size_bytes.toLocaleString()}
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  );
}
