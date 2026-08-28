import { ArrowRight, Boxes, Puzzle } from "lucide-react";
import { Link } from "react-router-dom";

import { AppMenu } from "../../components/AppMenu";
import { Card } from "../../components/ui/card";
import { Page } from "../../layout/Page";
import { PageHeader } from "../../layout/PageHeader";
import { cn } from "../../utils";

/**
 * The fork at the top of "New deployment": a hub, or a plugin engine.
 *
 * Two paths rather than one wizard with a switch in it. Almost nothing a hub is asked
 * applies to an engine — no services, no ports, no addresses to advertise, no mesh — so
 * a merged wizard would be mostly questions that do not apply to whichever one you
 * picked, asked before it knows which that is.
 */
const PATHS = [
  {
    to: "/new/hub",
    icon: Boxes,
    title: "Hub",
    lead: "The services an organization works on: orchestration, image and graph data, workflows, the app store — behind one gateway, registered with a coordination server.",
    detail: "A Docker Compose project of a dozen containers, in a folder you choose.",
  },
  {
    to: "/new/engine",
    icon: Puzzle,
    title: "Plugin engine",
    lead: "A machine that runs plugins. The engine sits next to Docker and starts the containers an organization installs, so the work happens here rather than on the hub.",
    detail: "One container — the deployer — with this machine's Docker socket.",
  },
] as const;

/** The two cards, so Home can offer the same fork when there is nothing to open yet. */
export const DeploymentPaths = () => (
  <div className="grid grid-cols-1 @2xl:grid-cols-2 gap-3">
    {PATHS.map((path) => (
      <Link key={path.to} to={path.to} className="block">
        <Card
          className={cn(
            "h-full gap-0 py-5 px-5 border-border transition-colors",
            "hover:border-primary/60 hover:bg-primary/5"
          )}
        >
          <span className="flex size-9 items-center justify-center rounded-lg border border-border text-primary">
            <path.icon className="size-4.5" />
          </span>
          <div className="font-semibold mt-3">{path.title}</div>
          <p className="text-sm text-muted-foreground mt-1 leading-relaxed">
            {path.lead}
          </p>
          <div className="text-xs text-muted-foreground mt-3">{path.detail}</div>
          <div className="flex items-center gap-1.5 text-sm text-primary mt-4">
            Create one
            <ArrowRight className="size-3.5" />
          </div>
        </Card>
      </Link>
    ))}
  </div>
);

export const NewDeployment = () => (
  <Page menu={<AppMenu back="/" breadcrumb="New deployment" />}>
    <div className="flex flex-col gap-6 max-w-3xl">
      <PageHeader
        title="What are you creating?"
        subtitle="Both are ordinary compose projects in a folder you choose."
      />

      <DeploymentPaths />
    </div>
  </Page>
);
