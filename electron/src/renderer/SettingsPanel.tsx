/**
 * The settings panel: your profile, your devices, and the defaults that apply
 * to conversations you have not decided about individually.
 *
 * It is shaped like the conversation details panel on purpose. The two are
 * siblings — one is everything about a conversation that is not its messages,
 * the other is everything about this installation that is not a conversation —
 * so sitting them in the same slide-in frame means there is one place to look
 * for a switch, not two that behave differently.
 *
 * Controls for flows the engine cannot drive yet are *shown disabled* with a
 * title saying why, rather than left out. A missing button reads as an
 * application that never had the feature; a disabled one reads as an
 * application that has not finished it, which is the truth.
 */

import * as React from 'react';

import { Avatar } from './Avatar';
import { CloseIcon } from './icons';
import { conversationTimestamp, shortAddress } from './format';
import { version } from '../../package.json';
import type { Device, Settings, TransportInfo } from '../preload';
import type { State } from './state';

// Imported here rather than from the entry point so the panel travels with its
// own rules; esbuild folds it into the same bundled stylesheet either way.
import './settings.css';

/** Where the source lives, for the About section. */
const PROJECT_URL = 'https://github.com/bounce-chat/bounce';

/**
 * Why the device controls are inert.
 *
 * `frames/pairing.rs` has the wire format for adopting a second device, but
 * nothing drives it, so a device list on a running client always has exactly
 * one row in it.
 */
const PAIRING_PENDING =
  'Multi-device pairing is not implemented yet. The frames exist in the core, but ' +
  'nothing drives the flow that adopts or retires a second device.';

/**
 * Retention choices for new conversations.
 *
 * Deliberately the same list the details panel offers, so a default and the
 * per-conversation override a user later sets are drawn from the same set and
 * one can never be a value the other cannot express.
 */
const RETENTION_OPTIONS: ReadonlyArray<{ label: string; seconds: number }> = [
  { label: 'Off', seconds: 0 },
  { label: '1 day', seconds: 24 * 60 * 60 },
  { label: '1 week', seconds: 7 * 24 * 60 * 60 },
  { label: '4 weeks', seconds: 4 * 7 * 24 * 60 * 60 },
  { label: '1 year', seconds: 365 * 24 * 60 * 60 },
];

/**
 * The settings that apply when a conversation has expressed no preference.
 *
 * `setReadReceipts` and `setTypingIndicators` on the bridge already take
 * `null` for a conversation, meaning "whatever the default is"; these are the
 * values that `null` resolves to.
 *
 * This is the panel's own flattened view of the engine's {@link Settings}: the
 * engine keeps a separate retention default for groups and for direct
 * conversations, and this screen offers one control for both, which is what
 * the Fyne client does.
 */
export interface DefaultSettings {
  /** Seconds before a message in a new conversation expires; 0 is off. */
  retention: number;
  readReceipts: boolean;
  typingIndicators: boolean;
  /** The permissions a group created on this device starts with. */
  restrictPosting: boolean;
  restrictGroupEdits: boolean;
  restrictUserManagement: boolean;
}

/**
 * What the panel shows before the engine answers, and what it falls back to if
 * the engine cannot be asked.
 *
 * Receipts and indicators are on because that is what the Fyne client does;
 * the group permissions are off because a new group with no admin restrictions
 * is the shape most groups want.
 */
const FALLBACK_DEFAULTS: DefaultSettings = {
  retention: 0,
  readReceipts: true,
  typingIndicators: true,
  restrictPosting: false,
  restrictGroupEdits: false,
  restrictUserManagement: false,
};

/** Flatten the engine's settings into what this screen shows. */
function flatten(settings: Settings): DefaultSettings {
  return {
    // One control drives both, so reading back the direct-conversation value
    // is enough — `setDefaultRetention` writes them together.
    retention: settings.defaultDmRetention,
    readReceipts: settings.defaultReadReceipts,
    typingIndicators: settings.defaultTypingIndicators,
    restrictPosting: settings.newGroupRestrictPosting,
    restrictGroupEdits: settings.newGroupRestrictGroupEdits,
    restrictUserManagement: settings.newGroupRestrictUserManagement,
  };
}

type SettingsProps = {
  state: State;
  onClose: () => void;
};

export function SettingsPanel({ state, onClose }: SettingsProps) {
  return (
    <aside className="settings" aria-label="Settings">
      <header className="settings__header">
        <button className="icon-button" onClick={onClose} title="Close">
          <CloseIcon />
        </button>
        <span className="settings__header-title">Settings</span>
      </header>

      <div className="settings__body">
        <ProfileSection state={state} />
        <DevicesSection devices={state.devices} />
        <DefaultsSection />
        <AboutSection />
      </div>
    </aside>
  );
}

/* ------------------------------------------------------------------ */

