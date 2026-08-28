import { ArrowLeft, Cog, Monitor, Moon, Palette, Sun } from "lucide-react";
import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { Button } from "../components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../components/ui/card";
import { Input } from "../components/ui/input";
import { Slider } from "../components/ui/slider";
import { Page } from "../layout/Page";
import { PageHeader, SectionHeading } from "../layout/PageHeader";
import { StepField } from "../screens/wizard/StepFrame";
import {
  applyBrand,
  brandSwatch,
  clearBrand,
  clampChroma,
  DEFAULT_BRAND_CHROMA,
  DEFAULT_BRAND_HUE,
  MAX_BRAND_CHROMA,
} from "../lib/brand";
import {
  DEFAULT_COORDINATION_SERVER,
  useSettings,
  type Theme,
} from "../settings/settings-context";
import { cn } from "../utils";

/**
 * There is little to configure: Konstruktor builds the deployment itself, so the only
 * things worth remembering between sessions are which coordination server to offer first
 * and how the window should look.
 *
 * The appearance controls mirror Kontrol's — the same hue and chroma, on the same scale,
 * with the same defaults — so a colour picked here and a colour picked there are the same
 * colour. The preference itself stays in this installer; nothing syncs. See
 * `src/lib/brand.ts`.
 */

const HUE_GRADIENT =
  "linear-gradient(to right, hsl(0 70% 50%), hsl(60 70% 50%), hsl(120 70% 50%), hsl(180 70% 50%), hsl(240 70% 50%), hsl(300 70% 50%), hsl(360 70% 50%))";

/** Grey → full-strength ramp at the hue in play, so the track previews itself. */
const chromaGradient = (hue: number) =>
  `linear-gradient(to right, ${brandSwatch(hue, 0)}, ${brandSwatch(
    hue,
    MAX_BRAND_CHROMA / 2
  )}, ${brandSwatch(hue, MAX_BRAND_CHROMA)})`;

const THEMES: { value: Theme; label: string; icon: React.ComponentType<{ className?: string }> }[] =
  [
    { value: "light", label: "Light", icon: Sun },
    { value: "dark", label: "Dark", icon: Moon },
    { value: "system", label: "System", icon: Monitor },
  ];

const ThemeCard = () => {
  const { settings, setSettings } = useSettings();

  return (
    <Card>
      <CardHeader className="flex flex-row items-center gap-3 space-y-0">
        <div className="flex size-10 items-center justify-center rounded-lg bg-muted">
          <Sun className="size-5" />
        </div>
        <div className="space-y-1">
          <CardTitle className="text-lg">Appearance</CardTitle>
          <CardDescription>
            Light, dark, or whatever this machine is set to.
          </CardDescription>
        </div>
      </CardHeader>
      <CardContent>
        <div className="grid grid-cols-3 gap-2">
          {THEMES.map(({ value, label, icon: Icon }) => (
            <button
              key={value}
              type="button"
              onClick={() => setSettings({ ...settings, theme: value })}
              className={cn(
                "flex flex-col items-center gap-2 rounded-lg border px-3 py-4 text-sm transition-colors",
                settings.theme === value
                  ? "border-primary bg-primary/10 text-foreground"
                  : "border-border text-muted-foreground hover:bg-accent/50"
              )}
            >
              <Icon className="size-5" />
              {label}
            </button>
          ))}
        </div>
      </CardContent>
    </Card>
  );
};

/**
 * The brand hue and chroma, previewed live while dragging.
 *
 * `applyBrand` writes straight to <html> on every move so the whole window re-tints as
 * the slider travels; the value is only persisted on save. Reset goes through
 * `clearBrand`, which removes the properties instead of writing today's defaults into
 * them — see the note there.
 */
const BrandCard = () => {
  const { settings, setSettings } = useSettings();

  const [hue, setHue] = useState(settings.brandHue ?? DEFAULT_BRAND_HUE);
  const [chroma, setChroma] = useState(
    clampChroma(settings.brandChroma ?? DEFAULT_BRAND_CHROMA)
  );

  // Adopt the stored values once they arrive — forage answers a few frames after mount.
  useEffect(() => {
    setHue(settings.brandHue ?? DEFAULT_BRAND_HUE);
  }, [settings.brandHue]);
  useEffect(() => {
    setChroma(clampChroma(settings.brandChroma ?? DEFAULT_BRAND_CHROMA));
  }, [settings.brandChroma]);

  const picked =
    settings.brandHue !== null || settings.brandChroma !== null;
  const dirty =
    hue !== (settings.brandHue ?? DEFAULT_BRAND_HUE) ||
    chroma !== clampChroma(settings.brandChroma ?? DEFAULT_BRAND_CHROMA);

  return (
    <Card>
      <CardHeader className="flex flex-row items-center gap-3 space-y-0">
        <div className="flex size-10 items-center justify-center rounded-lg bg-muted">
          <Palette className="size-5" />
        </div>
        <div className="space-y-1">
          <CardTitle className="text-lg">Brand colour</CardTitle>
          <CardDescription>
            Tints the whole window. Same scale and defaults as the colour Kontrol keeps
            for your organization, so the two match when you set them to match — this
            one is stored here, not synced.
          </CardDescription>
        </div>
      </CardHeader>
      <CardContent className="space-y-5">
        <div className="flex items-center gap-4">
          <div
            className="size-10 shrink-0 rounded-full border"
            style={{ backgroundColor: brandSwatch(hue, chroma) }}
            aria-hidden
          />
          <div className="flex-1 space-y-4">
            <div className="space-y-2">
              <div
                className="h-3 w-full rounded-full"
                style={{ background: HUE_GRADIENT }}
                aria-hidden
              />
              <Slider
                min={0}
                max={360}
                step={1}
                value={[hue]}
                onValueChange={([value]) => {
                  setHue(value);
                  applyBrand({ hue: value });
                }}
                aria-label="Brand hue"
              />
            </div>
            <div className="space-y-2">
              <div
                className="h-3 w-full rounded-full"
                style={{ background: chromaGradient(hue) }}
                aria-hidden
              />
              <Slider
                min={0}
                max={MAX_BRAND_CHROMA}
                step={0.005}
                value={[chroma]}
                onValueChange={([value]) => {
                  setChroma(value);
                  applyBrand({ chroma: value });
                }}
                aria-label="Brand intensity"
              />
            </div>
          </div>
          <div className="flex w-10 shrink-0 flex-col gap-4 text-right text-sm tabular-nums text-muted-foreground">
            <span>{Math.round(hue)}</span>
            <span>{chroma.toFixed(2)}</span>
          </div>
        </div>
        <div className="flex gap-2">
          <Button
            disabled={!dirty}
            onClick={() =>
              setSettings({ ...settings, brandHue: hue, brandChroma: chroma })
            }
          >
            Save colour
          </Button>
          {picked && (
            <Button
              variant="outline"
              onClick={() => {
                clearBrand();
                setHue(DEFAULT_BRAND_HUE);
                setChroma(DEFAULT_BRAND_CHROMA);
                void setSettings({ ...settings, brandHue: null, brandChroma: null });
              }}
            >
              Reset
            </Button>
          )}
        </div>
      </CardContent>
    </Card>
  );
};

