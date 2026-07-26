import { useEffect, useState } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

import "./App.css";
import { Toaster } from "@/components/ui/sonner";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ConnectionBar, type ConnectionStatus } from "@/components/ckm/ConnectionBar";
import { DiscoveryConfirm } from "@/components/ckm/DiscoveryConfirm";
import { StatsPanel } from "@/components/ckm/StatsPanel";
import { PushPanel } from "@/components/ckm/PushPanel";
import { PullPanel } from "@/components/ckm/PullPanel";
import { TerminalPanel } from "@/components/ckm/TerminalPanel";
import { AuditPanel } from "@/components/ckm/AuditPanel";
import { WorldLibraryPanel } from "@/components/ckm/WorldLibraryPanel";
import { DisclaimerDialog } from "@/components/ckm/DisclaimerDialog";
import type { AppConfig, DiscoveryResult } from "@/lib/api";
import * as api from "@/lib/api";
import { OperationsProvider } from "@/lib/operations";

function App() {
  const { t } = useTranslation();
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

  async function handleConnect(profileId: string, password?: string) {
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
    <OperationsProvider>
    <main className="flex h-screen flex-col bg-background text-foreground">
      <Toaster />
      <DisclaimerDialog />
      <h1 className="px-4 pt-4 text-lg font-semibold">{t("app.title")}</h1>

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
        <div className="p-4 text-sm text-muted-foreground">{t("app.loading")}</div>
      )}

      <Tabs defaultValue="server" className="flex min-h-0 flex-1 flex-col gap-3 p-4">
        <TabsList>
          <TabsTrigger value="server">{t("app.tabs.server")}</TabsTrigger>
          <TabsTrigger value="audit">{t("app.tabs.audit")}</TabsTrigger>
        </TabsList>

        <TabsContent value="server" className="flex min-h-0 flex-1 flex-col gap-4">
          {status === "connected-raw" && discoveryResult && selectedProfileId && (
            <DiscoveryConfirm
              profileId={selectedProfileId}
              result={discoveryResult}
              onRescan={handleRescan}
              onConfirmed={handleConfirmed}
            />
          )}

          {status === "connected-target" && (
            <Tabs defaultValue="stats" className="flex min-h-0 flex-1 flex-col gap-3">
              <TabsList>
                <TabsTrigger value="stats">{t("app.tabs.stats")}</TabsTrigger>
                <TabsTrigger value="library">{t("app.tabs.library")}</TabsTrigger>
                <TabsTrigger value="push">{t("app.tabs.push")}</TabsTrigger>
                <TabsTrigger value="pull">{t("app.tabs.pull")}</TabsTrigger>
                <TabsTrigger value="terminal">{t("app.tabs.terminal")}</TabsTrigger>
              </TabsList>
              <TabsContent value="stats" className="flex min-h-0 flex-1 flex-col">
                <StatsPanel />
              </TabsContent>
              <TabsContent value="library" className="flex min-h-0 flex-1 flex-col">
                <WorldLibraryPanel />
              </TabsContent>
              <TabsContent value="push" className="flex min-h-0 flex-1 flex-col">
                <PushPanel />
              </TabsContent>
              <TabsContent value="pull" className="flex min-h-0 flex-1 flex-col">
                <PullPanel />
              </TabsContent>
              <TabsContent value="terminal" keepMounted className="flex min-h-0 flex-1 flex-col">
                <TerminalPanel />
              </TabsContent>
            </Tabs>
          )}

          {status === "disconnected" && (
            <Card className="w-full">
              <CardHeader>
                <CardTitle>{t("app.noConnection.title")}</CardTitle>
              </CardHeader>
              <CardContent className="text-sm text-muted-foreground">
                {t("app.noConnection.body")}
              </CardContent>
            </Card>
          )}
        </TabsContent>

        <TabsContent value="audit" className="flex min-h-0 flex-1 flex-col">
          <AuditPanel />
        </TabsContent>
      </Tabs>
    </main>
    </OperationsProvider>
  );
}

export default App;
