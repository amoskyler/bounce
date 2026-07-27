/**
 * Icons, drawn inline.
 *
 * These are traced to match Signal's icon set: a 20px grid, 1.7px strokes with
 * round caps and joins, and `currentColor` throughout so a single CSS rule
 * recolours them for hover and theme.
 *
 * Inlining rather than loading a sprite keeps the renderer free of any network
 * or filesystem fetch, which matters under Electron's content security policy.
 */

import * as React from 'react';

import { DELIVERY_LABELS, type DeliveryState } from './delivery';
import { encodeQr } from './qrcode';

type IconProps = {
  size?: number;
  className?: string;
};

function svgProps(size: number) {
  return {
    width: size,
    height: size,
    viewBox: '0 0 20 20',
    fill: 'none',
    stroke: 'currentColor',
    strokeWidth: 1.7,
    strokeLinecap: 'round' as const,
    strokeLinejoin: 'round' as const,
    'aria-hidden': true,
  };
}

export function SearchIcon({ size = 16, className }: IconProps) {
  return (
    <svg {...svgProps(size)} className={className}>
      <circle cx="8.75" cy="8.75" r="5.25" />
      <path d="M12.6 12.6 16.5 16.5" />
    </svg>
  );
}

export function ComposeIcon({ size = 20, className }: IconProps) {
  return (
    <svg {...svgProps(size)} className={className}>
      <path d="M14.1 3.4a1.9 1.9 0 0 1 2.7 2.7L7.6 15.3l-3.6.9.9-3.6z" />
      <path d="M12.9 4.6 15.6 7.3" />
    </svg>
  );
}

export function NewGroupIcon({ size = 20, className }: IconProps) {
  return (
    <svg {...svgProps(size)} className={className}>
      <circle cx="7.6" cy="7" r="3.1" />
      <path d="M2.4 16.2a5.4 5.4 0 0 1 10.4 0" />
      <path d="M13.6 4.4a3.1 3.1 0 0 1 0 5.6" />
      <path d="M15 12.1a5.4 5.4 0 0 1 2.8 4.1" />
    </svg>
  );
}

export function MoreIcon({ size = 20, className }: IconProps) {
  return (
    <svg {...svgProps(size)} className={className} strokeWidth={0} fill="currentColor">
      <circle cx="10" cy="4.6" r="1.55" />
      <circle cx="10" cy="10" r="1.55" />
      <circle cx="10" cy="15.4" r="1.55" />
    </svg>
  );
}

export function CloseIcon({ size = 20, className }: IconProps) {
  return (
    <svg {...svgProps(size)} className={className}>
      <path d="M5.5 5.5 14.5 14.5" />
      <path d="M14.5 5.5 5.5 14.5" />
    </svg>
  );
}

export function InfoIcon({ size = 20, className }: IconProps) {
  return (
    <svg {...svgProps(size)} className={className}>
      <circle cx="10" cy="10" r="7.4" />
      <path d="M10 9.2v4.4" />
      <circle cx="10" cy="6.4" r="0.95" fill="currentColor" strokeWidth={0} />
    </svg>
  );
}

/**
 * A cog, not a sun.
 *
 * The obvious way to draw settings is a circle with spokes radiating from it,
 * which is also exactly how you draw a sun — and at 20px, with the spokes
 * detached from the body, the sun is what people see. The teeth here are
 * joined to the rim as one closed path so the shape reads as machinery.
 */
export function SettingsIcon({ size = 20, className }: IconProps) {
  return (
    <svg {...svgProps(size)} className={className}>
      <path d="M8.5 2.6h3l.35 2.03a5.6 5.6 0 0 1 1.4.81l1.93-.73 1.5 2.6-1.58 1.3a5.6 5.6 0 0 1 0 1.62l1.58 1.3-1.5 2.6-1.93-.73a5.6 5.6 0 0 1-1.4.81L11.5 17.4h-3l-.35-2.03a5.6 5.6 0 0 1-1.4-.81l-1.93.73-1.5-2.6 1.58-1.3a5.6 5.6 0 0 1 0-1.62l-1.58-1.3 1.5-2.6 1.93.73a5.6 5.6 0 0 1 1.4-.81z" />
      <circle cx="10" cy="10" r="2.4" />
    </svg>
  );
}

