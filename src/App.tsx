import { useEffect, useState } from "react";
import { toast } from "sonner";

import "./App.css";
import { Toaster } from "@/components/ui/sonner";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ConnectionBar, type ConnectionStatus } from "@/components/ckm/ConnectionBar";
import { DiscoveryConfirm } from "@/components/ckm/DiscoveryConfirm";
import { StatsPanel } from "@/components/ckm/StatsPanel";
import type { AppConfig, DiscoveryResult } from "@/lib/api";
import * as api from "@/lib/api";

function App() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [selectedProfileId, setSelectedProfileId] = useState<string | null>(null);
  const [status, setStatus] = useState<ConnectionStatus>("disconnected");
  const [discoveryResult, setDiscoveryResult] = useState<DiscoveryResult | null>(null);

  useEffect(() => {
    api
      .getConfig()
      .then((cfg) => {
        setConfig(cfg);
        setSelectedProfileId(cfg.activeProfileId ?? cfg.profiles[0]?.id ?? null);
      })
      .catch((e) => toast.error(String(e)));
  }, []);

  function selectProfile(id: string) {
    setSelectedProfileId(id);
    api.setActiveProfile(id).catch(() => {});
  }

  async function handleConnect(profileId: string, password: string) {
    setStatus("connecting");
    try {
      const result = await api.connect(profileId, password);
      if (result.needsDiscovery) {
        setStatus("connected-raw");
        const discovery = await api.runDiscovery();
        setDiscoveryResult(discovery);
      } else {
        setStatus("connected-target");
      }
    } catch (e) {
      setStatus("disconnected");
      throw e;
    }
  }

  async function handleDisconnect() {
    try {
      await api.disconnect();
    } finally {
      setStatus("disconnected");
      setDiscoveryResult(null);
    }
  }

  async function handleRescan() {
    try {
      const discovery = await api.runDiscovery();
      setDiscoveryResult(discovery);
    } catch (e) {
      toast.error(String(e));
    }
  }

  async function handleConfirmed() {
    const cfg = await api.getConfig();
    setConfig(cfg);
    setDiscoveryResult(null);
    setStatus("connected-target");
  }

  return (
    <main className="flex h-screen flex-col bg-background text-foreground">
      <Toaster />
      <h1 className="px-4 pt-4 text-lg font-semibold">Core Keeper World Manager</h1>

      {config ? (
        <ConnectionBar
          config={config}
          selectedProfileId={selectedProfileId}
          onSelectProfile={selectProfile}
          onConfigChanged={setConfig}
          status={status}
          onConnect={handleConnect}
          onDisconnect={handleDisconnect}
        />
      ) : (
        <div className="p-4 text-sm text-muted-foreground">Loading…</div>
      )}

      <div className="flex min-h-0 flex-1 flex-col gap-4 p-4">
        {status === "connected-raw" && discoveryResult && selectedProfileId && (
          <DiscoveryConfirm
            profileId={selectedProfileId}
            result={discoveryResult}
            onRescan={handleRescan}
            onConfirmed={handleConfirmed}
          />
        )}

        {status === "connected-target" && <StatsPanel />}

        {status === "disconnected" && (
          <Card className="w-full">
            <CardHeader>
              <CardTitle>No active connection</CardTitle>
            </CardHeader>
            <CardContent className="text-sm text-muted-foreground">
              Pick or add a profile above, then Connect. First connect to a profile
              triggers the discovery probe automatically.
            </CardContent>
          </Card>
        )}
      </div>
    </main>
  );
}

export default App;
