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

export function SettingsIcon({ size = 20, className }: IconProps) {
  return (
    <svg {...svgProps(size)} className={className}>
      <circle cx="10" cy="10" r="2.6" />
      <path d="M10 2.4v1.9M10 15.7v1.9M17.6 10h-1.9M4.3 10H2.4M15.4 4.6l-1.4 1.4M6 14l-1.4 1.4M15.4 15.4 14 14M6 6 4.6 4.6" />
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

export function SendIcon({ size = 18, className }: IconProps) {
  return (
    <svg {...svgProps(size)} className={className} strokeWidth={0} fill="currentColor">
      <path d="M3.2 16.5 17 10 3.2 3.5l1.9 5.2 7 1.3-7 1.3z" />
    </svg>
  );
}

/** Outgoing status: queued, not yet written to any peer. */
export function SendingIcon({ size = 12, className }: IconProps) {
  return (
    <svg {...svgProps(size)} className={className} strokeWidth={1.6}>
      <circle cx="10" cy="10" r="7.6" />
      <path d="M10 5.8V10l2.8 1.7" />
    </svg>
  );
}

/** Outgoing status: written to at least one peer. */
export function SentIcon({ size = 12, className }: IconProps) {
  return (
    <svg {...svgProps(size)} className={className} strokeWidth={2}>
      <path d="M3.6 10.6 7.7 14.6 16.4 5.6" />
    </svg>
  );
}

/**
 * Outgoing status: acknowledged by a recipient's device (double tick), or read
 * by the recipient — read uses the same glyph in a brighter colour, which is
 * how Signal distinguishes them.
 */
export function DeliveredIcon({ size = 12, className }: IconProps) {
  return (
    <svg
      {...svgProps(size)}
      className={className}
      viewBox="0 0 26 20"
      width={(size * 26) / 20}
      height={size}
      strokeWidth={2}
    >
      <path d="M2.6 10.6 6.7 14.6 15.4 5.6" />
      <path d="M10.6 14.2 11.7 15.4 20.4 6.4" />
    </svg>
  );
}

/** Outgoing status: delivery abandoned. */
export function UndeliverableIcon({ size = 12, className }: IconProps) {
  return (
    <svg {...svgProps(size)} className={className} strokeWidth={1.8}>
      <circle cx="10" cy="10" r="7.6" />
      <path d="M10 6.2v4.6" />
      <circle cx="10" cy="13.9" r="0.95" fill="currentColor" strokeWidth={0} />
    </svg>
  );
}

/** The disappearing-messages timer shown on expiring messages. */
export function TimerIcon({ size = 12, className }: IconProps) {
  return (
    <svg {...svgProps(size)} className={className} strokeWidth={1.6}>
      <circle cx="10" cy="10.8" r="6.6" />
      <path d="M10 7.4v3.4l2.3 1.4" />
      <path d="M7.8 2.6h4.4" />
    </svg>
  );
}

/** The wordmark shown in the empty state. */
export function BounceLogo({ size = 64, className }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 64 64"
      fill="none"
      className={className}
      aria-hidden
    >
      <circle cx="32" cy="32" r="30" stroke="currentColor" strokeWidth="2.5" />
      <path
        d="M20 40.5V23.5h9.6c3.6 0 5.9 1.9 5.9 4.7 0 2-1.2 3.5-3 4.1 2.2.5 3.7 2.2 3.7 4.5 0 3.1-2.5 5.2-6.4 5.2H20z"
        stroke="currentColor"
        strokeWidth="2.5"
        strokeLinejoin="round"
      />
      <path d="M40 27.5c3.5 0 6 2.8 6 6.3s-2.5 6.2-6 6.2" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" />
    </svg>
  );
}
