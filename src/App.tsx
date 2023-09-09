import "./globals.css";
import { BrowserRouter, Route, Routes, useLocation } from "react-router-dom";
import { BeaconProvider } from "./beacon/provider";
import { CommunicationProvider } from "./communication/communication-provider";
import { BindingsProvider } from "./interface/provider";
import { DashboardScreen } from "./screens/Dashboard";
import { Home } from "./screens/Home";
import { LogScreen } from "./screens/LogScreen";
import { Settings } from "./screens/Settings";
import { Setup } from "./screens/wizard/Setup";
import { SettingsProvider } from "./settings/settings-provider";
import { StorageProvider } from "./storage/storage-provider";
import { AppMenu } from "./components/AppMenu";
import { AnimatePresence } from "framer-motion";

function App() {

  const location = useLocation();

  return (
    <CommunicationProvider>
      <BindingsProvider>
        <BeaconProvider>
          <SettingsProvider>
            <StorageProvider>
              <div className="h-screen w-screen bg-back-900 text-white w-screen flex h-full overflow-y-hidden flex flex-col">
                
                <AppMenu/>
                  <Routes  location={location} key={location.pathname}>
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
              </div>
            </StorageProvider>
          </SettingsProvider>
        </BeaconProvider>
      </BindingsProvider>
    </CommunicationProvider>
  );
}

export default App;
