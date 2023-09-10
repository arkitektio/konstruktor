import { DialogTrigger } from "@radix-ui/react-dialog";
import { useCommand } from "../hooks/useCommand";
import { Logo } from "../layout/Logo";
import { useSettings } from "../settings/settings-context";
import {
  Menubar,
  MenubarCheckboxItem,
  MenubarContent,
  MenubarItem,
  MenubarMenu,
  MenubarRadioGroup,
  MenubarRadioItem,
  MenubarSeparator,
  MenubarShortcut,
  MenubarSub,
  MenubarSubContent,
  MenubarSubTrigger,
  MenubarTrigger,
} from "./ui/menubar";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "./ui/dialog";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "./ui/select";
import { useCommunication } from "../communication/communication-context";

export const LogoMenu = () => {
  return (
    <MenubarMenu>
      <MenubarTrigger>
        {" "}
        <div className="mr-2">
          <Logo
            width={"25"}
            height={"25"}
            cubeColor={"hsl(var(--accent)"}
            aColor={"hsl(var(--foreground)"}
            strokeColor={"hsl(var(--foreground)"}
          />
        </div>
        Konstruktor
      </MenubarTrigger>
      <MenubarContent>
        <MenubarItem>
          Settings <MenubarShortcut>⌘T</MenubarShortcut>
        </MenubarItem>
        <MenubarItem>
          New Window <MenubarShortcut>⌘N</MenubarShortcut>
        </MenubarItem>
      </MenubarContent>
    </MenubarMenu>
  );
};

export const SettingsMenu = () => {
  const { status } = useCommunication();
  const { settings, setSettings } = useSettings();
  return (
    <MenubarMenu>
      <MenubarTrigger>Settings</MenubarTrigger>
      <MenubarContent>
        <Dialog>
          <DialogTrigger>
            <MenubarItem onSelect={(e) => e.preventDefault()}>
              Open Settings <MenubarShortcut>⌘T</MenubarShortcut>
            </MenubarItem>
          </DialogTrigger>
          <DialogContent>
            <DialogTitle>Settings</DialogTitle>
            <DialogDescription>
              These are global settings for this installer. They won't apply to
              your deployments.
              <div className="grid grid-cols-2 gap-2 w-[180px] mt-5">
                <div className="my-auto">Theme</div>
                <Select
                  onValueChange={(v) => setSettings({ ...settings, theme: v })}
                  value={settings.theme}
                >
                  <SelectTrigger>
                    <SelectValue placeholder="Select a Theme" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      <SelectItem value="dark">Dark</SelectItem>
                      <SelectItem value="light">Light</SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </div>
              {status ? (
                <div className="grid grid-cols-2 gap-2 w-[180px] mt-5">
                  <div className="col-span-2">Connected to Docker</div>
                  <div className="font-light">Version</div>
                  <div className="font-light">{status?.version}</div>
                  <div className="font-light">Memory</div>
                  <div className="font-light">
                    {status?.memory &&
                      (status.memory / 1000000000).toPrecision(4)}{" "}
                    GB
                  </div>
                </div>
              ) : (
                <div className="grid grid-cols-2 gap-2 w-[180px] mt-5">
                  <div className="col-span-2">
                    Not connected to Docker. Is docker running?
                  </div>
                </div>
              )}
            </DialogDescription>
          </DialogContent>
        </Dialog>
      </MenubarContent>
    </MenubarMenu>
  );
};

export const AppMenu = () => {
  return (
    <Menubar className="flex-initial border-0 justify-between">
      <LogoMenu />
      <SettingsMenu />
    </Menubar>
  );
};