function ProfileSection({ state }: { state: State }) {
  const profile = state.profile;
  const storedName = profile?.name ?? '';
  const [name, setName] = React.useState(storedName);

  return (
    <>
      <div className="settings__identity">
        <Avatar id={profile?.id ?? state.address} name={storedName || 'You'} size={96} />
      </div>

      <Section title="Profile">
        <Field label="Display name">
          <input
            className="settings__input"
            value={name}
            placeholder="Your name"
            onChange={(event) => setName(event.target.value)}
            onBlur={() => {
              const trimmed = name.trim();
              // An empty name would leave contacts with nothing to show, so a
              // cleared field reverts rather than being sent.
              if (trimmed && trimmed !== storedName) {
                void window.bounce.updateProfileName(trimmed);
              } else {
                setName(storedName);
              }
            }}
          />
        </Field>
        <p className="settings__hint">
          Your name reaches everyone you have ever added, the next time their device
          talks to yours.
        </p>
      </Section>

      <Section title="This device">
        <Field label="Address">
          <AddressWithCopy address={state.address} />
        </Field>
        <p className="settings__hint">
          This address is your public key, written as an onion address. It is the whole
          of your identity here: contacts recognise you by it, and anyone holding it can
          reach you. Read it out loud to compare it in person.
        </p>
      </Section>
    </>
  );
}

/**
 * The address, in full, with a button that copies it.
 *
 * Shown untruncated because this is the one place it is meant to be checked
 * character by character; the abbreviation used elsewhere would defeat that.
 */
function AddressWithCopy({ address }: { address: string }) {
  const [copied, setCopied] = React.useState(false);

  React.useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), 2000);
    return () => window.clearTimeout(timer);
  }, [copied]);

  return (
    <div className="settings__address-row">
      <code className="settings__address">{address || 'Not published yet'}</code>
      <button
        className="settings__copy"
        disabled={!address}
        onClick={() => {
          void navigator.clipboard.writeText(address).then(() => setCopied(true));
        }}
      >
        {copied ? 'Copied' : 'Copy'}
      </button>
    </div>
  );
}

/* ------------------------------------------------------------------ */

function DevicesSection({ devices }: { devices: Device[] }) {
  // The local device first, then the rest by name, so the row a user is
  // looking for is never buried by connection order.
  const ordered = [...devices].sort((a, b) => {
    if (a.local !== b.local) return a.local ? -1 : 1;
    return a.name.localeCompare(b.name);
  });

  return (
    <Section title="Devices">
      {ordered.length === 0 && <p className="settings__hint">No devices recorded yet.</p>}

      {ordered.map((device) => (
        <div key={device.id} className="settings__device">
          <span
            className={
              device.online ? 'settings__dot settings__dot--online' : 'settings__dot'
            }
            aria-label={device.online ? 'online' : 'offline'}
          />
          <div className="settings__device-text">
            <div className="settings__device-name">
              <span className="settings__device-label">{device.name}</span>
              {device.local && <span className="settings__badge">This device</span>}
              {device.revoked && <span className="settings__badge">Revoked</span>}
            </div>
            {/*
              The address goes on the meta line because two devices may well
              share a name, and the address is the thing that actually
              distinguishes them.
            */}
            <div className="settings__device-meta">
              <code>{shortAddress(device.address)}</code>
              <span>{presence(device)}</span>
            </div>
          </div>
          <button className="settings__link" disabled title={PAIRING_PENDING}>
            Revoke
          </button>
        </div>
      ))}

      <button className="settings__action" disabled title={PAIRING_PENDING}>
        Add device
      </button>
      <p className="settings__hint">
        Pairing a second device is not built yet, so this profile lives on this device
        alone. Losing it loses the identity with it.
      </p>
    </Section>
  );
}

/** How a device's connection reads on its row. */
function presence(device: Device): string {
  if (device.online) return 'Active now';
  if (!device.lastSeen) return 'Never connected';
  return `Last seen ${conversationTimestamp(device.lastSeen)}`;
}

/* ------------------------------------------------------------------ */

