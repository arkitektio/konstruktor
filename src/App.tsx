import React, { useState } from "react";
import { BrowserRouter, Route, Routes } from "react-router-dom";
import { BeaconContext } from "./beacon/context";
import { BeaconProvider } from "./beacon/provider";
import { CommunicationProvider } from "./communication/communication-provider";
import { HealthProvider } from "./health/health-provider";
import { Dashboard, DashboardScreen } from "./screens/Dashboard";
import { Home } from "./screens/Home";
import { Setup } from "./screens/wizard/Setup";
import { StorageProvider } from "./storage/storage-provider";
import { Command } from "@tauri-apps/api/shell";
import { BindingsProvider } from "./interface/provider";
import { LogScreen } from "./screens/LogScreen";
import { SettingsProvider } from "./settings/settings-provider";
import { Settings } from "./screens/Settings";

function App() {
  return (
    <CommunicationProvider>
      <BindingsProvider>
        <BeaconProvider>
          <SettingsProvider>
            <StorageProvider>
              <div className="h-screen w-screen bg-back-900 text-white w-screen flex h-full overflow-y-hidden">
                <BrowserRouter>
                  <Routes>
                    <Route path="/" element={<Home />} />
                    <Route path="/settings" element={<Settings />} />
                    <Route path="/setup" element={<Setup />} />
                    <Route
                      path="/dashboard/:id"
                      element={<DashboardScreen />}
                    />
                    <Route path="/logs/:id" element={<LogScreen />} />
                    <Route
                      path="/logs/:id/service/:service"
                      element={<LogScreen />}
                    />
                  </Routes>
                </BrowserRouter>
              </div>
            </StorageProvider>
          </SettingsProvider>
        </BeaconProvider>
      </BindingsProvider>
    </CommunicationProvider>
  );
}

export default App;
