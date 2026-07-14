interface Props {
  seconds: number;
  title: string;
  description: string;
  statusLabel: string;
  secondsLabel: string;
  undoLabel: string;
  onUndo: () => void;
}

export function SendCountdownDialog({ seconds, title, description, statusLabel, secondsLabel, undoLabel, onUndo }: Props) {
  return (
    <div className="send-countdown-overlay">
      <div
        className="send-countdown-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-live="assertive"
        aria-labelledby="send-countdown-title"
        aria-describedby="send-countdown-description"
      >
        <div className="send-countdown-status" aria-hidden="true">
          <span className="send-countdown-status-dot" />
          {statusLabel}
        </div>
        <h2 id="send-countdown-title">{title}</h2>
        <div className="send-countdown-number" aria-label={`${seconds} ${secondsLabel}`}>
          {seconds}
        </div>
        <div className="send-countdown-unit" aria-hidden="true">
          {secondsLabel}
        </div>
        <p id="send-countdown-description">{description}</p>
        <button className="send-countdown-undo" onClick={onUndo} autoFocus>
          <span aria-hidden="true">↩</span>
          {undoLabel}
        </button>
      </div>
    </div>
  );
}