function DefaultsSection() {
  const [defaults, setDefaults] = React.useState<DefaultSettings>(FALLBACK_DEFAULTS);

  React.useEffect(() => {
    let cancelled = false;

    void (async () => {
      try {
        const stored = await window.bounce.settings();
        if (!cancelled) setDefaults(flatten(stored));
      } catch (error) {
        // A screen showing the fallbacks is more useful than a screen showing
        // nothing, but silently disagreeing with the engine is the kind of bug
        // that takes an afternoon to find, so say so.
        console.warn('could not read stored defaults:', error);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, []);

  /**
   * Move a switch and tell the engine.
   *
   * The engine emits no event for these, so the local copy is the only thing
   * that redraws the control; applying it first is what keeps the switch from
   * lagging a round trip behind the click.
   */
  const apply = (next: DefaultSettings, write: () => Promise<void>) => {
    setDefaults(next);
    // `then` rather than a bare call, so a bridge method that is missing
    // entirely surfaces as a warning instead of throwing through the handler.
    void Promise.resolve()
      .then(write)
      .catch((error: unknown) => console.warn('could not save default:', error));
  };

  return (
    <>
      <Section title="Message defaults">
        <p className="settings__hint">
          These apply to conversations you have not set individually. Changing one
          leaves those alone.
        </p>

        <Field label="Disappearing messages">
          <select
            className="settings__input"
            value={defaults.retention}
            onChange={(event) => {
              const seconds = Number(event.target.value);
              apply({ ...defaults, retention: seconds }, () =>
                window.bounce.setDefaultRetention(seconds),
              );
            }}
          >
            {RETENTION_OPTIONS.map((option) => (
              <option key={option.seconds} value={option.seconds}>
                {option.label}
              </option>
            ))}
          </select>
        </Field>

        <Toggle
          label="Send read receipts"
          checked={defaults.readReceipts}
          onChange={(value) =>
            apply({ ...defaults, readReceipts: value }, () =>
              window.bounce.setDefaultReadReceipts(value),
            )
          }
        />
        <Toggle
          label="Send typing indicators"
          checked={defaults.typingIndicators}
          onChange={(value) =>
            apply({ ...defaults, typingIndicators: value }, () =>
              window.bounce.setDefaultTypingIndicators(value),
            )
          }
        />
        <p className="settings__hint">
          Turning either off stops your device sending them. You still see other
          people&rsquo;s.
        </p>
      </Section>

      <Section title="New group defaults">
        <p className="settings__hint">
          How a group you create starts out. Every one of these can be changed later by
          an admin.
        </p>
        <Toggle
          label="Only admins can post"
          checked={defaults.restrictPosting}
          onChange={(value) =>
            apply({ ...defaults, restrictPosting: value }, () =>
              window.bounce.setDefaultGroupPermission('posting', value),
            )
          }
        />
        <Toggle
          label="Only admins can edit the group"
          checked={defaults.restrictGroupEdits}
          onChange={(value) =>
            apply({ ...defaults, restrictGroupEdits: value }, () =>
              window.bounce.setDefaultGroupPermission('edits', value),
            )
          }
        />
        <Toggle
          label="Only admins can manage members"
          checked={defaults.restrictUserManagement}
          onChange={(value) =>
            apply({ ...defaults, restrictUserManagement: value }, () =>
              window.bounce.setDefaultGroupPermission('userManagement', value),
            )
          }
        />
      </Section>
    </>
  );
}

/* ------------------------------------------------------------------ */

function AboutSection() {
  const [transport, setTransport] = React.useState<TransportInfo | null>(null);

  React.useEffect(() => {
    let cancelled = false;

    void window.bounce
      .transport()
      .then((info) => {
        if (!cancelled) setTransport(info);
      })
      .catch((error: unknown) => console.warn('could not read the transport:', error));

    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <Section title="About">
      <div className="settings__about-row">
        <span>Bounce</span>
        <span className="settings__about-value">{version}</span>
      </div>

      <div className="settings__about-row">
        <span>Transport</span>
        <span className="settings__about-value">{transportName(transport)}</span>
      </div>

      {/*
        The insecure case already has a banner across the top of the window, but
        someone who came here to check should get an answer rather than be sent
        back to look at it.
      */}
      <p className="settings__hint">{transportNote(transport)}</p>

      <div className="settings__about-row">
        <span>Project</span>
        {/*
          The main process denies in-window navigation and hands the URL to the
          system browser, so this opens outside the app.
        */}
        <a
          className="settings__link"
          href={PROJECT_URL}
          target="_blank"
          rel="noreferrer"
        >
          bounce-chat/bounce
        </a>
      </div>
    </Section>
  );
}

function transportName(transport: TransportInfo | null): string {
  if (!transport) return 'Checking…';
  return transport.name === 'tor' ? 'Tor onion service' : 'Direct TCP';
}

function transportNote(transport: TransportInfo | null): string {
  if (!transport) return 'Asking the engine which transport it started on.';
  if (transport.anonymous) {
    return 'Every connection runs through Tor, so no peer learns your network address and no observer learns who you talk to.';
  }
  return 'Running without Tor. Connections go straight to peers, which protects no metadata at all — a development mode, not a private one.';
}

/* ------------------------------------------------------------------ */

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="settings__section">
      <div className="settings__section-title">{title}</div>
      {children}
    </section>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="settings__field">
      <span className="settings__field-label">{label}</span>
      {children}
    </label>
  );
}

function Toggle({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <label className="settings__toggle">
      <span>{label}</span>
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.target.checked)}
      />
    </label>
  );
}
