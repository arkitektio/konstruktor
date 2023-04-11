import { useEffect, useRef } from "react";
import autoAnimate from "@formkit/auto-animate";
export type DivProps = React.DetailedHTMLProps<
  React.HTMLAttributes<HTMLDivElement>,
  HTMLDivElement
>;

export const Hover = ({ children, ...props }: DivProps) => {
  const parent = useRef<any>(null);

  useEffect(() => {
    parent.current && autoAnimate(parent.current);
  }, [parent]);
  return (
    <div
      ref={parent}
      onMouseMove={(e) => {
        if (parent.current) {
          const selectables = Array.from<HTMLElement>(
            parent.current.children as unknown as HTMLElement[]
          );

          for (const card of selectables) {
            const rect = card.getBoundingClientRect(),
              x = e.clientX - rect.left,
              y = e.clientY - rect.top;

            card.style.setProperty("--mouse-x", `${x}px`);
            card.style.setProperty("--mouse-y", `${y}px`);
          }
        }
      }}
      {...props}
    >
      {children}
    </div>
  );
};