export const Settings = () => {
  const { settings, setSettings } = useSettings();
  const [server, setServer] = useState(settings.coordinationServer);
  const [saved, setSaved] = useState(false);

  // As above: the stored settings land after the first render.
  useEffect(() => {
    setServer(settings.coordinationServer);
  }, [settings.coordinationServer]);

  const [egress, setEgress] = useState(settings.egressEndpoint);
  const [prober, setProber] = useState(settings.proberEndpoint);

  useEffect(() => {
    setEgress(settings.egressEndpoint);
    setProber(settings.proberEndpoint);
  }, [settings.egressEndpoint, settings.proberEndpoint]);

  const save = async () => {
    await setSettings({
      ...settings,
      coordinationServer: server.trim(),
      egressEndpoint: egress.trim(),
      proberEndpoint: prober.trim(),
    });
    setSaved(true);
  };

  const dirty =
    server.trim() !== settings.coordinationServer ||
    egress.trim() !== settings.egressEndpoint ||
    prober.trim() !== settings.proberEndpoint;

  return (
    <Page
      buttons={
        <>
          <Button disabled={!dirty || server.trim() === ""} onClick={save}>
            Save
          </Button>
          <Button variant="outline" asChild>
            <Link to="/">
              <ArrowLeft className="size-3.5" />
              Back
            </Link>
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-8">
        <PageHeader
          icon={Cog}
          title="Settings"
          subtitle="Global to this installer — deployments keep their own."
        />

        <div className="max-w-xl">
          <SectionHeading>Deployment defaults</SectionHeading>
          <StepField
            label="Coordination server"
            hint="The server new hubs are offered first — where accounts, organizations and permissions live. A bare hostname is reached over https."
          >
            <Input
              id="coordination-server"
              value={server}
              onChange={(event) => {
                setServer(event.target.value);
                setSaved(false);
              }}
              placeholder={DEFAULT_COORDINATION_SERVER}
            />
          </StepField>

          <div className="flex items-center gap-3 mt-3">
            {settings.coordinationServer !== DEFAULT_COORDINATION_SERVER && (
              <Button
                variant="outline"
                size="sm"
                onClick={() => setServer(DEFAULT_COORDINATION_SERVER)}
              >
                Use {DEFAULT_COORDINATION_SERVER}
              </Button>
            )}
            {saved && !dirty && (
              <span className="text-xs text-muted-foreground">Saved.</span>
            )}
          </div>
        </div>

        <div className="max-w-xl">
          <SectionHeading hint="Off unless you fill them in. Every other request this app makes goes to your coordination server.">
            Checking what the outside can reach
          </SectionHeading>
          <div className="flex flex-col gap-5">
            <StepField
              label="Address echo"
              hint="Asked what address the internet sees this machine as, so the address step can point out which of your addresses that is. It tells whoever answers your IP, which is why there is no default."
            >
              <Input
                id="egress-endpoint"
                value={egress}
                onChange={(event) => {
                  setEgress(event.target.value);
                  setSaved(false);
                }}
                placeholder="https://example.org/ip"
              />
            </StepField>

            <StepField
              label="Reachability prober"
              hint="A service that fetches a URL you give it and reports what it got, used by Authorize to check whether this hub answers from outside. Without one nothing is checked — and only a hub that answers is ever advertised as publicly reachable."
            >
              <Input
                id="prober-endpoint"
                value={prober}
                onChange={(event) => {
                  setProber(event.target.value);
                  setSaved(false);
                }}
                placeholder="https://example.org/probe"
              />
            </StepField>
          </div>
        </div>

        <div>
          <SectionHeading hint="Local to this installer, on the same scale Kontrol uses.">
            Appearance
          </SectionHeading>
          <div className="grid gap-4 grid-cols-1 lg:grid-cols-2 max-w-4xl">
            <ThemeCard />
            <BrandCard />
          </div>
        </div>
      </div>
    </Page>
  );
};