export function AttachIcon({ size = 20, className }: IconProps) {
  return (
    <svg {...svgProps(size)} className={className}>
      <path d="M15.4 9.6 9.9 15a3.4 3.4 0 0 1-4.8-4.8l6-6a2.3 2.3 0 0 1 3.2 3.2l-6 6a1.1 1.1 0 0 1-1.6-1.6l5.2-5.2" />
    </svg>
  );
}

/** The composer's attachment button. */
export function PlusIcon({ size = 20, className }: IconProps) {
  return (
    <svg {...svgProps(size)} className={className} strokeWidth={1.9}>
      <path d="M10 4.6v10.8M4.6 10h10.8" />
    </svg>
  );
}

/** "Photos & Videos": a picture frame with a horizon and a sun. */
export function MediaIcon({ size = 20, className }: IconProps) {
  return (
    <svg {...svgProps(size)} className={className}>
      <rect x="2.6" y="3.8" width="14.8" height="12.4" rx="2.2" />
      <circle cx="7.2" cy="8.1" r="1.35" />
      <path d="M3 13.9 6.9 10.6a1.4 1.4 0 0 1 1.85 0l2.9 2.6 1.6-1.35a1.4 1.4 0 0 1 1.85 0L17.4 13.6" />
    </svg>
  );
}

/** "File": a document with a folded corner. */
export function DocumentIcon({ size = 20, className }: IconProps) {
  return (
    <svg {...svgProps(size)} className={className}>
      <path d="M11.4 2.8H6.4a1.8 1.8 0 0 0-1.8 1.8v10.8a1.8 1.8 0 0 0 1.8 1.8h7.2a1.8 1.8 0 0 0 1.8-1.8V6.8z" />
      <path d="M11.4 2.8v3.2a.8.8 0 0 0 .8.8h3.2" />
    </svg>
  );
}

export function EmojiIcon({ size = 20, className }: IconProps) {
  return (
    <svg {...svgProps(size)} className={className}>
      <circle cx="10" cy="10" r="7.4" />
      <path d="M7 11.6a3.6 3.6 0 0 0 6 0" />
      <circle cx="7.5" cy="8.1" r="0.9" fill="currentColor" strokeWidth={0} />
      <circle cx="12.5" cy="8.1" r="0.9" fill="currentColor" strokeWidth={0} />
    </svg>
  );
}

/* --------------------------------------------------------------------------
 * Delivery status
 *
 * These four are the only icons drawn on a 12-unit grid rather than the 20-unit
 * one above, because that is the grid Signal draws them on and the geometry is
 * unusually tight: a 1.1px ring on a 5.45 radius, with a check whose arms are
 * placed to sit optically centred rather than geometrically. Rounding those
 * numbers onto the shared grid is visible at 12px.
 *
 * Signal ships them as filled paths produced by a boolean subtraction. Redrawn
 * here as rings and strokes so the pair glyphs can be composed from the single
 * one and nothing has to be kept in sync by hand.
 * -------------------------------------------------------------------------- */

/** The ring's radius and weight, shared by every state. */
const TICK_RADIUS = 5.45;
const TICK_STROKE = 1.1;

/** The check inside one dial, as a centreline through three points. */
const TICK_CHECK = 'M3.75 6.25 5.25 8.25 8 4';

/**
 * The filled dial is drawn a little smaller than the outlined one.
 *
 * Signal's is 5.75 against the ring's 6.0 outer edge. Solid shapes read heavier
 * than outlines at the same diameter, and matching them numerically makes the
 * read pair look like it grew.
 */
const TICK_FILLED_RADIUS = 5.75;

/**
 * How much of the left dial the right one eats.
 *
 * Both are the dial's own radius plus a one-unit gap — 6.0 + 1 for the ring,
 * 5.75 + 1 for the disc — which is what separates the two shapes instead of
 * letting them touch. Recovered from the endpoints of Signal's own arcs; they
 * ship the subtraction already applied, so the numbers are not written down
 * anywhere in their source either.
 */
const TICK_BITE_OUTLINE = 7;
const TICK_BITE_FILLED = 6.75;

/** A mask id that is safe to interpolate into `url(#…)`. */
function useMaskId(): string {
  return `tick-${React.useId().replace(/:/g, '')}`;
}

