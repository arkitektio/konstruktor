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
import { TooltipProvider } from "./components/ui/tooltip";
import { AlerterProvider } from "./alerter/alert-provider";
import { AlerterContext } from "./alerter/alerter-context";
import { AlerterDialog } from "./alerter/AlerterDialog";
import { ChannelSetup, ChannelSetupPage } from "./screens/wizard/ChannelSetup";
import { RepoProvider } from "./repo/repo-provider";
import { RepoSearcher } from "./repo/RepoSearcher";

function App() {
  const location = useLocation();

  return (
    <CommunicationProvider>
      <BindingsProvider>
        <RepoProvider>
          <RepoSearcher url="https://raw.githubusercontent.com/jhnnsrs/konstruktor/master/repo/channels.json" />
          <AlerterProvider>
            <AlerterDialog />
            <BeaconProvider>
              <TooltipProvider>
                <SettingsProvider>
                  <StorageProvider>
                    <Routes location={location} key={location.pathname}>
                      <Route path="/" element={<Home />} />
                      <Route path="/settings" element={<Settings />} />
                      <Route path="/setup" element={<Setup />} />
                      <Route
                        path="/channelsetup/:name/:channel"
                        element={<ChannelSetupPage />}
                      />
                      <Route
                        path="/dashboard/:id"
                        element={<DashboardScreen />}
                      />
                      <Route path="/logs/:id" element={<LogScreen />} />
                      <Route
                        path="/logs/:id/service/:service"
                        element={<LogScreen />}
                      />
                      <Route path="*" element={<Home />} />
                    </Routes>
                  </StorageProvider>
                </SettingsProvider>
              </TooltipProvider>
            </BeaconProvider>
          </AlerterProvider>
        </RepoProvider>
      </BindingsProvider>
    </CommunicationProvider>
  );
}

export default App;
