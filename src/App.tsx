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

function App() {
  return (
    <CommunicationProvider>
      <BindingsProvider>
        <BeaconProvider>
          <StorageProvider>
            <div className="h-screen bg-back-900 text-white w-screen flex @container h-full overflow-y-hidden">
              <BrowserRouter>
                <Routes>
                  <Route path="/" element={<Home />} />
                  <Route path="/setup" element={<Setup />} />
                  <Route path="/dashboard/:id" element={<DashboardScreen />} />
                  <Route path="/logs/:id" element={<LogScreen />} />
                </Routes>
              </BrowserRouter>
            </div>
          </StorageProvider>
        </BeaconProvider>
      </BindingsProvider>
    </CommunicationProvider>
  );
}

export default App;