function tickProps(size: number, width: number) {
  return {
    width: (size * width) / 12,
    height: size,
    viewBox: `0 0 ${width} 12`,
    fill: 'none',
    stroke: 'currentColor',
    strokeWidth: TICK_STROKE,
    strokeLinecap: 'round' as const,
    strokeLinejoin: 'round' as const,
    'aria-hidden': true,
  };
}

/** One dial: a ring with a check in it, centred on `cx`. */
function TickDial({ cx }: { cx: number }) {
  return (
    <g transform={`translate(${cx - 6} 0)`}>
      <circle cx="6" cy="6" r={TICK_RADIUS} />
      <path d={TICK_CHECK} />
    </g>
  );
}

/**
 * Outgoing status: queued, not yet acknowledged by anybody.
 *
 * A dashed ring that turns. Twelve dashes at this radius come out at 2.85 units
 * of arc each, so the dash and gap below divide that; drawing it as one dashed
 * circle rather than twelve arcs is what lets the rotation be a single
 * transform on a single element.
 */
export function SendingIcon({ size = 12, className }: IconProps) {
  const step = (2 * Math.PI * TICK_RADIUS) / 12;
  // The round caps add a stroke-width to each dash on top of this, which is
  // most of what is visible: the drawn centreline is barely longer than a point.
  const dash = 0.42;

  return (
    <svg {...tickProps(size, 12)} className={`tick-spin${className ? ` ${className}` : ''}`}>
      <circle cx="6" cy="6" r={TICK_RADIUS} strokeDasharray={`${dash} ${step - dash}`} />
    </svg>
  );
}

/**
 * Outgoing status: held by a device of the recipient's that is not their
 * client — an encrypted storage device.
 *
 * Unreachable until encrypted devices land (P24). It exists now because it is
 * the state the glyph set is *for*: one dial means somebody has the bytes, two
 * means the person does. Leaving the middle rung out and adding it later would
 * silently change what a single check had meant.
 */
export function SentIcon({ size = 12, className }: IconProps) {
  return (
    <svg {...tickProps(size, 12)} className={className}>
      <TickDial cx={6} />
    </svg>
  );
}

/**
 * Where the right dial's bite crosses the left dial's ring.
 *
 * Standard two-circle intersection, with the centres six apart on the same
 * line. Computed rather than written down because the three radii above are the
 * numbers worth being able to change, and hand-solving this again each time is
 * how a ring ends up not quite meeting its own crescent.
 */
function biteCrossing(radius: number, bite: number): { x: number; y: number } {
  const separation = 6;
  const x = (separation ** 2 - bite ** 2 + radius ** 2) / (2 * separation);
  return { x: 6 + x, y: Math.sqrt(Math.max(0, radius ** 2 - x ** 2)) };
}

/** Outgoing status: acknowledged by a recipient's own device. */
export function DeliveredIcon({ size = 12, className }: IconProps) {
  // An arc rather than a masked circle. A mask here would inherit this svg's
  // stroke onto its own children and leak a faint ghost of the whole ring —
  // and the arc is what Signal ships anyway, with the subtraction pre-applied.
  const cut = biteCrossing(TICK_RADIUS, TICK_BITE_OUTLINE);

  return (
    <svg {...tickProps(size, 18)} className={className}>
      {/* The crescent: from the upper crossing, anticlockwise round the far
          side, to the lower one. */}
      <path
        d={`M${cut.x} ${6 - cut.y}A${TICK_RADIUS} ${TICK_RADIUS} 0 1 0 ${cut.x} ${6 + cut.y}`}
      />
      {/* All that survives of the left check is its short arm. */}
      <path d="M3.75 6.25 5.25 8.25" />
      <TickDial cx={12} />
    </svg>
  );
}

/**
 * Outgoing status: read.
 *
 * The same pair, filled. Signal's one departure from stroking: the check is
 * knocked *out* of the disc rather than drawn on it, so it reads at 12px where
 * a light-on-dark stroke would close up.
 */
