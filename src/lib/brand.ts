import type { CSSProperties } from "react";

/**
 * The brand hue and chroma, and how they get onto the page.
 *
 * Mirrors `kontrol/src/lib/brand.ts` — the same constants, the same scale, the same
 * `--brand-chroma-user` discipline — so a hue set in either application produces the
 * same colour in both. The storage keys match too, but that is house style rather than
 * a channel: the two run on different origins, so neither reads the other's
 * localStorage, and Kontrol's real preference lives per-membership on the coordination
 * server. Carrying a colour across would mean asking that server for it, which nothing
 * here does.
 *
 * This is deliberately not wired into `next-themes`, which Kontrol uses. That package
 * is there to survive prerendering and hydration, neither of which a Tauri window does;
 * `settings-provider` already owns the `light`/`dark` class on <html>, and a second
 * writer of the same class is how those two end up fighting.
 */

/** The neutral brand hue (Arkitekt violet) used when nothing else applies. */
export const DEFAULT_BRAND_HUE = 267.256;

/**
 * The neutral brand chroma, matching `--brand-chroma-user`'s fallback in `globals.css`.
 * Dark mode damps this further on its own, so everything here is the light-mode scale.
 */
export const DEFAULT_BRAND_CHROMA = 0.19;

export const MIN_BRAND_CHROMA = 0;

/**
 * Chroma is clamped to the sRGB-safe range: oklch above ~0.3 clips on most displays, so
 * a picker that ran to 1 would spend most of its travel producing the same colour.
 *
 * NOTE: the pre-paint script in index.html cannot import this — it runs before the
 * bundle — so it hardcodes the same ceiling. Change both.
 */
export const MAX_BRAND_CHROMA = 0.3;

/** The keys the pre-paint script reads. Named as Kontrol names them, but local. */
export const BRAND_HUE_KEY = "arkitekt-brand-hue";
export const BRAND_CHROMA_KEY = "arkitekt-brand-chroma";

/** Keep a chroma inside the displayable range, whatever it came from. */
export const clampChroma = (chroma: number) =>
  Math.min(MAX_BRAND_CHROMA, Math.max(MIN_BRAND_CHROMA, chroma));

/**
 * Push the brand values onto <html> and remember them.
 *
 * Writes the *knob* (`--brand-chroma-user`), never `--brand-chroma` itself: an inline
 * property on <html> outranks the `.dark` rule that lives there too, so writing the
 * derived value directly would flatten dark mode's damping. A non-finite value is
 * dropped rather than written — an invalid custom property makes the dependent `calc()`
 * invalid at computed-value time, which blanks the palette instead of falling back.
 */
export const applyBrand = ({
  hue,
  chroma,
}: {
  hue?: number | null;
  chroma?: number | null;
}) => {
  const root = document.documentElement;

  if (hue != null && Number.isFinite(hue)) {
    root.style.setProperty("--brand-hue", String(hue));
    try {
      localStorage.setItem(BRAND_HUE_KEY, String(hue));
    } catch {
      /* localStorage unavailable */
    }
  }

  if (chroma != null && Number.isFinite(chroma)) {
    const clamped = clampChroma(chroma);
    root.style.setProperty("--brand-chroma-user", String(clamped));
    try {
      localStorage.setItem(BRAND_CHROMA_KEY, String(clamped));
    } catch {
      /* localStorage unavailable */
    }
  }
};

/** A swatch of the brand colour itself, for previews that sit outside the theme. */
export const brandSwatch = (hue: number, chroma: number = DEFAULT_BRAND_CHROMA) =>
  `oklch(0.62 ${clampChroma(chroma)} ${hue})`;

/**
 * Inline style that scopes the brand values to one subtree, re-tinting anything inside
 * it that reads the tokens — the logo cube included. Used by previews that have to show
 * a colour other than the one the window is wearing.
 */
export const hueStyle = (
  hue: number | null | undefined,
  chroma?: number | null
): CSSProperties =>
  ({
    ["--brand-hue"]: String(hue ?? DEFAULT_BRAND_HUE),
    ["--brand-chroma-user"]: String(clampChroma(chroma ?? DEFAULT_BRAND_CHROMA)),
  }) as CSSProperties;

/**
 * Forget a picked colour, so the stock palette applies again.
 *
 * Removes the keys rather than writing today's defaults into them: a stored
 * `267.256` would go on overriding the stylesheet forever, and a later change to what
 * "the Arkitekt default" means would never reach anyone who had once pressed Reset.
 */
export const clearBrand = () => {
  const root = document.documentElement;
  root.style.removeProperty("--brand-hue");
  root.style.removeProperty("--brand-chroma-user");
  try {
    localStorage.removeItem(BRAND_HUE_KEY);
    localStorage.removeItem(BRAND_CHROMA_KEY);
  } catch {
    /* localStorage unavailable */
  }
};
