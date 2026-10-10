import { useBusy } from "@/hooks/useAnalysis";
import { useClient } from "@/state/store";

import styles from "@/styles/modal.module.scss";

/**
 * The modal shown while a count runs.
 *
 * A window covering everything, with the label in the middle and no way through
 * except Cancel -- which is what stops a second press of `Count` from starting a
 * run the first one has already answered. The overlay is a real element rather
 * than a `disabled` attribute on every button: a disabled button that stays
 * enabled after one missed state is a button that gets clicked.
 *
 * `useBusy` is read once here rather than per view, so the dialog covers all
 * three analyses and the file panel at once, and there is one place the label
 * and the button live.
 *
 * The Cancel button calls the store's own `cancel`, which bumps the host's epoch
 * and closes the dialog at once. It does not stop the work -- `sephera_core`'s
 * `Progress` has no cancel, and adding one to a published crate for one caller is
 * not a trade worth making -- it marks the reply stale, and the host drops it
 * when it arrives.
 */
export function ProgressModal({ label }: { label: string }) {
  const busy = useBusy();
  const cancel = useClient((state) => state.cancel);

  if (!busy) return null;

  return (
    <div className={styles.modal} role="dialog" aria-label={label}>
      <div className={styles.modal__backdrop} aria-hidden="true" />
      <div className={styles.modal__box}>
        <span className={styles.modal__spinner} aria-hidden="true" />
        <span className={styles.modal__label}>{label}</span>
        <button
          type="button"
          className={styles.modal__cancel}
          onClick={() => cancel()}
        >
          Cancel
        </button>
      </div>
    </div>
  );
}