export function ReadIcon({ size = 12, className }: IconProps) {
  const maskId = useMaskId();

  // A mask per disc, not one for the pair: the bite belongs to the left disc
  // alone, and a shared mask would take the same crescent out of the right one.
  // Every child spells out its own paint. Mask content inherits from the svg it
  // is declared in, and an inherited stroke on the white rect would put a rim of
  // partial luminance around the whole glyph.
  const knockout = (check: string, bite: boolean) => (
    <>
      <rect width="18" height="12" fill="#fff" stroke="none" />
      {bite && <circle cx="12" cy="6" r={TICK_BITE_FILLED} fill="#000" stroke="none" />}
      <path
        d={TICK_CHECK}
        transform={check}
        stroke="#000"
        strokeWidth={TICK_STROKE}
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="none"
      />
    </>
  );

  return (
    <svg {...tickProps(size, 18)} className={className} fill="currentColor" stroke="none">
      <mask id={`${maskId}-l`}>{knockout('translate(0 0)', true)}</mask>
      <mask id={`${maskId}-r`}>{knockout('translate(6 0)', false)}</mask>
      <circle cx="6" cy="6" r={TICK_FILLED_RADIUS} mask={`url(#${maskId}-l)`} />
      <circle cx="12" cy="6" r={TICK_FILLED_RADIUS} mask={`url(#${maskId}-r)`} />
    </svg>
  );
}

/** Outgoing status: delivery abandoned. */
export function UndeliverableIcon({ size = 12, className }: IconProps) {
  return (
    <svg {...tickProps(size, 12)} className={className}>
      <circle cx="6" cy="6" r={TICK_RADIUS} />
      <path d="M6 3.4v3" />
      <circle cx="6" cy="8.6" r="0.62" fill="currentColor" strokeWidth={0} />
    </svg>
  );
}

const TICK_GLYPHS: Record<DeliveryState, (props: IconProps) => React.JSX.Element> = {
  sending: SendingIcon,
  sent: SentIcon,
  delivered: DeliveredIcon,
  read: ReadIcon,
  undeliverable: UndeliverableIcon,
};

/**
 * The tick for one delivery state, wherever it appears.
 *
 * The bubble and the sidebar row both show one, and they have to agree — a
 * message whose row says delivered and whose bubble says sending is worse than
 * either being wrong on its own. Keeping the mapping in one table is what makes
 * that impossible rather than merely unlikely.
 */
export function DeliveryTick({
  state,
  className,
  size = 12,
}: {
  state: DeliveryState;
  className?: string;
  size?: number;
}) {
  const Glyph = TICK_GLYPHS[state];
  const label = DELIVERY_LABELS[state];

  return (
    <span
      className={`tick tick--${state}${className ? ` ${className}` : ''}`}
      title={label}
      aria-label={label}
      role="img"
    >
      <Glyph size={size} />
    </span>
  );
}

/** The floating control that returns the timeline to its newest message. */
export function JumpToBottomIcon({ size = 20, className }: IconProps) {
  return (
    <svg {...svgProps(size)} className={className}>
      <path d="M10 4.2v11" />
      <path d="M5.4 10.6 10 15.2l4.6-4.6" />
    </svg>
  );
}

/** Modules of light space around a symbol. Four is the specified minimum. */
const QUIET_ZONE = 4;

/**
 * A string as a QR code, drawn inline.
 *
 * The Fyne client shows the pairing code this way so the person beside you can
 * point a phone at it. The colours are fixed rather than themed: a reader
 * expects dark modules on a light field, and an inverted code is one more thing
 * for a camera to get wrong in a dim room.
 *
 * The whole symbol is one path — a run of adjacent dark modules becomes a
 * single rectangle — because a few thousand sibling `<rect>` elements is a real
 * cost to lay out, and this is drawn inside a dialog that opens instantly.
 */
