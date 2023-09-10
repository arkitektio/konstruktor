import { motion } from "framer-motion";
import { cn } from "../utils";
import { AppMenu } from "../components/AppMenu";
import { ScrollArea } from "../components/ui/scroll-area";

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
    <div className="h-screen w-screen bg-back-900 text-white w-screen flex h-full overflow-y-hidden flex flex-col relative">
      <div className="h-[5%]">{menu || <AppMenu />}</div>
      <ScrollArea className={cn("flex flex-col p-6 h-[90%]", className)}>
        {children}
      </ScrollArea>

      <div className="absolute w-full bottom-0 flex-initial flex flex-row-reverse gap-2 p-3 bg-card h-[10%] border-t border-foreground">
        {buttons}
      </div>
    </div>
  );
};
