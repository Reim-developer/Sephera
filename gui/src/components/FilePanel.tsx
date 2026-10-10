import { AlertTriangle, FileText } from "lucide-react";


import { ProgressBar } from "@/components/ProgressBar";
import { useFile } from "@/hooks/useAnalysis";
import { useClient } from "@/state/store";
import styles from "@/styles/panel.module.scss";

/**
 * The per-file panel: everything about the one file that is selected.
 *
 * This is the DataGrip half of the layout -- a result grid on one side and the
 * record it came from on the other. The grid stays put and the panel replaces its
 * contents, which is what makes clicking through a directory a way of reading it
 * rather than a series of page loads.
 *
 * It reads nothing and calls nothing. The counts came from the host when the file
 * was selected, and this renders them.
 */
export function FilePanel() {
  const { data, error, busy } = useFile();
  const selected = useClient((state) => state.selected);

  if (!selected) {
    return (
      <div className={styles.panel__empty}>
        <FileText size={20} aria-hidden="true" />
        <span>Select a file</span>
        <span className={styles.panel__hint}>
          Its counts and declarations appear here.
        </span>
      </div>
    );
  }

  if (error) {
    return (
      <div className={styles.panel__error}>
        <AlertTriangle size={16} aria-hidden="true" />
        <span>{error}</span>
      </div>
    );
  }

  if (busy && !data) {
    return (
      <div className={styles.panel__busy}>
        <ProgressBar label={`Reading ${selected}`} />
      </div>
    );
  }

  if (!data) return null;

  const share = data.code ? 100 : 0;

  return (
    <div className={styles.panel}>
      <header className={styles.panel__header}>
        <h2 className={styles.panel__title} title={data.path}>
          {data.path.split("/").pop() || data.path}
        </h2>
        <p className={styles.panel__path}>{data.path}</p>
      </header>

      <dl className={styles.panel__stats}>
        <Stat label="Language" value={data.language ?? "unknown"} />
        <Stat label="Code" value={data.code.toLocaleString()} accent />
        <Stat label="Comment" value={data.comment.toLocaleString()} />
        <Stat label="Empty" value={data.empty.toLocaleString()} />
        <Stat label="Size" value={`${data.size_bytes.toLocaleString()} B`} />
        <Stat
          label="Declarations"
          value={data.declarations.length.toLocaleString()}
        />
      </dl>

      {data.declarations.length > 0 ? (
        <table className={styles.panel__table}>
          <caption className="sr-only">
            Declarations in {data.path}
          </caption>
          <thead>
            <tr>
              <th scope="col">Name</th>
              <th scope="col">Kind</th>
              <th scope="col" className={styles.numeric}>
                Line
              </th>
            </tr>
          </thead>
          <tbody>
            {data.declarations.map((declaration) => (
              <tr key={`${declaration.line}:${declaration.name}`}>
                <td>{declaration.name}</td>
                <td>{declaration.kind}</td>
                <td className={styles.numeric}>{declaration.line}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : (
        <p className={styles.panel__none}>
          No declarations found in this file.
        </p>
      )}

      <div className={styles.panel__bar}>
        <span>{share}% of this file is code</span>
      </div>
    </div>
  );
}

/** One figure in the panel's stat list. */
function Stat({
  label,
  value,
  accent,
}: {
  label: string;
  value: string;
  accent?: boolean;
}) {
  return (
    <div className={styles.panel__stat}>
      <dt>{label}</dt>
      <dd className={accent ? styles.panel__statAccent : undefined}>
        {value}
      </dd>
    </div>
  );
}
