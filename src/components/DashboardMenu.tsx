import { Logo } from "../layout/Logo";
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

export const AppMenu = () => {
  return (
    <Menubar className="flex-initial border-0 justify-between">
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
      <MenubarMenu>
        <MenubarTrigger>Settings</MenubarTrigger>
        <MenubarContent>
          <MenubarItem>
            Settings <MenubarShortcut>⌘T</MenubarShortcut>
          </MenubarItem>
          <MenubarItem>
            New Window <MenubarShortcut>⌘N</MenubarShortcut>
          </MenubarItem>
        </MenubarContent>
      </MenubarMenu>
    </Menubar>
  );
};
