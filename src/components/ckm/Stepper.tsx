import { CheckIcon, LoaderCircleIcon, XIcon } from "lucide-react";

import { cn } from "@/lib/utils";

// Shared by PushPanel/PullPanel: both drive a linear pipeline from backend progress
// events, differing only in which states exist and what "it broke" looks like.
//
// SPEC §12 names this the signature element — a horizontal pipeline that fills as the
// operation runs, with a live readout of the current stage. One simplification from the
// spec's literal "each stage showing its live journal line": only the *current* stage's
// detail is surfaced (in the readout below the pipeline), not all of them at once — once
// a stage completes there's nothing live left to show for it anyway.

type StepStatus = "pending" | "active" | "done" | "error";

export interface StepDef<S extends string> {
  state: S;
  label: string;
}

export interface ProgressEntry<S extends string> {
  state: S;
  detail?: string | null;
  ok?: boolean | null;
}

function computeStatus(index: number, currentIdx: number, terminal: "none" | "done" | "failed"): StepStatus {
  if (terminal === "done") return "done";
  if (terminal === "failed") {
    if (index < currentIdx) return "done";
    if (index === currentIdx) return "error";
    return "pending";
  }
  if (index < currentIdx) return "done";
  if (index === currentIdx) return "active";
  return "pending";
}

function nodeClasses(status: StepStatus): string {
  switch (status) {
    case "done":
      return "border-signal bg-signal text-signal-foreground";
    case "active":
      return "border-signal text-signal";
    case "error":
      return "border-destructive bg-destructive text-destructive-foreground";
    default:
      return "border-muted-foreground/30 text-muted-foreground";
  }
}

function trackClasses(leftStatus: StepStatus): string {
  return leftStatus === "pending" ? "bg-border" : leftStatus === "error" ? "bg-destructive" : "bg-signal";
}

function StepGlyph({ status }: { status: StepStatus }) {
  if (status === "done") return <CheckIcon className="size-3.5" />;
  if (status === "error") return <XIcon className="size-3.5" />;
  if (status === "active") return <LoaderCircleIcon className="size-3.5 motion-safe:animate-spin" />;
  return null;
}

export function Stepper<S extends string>({
  steps,
  log,
  doneState,
  failedStates,
  failedBanner,
}: {
  steps: StepDef<S>[];
  log: ProgressEntry<S>[];
  doneState: S;
  failedStates: S[];
  failedBanner: string;
}) {
  const lastEntry = log[log.length - 1];
  // currentIdx must track the last *pipeline* step reached, not just the last log
  // entry — on failure the log ends with rollingBack/failed, which aren't in `steps`
  // and would otherwise make every step register as "pending" instead of "done".
  const lastMainEntry = [...log].reverse().find((l) => steps.some((s) => s.state === l.state));
  const currentIdx = lastMainEntry ? steps.findIndex((s) => s.state === lastMainEntry.state) : -1;
  const terminal: "none" | "done" | "failed" =
    lastEntry?.state === doneState
      ? "done"
      : log.some((l) => failedStates.includes(l.state))
        ? "failed"
        : "none";

  const statuses = steps.map((_, i) => computeStatus(i, currentIdx, terminal));
  const showReadout = terminal === "failed" || (terminal === "none" && currentIdx >= 0);
  const readoutLabel = terminal === "failed" ? failedBanner : (currentIdx >= 0 ? steps[currentIdx].label : "");
  const readoutDetail = lastEntry?.detail;

  return (
    <div className="rounded-md border p-3">
      <div className="flex items-start overflow-x-auto pb-1">
        {steps.map((step, i) => (
          <div key={step.state} className={cn("flex items-center", i < steps.length - 1 && "flex-1")}>
            <div className="flex flex-col items-center gap-1.5">
              <div
                className={cn(
                  "flex size-7 shrink-0 items-center justify-center rounded-full border-2 transition-colors",
                  nodeClasses(statuses[i])
                )}
              >
                <StepGlyph status={statuses[i]} />
              </div>
              <span
                className={cn(
                  "w-16 text-center text-[11px] leading-tight",
                  statuses[i] === "pending" ? "text-muted-foreground" : "text-foreground"
                )}
              >
                {step.label}
              </span>
            </div>
            {i < steps.length - 1 && (
              <div
                className={cn("mx-1 h-0.5 flex-1 self-start mt-3.5 rounded-full transition-colors", trackClasses(statuses[i]))}
              />
            )}
          </div>
        ))}
      </div>

      {showReadout && (
        <div
          className={cn(
            "mt-3 rounded-md border p-2 font-mono text-xs",
            terminal === "failed" ? "border-destructive/40 bg-destructive/5" : "border-signal/30 bg-signal/5"
          )}
        >
          <div
            className={cn(
              "font-sans text-[11px] font-medium tracking-wide uppercase",
              terminal === "failed" ? "text-destructive" : "text-signal"
            )}
          >
            {readoutLabel}
          </div>
          {readoutDetail && (
            <div className="mt-1 max-h-40 overflow-y-auto overflow-x-hidden whitespace-pre-wrap text-foreground">
              {readoutDetail}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
