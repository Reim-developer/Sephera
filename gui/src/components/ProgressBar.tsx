import styles from "@/styles/progress.module.scss";

/**
 * The indeterminate bar shown while a count runs.
 *
 * Indeterminate rather than a percentage, because the analysis is a single call
 * into Rust that reports nothing until it has finished. A bar that fills on a
 * timer is a lie about progress; a bar that slides forever is the honest
 * statement of "this is taking a while and I do not know how long".
 *
 * `prefers-reduced-motion` is honoured in the stylesheet, where it belongs: a
 * client that respects the OS setting does not have to know which animations it
 * happens to be running.
 */
export function ProgressBar({ label }: { label: string }) {
  return (
    <div
      className={styles.progress}
      role="progressbar"
      aria-label={label}
      aria-busy="true"
    >
      <div className={styles.progress__bar} />
    </div>
  );
}
