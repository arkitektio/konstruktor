import { cn } from "../utils";
import { AppMenu } from "../components/AppMenu";
import { ScrollArea } from "../components/ui/scroll-area";

/**
 * The frame every screen outside the wizard runs inside.
 *
 * Laid out the same way `WizardPage` is — fixed menu, one scrolling middle that takes
 * whatever is left, one footer of actions — so a screen and a wizard step sit their
 * content at the same place on the window. The bands used to be 5% / 90% / 10% of the
 * viewport, which put the footer over the content whenever the window was short.
 */
export const Page = ({
  children,
  className,
  buttons,
  menu,
}: {
  children: React.ReactNode;
  className?: string;
  menu?: React.ReactNode;
  buttons?: React.ReactNode;
}) => {
  return (
    <div className="@container h-screen w-screen flex flex-col overflow-hidden bg-radial-[at_100%_100%] from-background to-backgroundpaired text-foreground">
      <div className="shrink-0">{menu || <AppMenu />}</div>

      <ScrollArea className={cn("flex-1 min-h-0", className)}>
        <div className="@container px-6 py-6">{children}</div>
      </ScrollArea>

      {buttons && (
        <div className="shrink-0 flex flex-row-reverse items-center gap-2 px-6 py-3 border-t border-border bg-card/60 backdrop-blur">
          {buttons}
        </div>
      )}
    </div>
  );
};