export function QrCode({
  text,
  size = 200,
  className,
}: {
  text: string;
  size?: number;
  className?: string;
}) {
  const symbol = React.useMemo(() => {
    if (!text) return null;
    try {
      return encodeQr(text);
    } catch {
      // Nothing a pairing code can hit — it is 89 characters against a limit of
      // 2,331 — and a missing square is better than a broken dialog.
      return null;
    }
  }, [text]);

  if (!symbol) return null;

  const side = symbol.size + QUIET_ZONE * 2;
  const parts: string[] = [];

  for (let y = 0; y < symbol.size; y += 1) {
    const row = symbol.modules[y];
    let x = 0;
    while (x < symbol.size) {
      if (!row[x]) {
        x += 1;
        continue;
      }
      let run = 1;
      while (x + run < symbol.size && row[x + run]) run += 1;
      parts.push(`M${x + QUIET_ZONE} ${y + QUIET_ZONE}h${run}v1h-${run}z`);
      x += run;
    }
  }

  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${side} ${side}`}
      className={className}
      role="img"
      aria-label="Pairing code as a QR code"
    >
      <rect width={side} height={side} fill="#ffffff" />
      <path d={parts.join('')} fill="#000000" shapeRendering="crispEdges" />
    </svg>
  );
}

/**
 * The Bounce mark.
 *
 * Taken verbatim from `ui/assets/icon.svg` — the same artwork the Fyne client
 * and the application icon use — rather than redrawn. Two builds of the same
 * product showing two different logos is the kind of divergence nobody files
 * and everybody notices.
 *
 * Unlike every other icon in this file it ignores `currentColor`: it is a
 * three-colour mark built from gradients, and recolouring it would make it
 * something else.
 */
export function BounceLogo({ size = 64, className }: IconProps) {
  // The gradients are referenced by id and the mark can appear more than once
  // on a page — the empty state and the first-run screen both use it. Fixed
  // ids would collide, and every reference in the document would resolve to
  // whichever definition happened to be parsed first.
  const id = React.useId().replace(/:/g, '');

  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 400 381.737"
      className={className}
      aria-hidden
    >
      <defs>
        {/* The arc. */}
        <linearGradient
          id={`${id}-arc`}
          gradientUnits="userSpaceOnUse"
          x1="165.1056"
          y1="220.5238"
          x2="165.1056"
          y2="0"
        >
          <stop offset="0" stopColor="#40937C" />
          <stop offset="1" stopColor="#41D26F" />
        </linearGradient>

        {/* The bowl it falls into. */}
        <linearGradient
          id={`${id}-bowl`}
          gradientUnits="userSpaceOnUse"
          x1="190.8685"
          y1="381.7371"
          x2="190.8685"
          y2="122.1453"
        >
          <stop offset="0" stopColor="#3260E6" />
          <stop offset="1" stopColor="#4D82FF" />
        </linearGradient>

        {/* The ball. */}
        <linearGradient
          id={`${id}-ball`}
          gradientUnits="userSpaceOnUse"
          x1="320.4202"
          y1="129.3371"
          x2="386.3282"
          y2="63.4291"
        >
          <stop offset="0" stopColor="#F19A23" />
          <stop offset="1" stopColor="#FFC41C" />
        </linearGradient>
      </defs>

      <path
        fill={`url(#${id}-arc)`}
        d="M185.395,216.223c0.761,2.573,3.075,4.301,5.756,4.301c2.657,0,4.948-1.712,5.701-4.259 c12.063-40.796,38.377-76.309,74.093-99.999l2.135-1.416l-0.496-2.514c-1.038-5.268-1.565-10.635-1.565-15.953 c0-22.45,8.925-43.452,25.13-59.138l3.433-3.323l-3.992-2.626C264.462,10.822,228.249,0,190.868,0 c-31.292,0-62.328,7.746-89.753,22.401C74.574,36.583,51.43,57.164,34.185,81.918l-3.558,5.107l6.163,0.871 C106.933,97.814,165.264,148.185,185.395,216.223z"
      />
      <path
        fill={`url(#${id}-bowl)`}
        d="M381.737,190.869c0-3.851-0.137-7.895-0.408-12.019l-0.313-4.762l-4.581,1.339 c-7.516,2.197-15.275,3.311-23.061,3.311c-23.881,0-46.583-10.414-62.285-28.573l-2.292-2.651l-2.843,2.05 c-28.468,20.534-48.567,50.545-56.594,84.503c-4.142,17.524-19.863,29.762-38.229,29.762c-18.374,0-34.091-12.221-38.221-29.719 c-15.105-63.994-71.616-110.003-137.424-111.886l-2.748-0.079l-0.953,2.578C4.1,145.526,0.136,167.393,0.003,189.715 c-0.299,50.299,19.449,98.03,55.605,134.401c17.906,18.013,38.787,32.185,62.063,42.124c24.081,10.283,49.539,15.496,75.666,15.496 h177.574h9.242l-6.535-6.535l-48.617-48.617c17.273-17.058,30.957-36.918,40.701-59.085 C376.342,243.295,381.737,217.513,381.737,190.869z"
      />
      <circle fill={`url(#${id}-ball)`} cx="353.374" cy="96.383" r="46.626" />
    </svg>
  );
}
