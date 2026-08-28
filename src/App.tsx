import "./globals.css";
import { Route, Routes, useLocation } from "react-router-dom";
import { CommunicationProvider } from "./communication/communication-provider";
import { DashboardScreen } from "./screens/Dashboard";
import { Home } from "./screens/Home";
import { LogScreen } from "./screens/LogScreen";
import { Settings } from "./screens/Settings";
import { SettingsProvider } from "./settings/settings-provider";
import { RegistryProvider } from "./registry/registry-provider";
import { TooltipProvider } from "./components/ui/tooltip";
import { AlerterProvider } from "./alerter/alert-provider";
import { AlerterDialog } from "./alerter/AlerterDialog";
import { HubWizard } from "./screens/deploy/HubWizard";
import { ConnectScreen } from "./screens/deploy/ConnectScreen";

function App() {
  const location = useLocation();

  return (
    <CommunicationProvider>
        <AlerterProvider>
          <AlerterDialog />
          <TooltipProvider>
            <SettingsProvider>
              <RegistryProvider>
                <Routes location={location} key={location.pathname}>
                  <Route path="/" element={<Home />} />
                  <Route path="/settings" element={<Settings />} />
                  <Route path="/new" element={<HubWizard />} />
                  <Route path="/new/hub" element={<HubWizard />} />
                  <Route path="/dashboard/:id" element={<DashboardScreen />} />
                  <Route path="/connect/:id" element={<ConnectScreen />} />
                  <Route path="/logs/:id" element={<LogScreen />} />
                  <Route
                    path="/logs/:id/service/:service"
                    element={<LogScreen />}
                  />
                  <Route path="*" element={<Home />} />
                </Routes>
              </RegistryProvider>
            </SettingsProvider>
          </TooltipProvider>
        </AlerterProvider>
    </CommunicationProvider>
  );
}

export default App;
