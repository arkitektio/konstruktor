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

function App() {
  return (
    <CommunicationProvider>
      <BeaconProvider>
        <StorageProvider>
          <div className="h-screen bg-slate-900 text-white text-justify w-screen flex @container">
            <BrowserRouter>
              <Routes>
                <Route path="/" element={<Home />} />
                <Route path="/setup" element={<Setup />} />
                <Route path="/dashboard/:id" element={<DashboardScreen />} />
              </Routes>
            </BrowserRouter>
          </div>
        </StorageProvider>
      </BeaconProvider>
    </CommunicationProvider>
  );
}

export default App;
