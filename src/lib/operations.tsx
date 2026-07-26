import { createContext, useCallback, useContext, useRef, useState } from "react";
import type { ReactNode } from "react";
import { toast } from "sonner";

import type {
  LocalWorldCandidate,
  PullProgress,
  PulledWorld,
  PushProgress,
  PushResult,
  RestoreProgress,
  RestoreResult,
} from "@/lib/api";
import * as api from "@/lib/api";

// Push/pull/restore run inside one Tauri command invocation each — the actual swap
// keeps going on the backend for as long as that command runs, regardless of which tab
// is visible. But before this provider existed, the progress-event listener and the
// running/log/result state lived *inside* PushPanel/PullPanel/WorldLibraryPanel — plain
// components rendered only while their tab was active. Base UI's Tabs unmounts inactive
// panels by default, and Tauri events aren't replayed to a listener that wasn't there
// when they fired, so switching away and back mid-operation silently dropped every
// event from that point on and made the whole thing look like it vanished.
//
// Fix: this provider is mounted once, for the app's whole lifetime, at the App level —
// above the tabs, not inside them. The listener and the log/result state live here, so
// they survive tab switches; the panels become thin views over this context.
//
// `activeOperation` also doubles as the cross-tab "something destructive is already
// running" lock the UI uses to disable the other two triggers (and the Disconnect
// button) — see ConnectionBar/TerminalPanel. The backend enforces the same rule itself
// (see `commands.rs`'s `BUSY` / `try_lock`), so this is the friendly first line, not the
// only one.

export type OperationKind = "push" | "pull" | "restore";

interface PushOpState {
  running: boolean;
  log: PushProgress[];
  result: PushResult | null;
  error: string | null;
}

interface PullOpState {
  running: boolean;
  log: PullProgress[];
  result: PulledWorld | null;
  error: string | null;
}

interface RestoreOpState {
  running: boolean;
  log: RestoreProgress[];
  result: RestoreResult | null;
  error: string | null;
}

const emptyPush: PushOpState = { running: false, log: [], result: null, error: null };
const emptyPull: PullOpState = { running: false, log: [], result: null, error: null };
const emptyRestore: RestoreOpState = { running: false, log: [], result: null, error: null };

interface OperationsContextValue {
  /** Which of push/pull/restore is currently running, if any — the cross-tab lock. */
  activeOperation: OperationKind | null;
  push: PushOpState;
  pull: PullOpState;
  restore: RestoreOpState;
  startPush: (candidate: LocalWorldCandidate) => Promise<void>;
  startPull: (slot: number) => Promise<void>;
  startRestore: (backupPath: string, slot: number) => Promise<void>;
  resetPush: () => void;
  resetPull: () => void;
  resetRestore: () => void;
}

const OperationsContext = createContext<OperationsContextValue | null>(null);

export function useOperations(): OperationsContextValue {
  const ctx = useContext(OperationsContext);
  if (!ctx) {
    throw new Error("useOperations() must be used inside <OperationsProvider>");
  }
  return ctx;
}

export function OperationsProvider({ children }: { children: ReactNode }) {
  const [activeOperation, setActiveOperation] = useState<OperationKind | null>(null);
  const [push, setPush] = useState<PushOpState>(emptyPush);
  const [pull, setPull] = useState<PullOpState>(emptyPull);
  const [restore, setRestore] = useState<RestoreOpState>(emptyRestore);

  // `activeOperation` state updates are async (batched); the start* functions need the
  // *current* value synchronously to refuse overlapping starts, hence the ref mirror.
  const activeRef = useRef<OperationKind | null>(null);
  const setActive = useCallback((kind: OperationKind | null) => {
    activeRef.current = kind;
    setActiveOperation(kind);
  }, []);

  const startPush = useCallback(
    async (candidate: LocalWorldCandidate) => {
      if (activeRef.current) {
        toast.error("Another operation is already in progress.");
        return;
      }
      setActive("push");
      setPush({ running: true, log: [], result: null, error: null });
      const unlisten = await api.onPushProgress((p) =>
        setPush((prev) => ({ ...prev, log: [...prev.log, p] }))
      );
      try {
        const r = await api.pushWorld(candidate);
        setPush((prev) => ({ ...prev, running: false, result: r }));
        toast.success(`World ${r.slot} swapped in.`);
      } catch (e) {
        setPush((prev) => ({ ...prev, running: false, error: String(e) }));
        toast.error(String(e));
      } finally {
        unlisten();
        setActive(null);
      }
    },
    [setActive]
  );

  const startPull = useCallback(
    async (slot: number) => {
      if (activeRef.current) {
        toast.error("Another operation is already in progress.");
        return;
      }
      setActive("pull");
      setPull({ running: true, log: [], result: null, error: null });
      const unlisten = await api.onPullProgress((p) =>
        setPull((prev) => ({ ...prev, log: [...prev.log, p] }))
      );
      try {
        const r = await api.pullWorld(slot);
        setPull((prev) => ({ ...prev, running: false, result: r }));
        toast.success(`World ${r.slot} downloaded — pick where to save it.`);
      } catch (e) {
        setPull((prev) => ({ ...prev, running: false, error: String(e) }));
        toast.error(String(e));
      } finally {
        unlisten();
        setActive(null);
      }
    },
    [setActive]
  );

  const startRestore = useCallback(
    async (backupPath: string, slot: number) => {
      if (activeRef.current) {
        toast.error("Another operation is already in progress.");
        return;
      }
      setActive("restore");
      setRestore({ running: true, log: [], result: null, error: null });
      const unlisten = await api.onRestoreProgress((p) =>
        setRestore((prev) => ({ ...prev, log: [...prev.log, p] }))
      );
      try {
        const r = await api.restoreBackup(backupPath, slot);
        setRestore((prev) => ({ ...prev, running: false, result: r }));
        toast.success(`World ${r.slot} restored from backup.`);
      } catch (e) {
        setRestore((prev) => ({ ...prev, running: false, error: String(e) }));
        toast.error(String(e));
      } finally {
        unlisten();
        setActive(null);
      }
    },
    [setActive]
  );

  const resetPush = useCallback(() => setPush(emptyPush), []);
  const resetPull = useCallback(() => setPull(emptyPull), []);
  const resetRestore = useCallback(() => setRestore(emptyRestore), []);

  return (
    <OperationsContext.Provider
      value={{
        activeOperation,
        push,
        pull,
        restore,
        startPush,
        startPull,
        startRestore,
        resetPush,
        resetPull,
        resetRestore,
      }}
    >
      {children}
    </OperationsContext.Provider>
  );
}
