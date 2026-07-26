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
import { unblockContact } from './DetailsPanel';
import { CloseIcon, QrCode } from './icons';
import { conversationTimestamp, shortAddress } from './format';
import { choosePicture, prepareImage } from './outgoing-image';
import { version } from '../../package.json';
import type { Device, Settings, TransportInfo, User } from '../preload';
import type { State } from './state';

// Imported here rather than from the entry point so the panel travels with its
// own rules; esbuild folds it into the same bundled stylesheet either way.
import './settings.css';

/** Where the source lives, for the About section. */
const PROJECT_URL = 'https://github.com/bounce-chat/bounce';

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
        <ContactsSection state={state} />
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
  const [pictureError, setPictureError] = React.useState<string | null>(null);

  return (
    <>
      <div className="settings__identity">
        {/*
          The avatar is the control. A separate "change picture" button beside
          it would say the same thing twice, and clicking your own face is what
          people try first.
        */}
        <button
          className="settings__avatar-button"
          onClick={() => void pickProfilePicture(setPictureError)}
          title="Change your picture"
          aria-label="Change your picture"
        >
          <Avatar
            images={profile?.images}
            id={profile?.id ?? state.address}
            name={storedName || 'You'}
            size={96}
          />
          <span className="settings__avatar-overlay">Change</span>
        </button>
      </div>

      {pictureError && <p className="settings__error">{pictureError}</p>}

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
  const [linking, setLinking] = React.useState(false);
  const [revoking, setRevoking] = React.useState<Device | null>(null);

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
              {/*
                Editable in place. The name is local — nothing is broadcast —
                so this writes on blur and lets the engine's `deviceUpdated`
                event put the value back, rather than holding a second copy.
              */}
              <input
                className="settings__device-label settings__device-input"
                defaultValue={device.name}
                aria-label={`Name for ${shortAddress(device.address)}`}
                onBlur={(event) => {
                  const name = event.target.value.trim();
                  if (!name || name === device.name) {
                    event.target.value = device.name;
                    return;
                  }
                  void window.bounce.renameDevice(device.id, name).catch(() => {
                    // A rename that does not take should not leave the field
                    // showing something the engine never accepted.
                    event.target.value = device.name;
                  });
                }}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') event.currentTarget.blur();
                  if (event.key === 'Escape') {
                    event.currentTarget.value = device.name;
                    event.currentTarget.blur();
                  }
                }}
              />
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
          {/*
            A device cannot revoke itself: it would be signing away its own
            ability to sign, including for the frame that says so.
          */}
          {!device.local && !device.revoked && (
            <button
              className="settings__link settings__link--destructive"
              onClick={() => setRevoking(device)}
            >
              Revoke
            </button>
          )}
        </div>
      ))}

      <button className="settings__action" onClick={() => setLinking(true)}>
        Link a device
      </button>

      {linking && <LinkDeviceDialog onClose={() => setLinking(false)} />}
      {revoking && (
        <RevokeDeviceDialog device={revoking} onClose={() => setRevoking(null)} />
      )}
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

/**
 * The contacts this profile knows, blocked ones included.
 *
 * The sidebar is a list of *conversations*, and a blocked contact has none, so
 * blocking used to be a one-way door: the only Unblock button lived in a panel
 * that blocking unmounted, and there is no directory to re-find anyone through
 * — re-pairing in person does not clear the flag either
 * (`engine/mod.rs:1079-1086`). Go pairs blocking with a browser over the
 * contact store for exactly this reason, with a "Show Blocked" checkbox
 * (`ui/new_dm_container.go:64-68`); this is that list. It reads `state.users`
 * rather than `conversations()`, so nothing a contact's own state does can
 * remove the row that undoes it.
 *
 * @param users every known user
 * @param myId the profile's own id, which is a conversation but not a contact
 */
export function visibleContacts(
  users: readonly User[],
  myId: string | undefined,
  showBlocked: boolean,
): User[] {
  return users
    .filter((user) => user.id !== myId && (showBlocked || !user.blocked))
    .sort((a, b) => (a.alias || a.name).localeCompare(b.alias || b.name));
}

function ContactsSection({ state }: { state: State }) {
  const [showBlocked, setShowBlocked] = React.useState(false);
  const known = Object.values(state.users);
  const contacts = visibleContacts(known, state.profile?.id, showBlocked);
  const blocked = known.filter((user) => user.blocked && user.id !== state.profile?.id).length;

  return (
    <Section title={`Contacts (${contacts.length})`}>
      {known.length === 0 && (
        <p className="settings__hint">
          No contacts yet. Adding one is an exchange of pairing codes in person.
        </p>
      )}

      {contacts.map((contact) => {
        const name = contact.alias || contact.name;
        return (
          <div
            key={contact.id}
            className={
              contact.blocked ? 'settings__contact settings__contact--blocked' : 'settings__contact'
            }
          >
            <Avatar id={contact.id} name={name} size={28} />
            <div className="settings__contact-text">
              <div className="settings__contact-name">
                <span className="settings__contact-label">{name}</span>
                {contact.blocked && <span className="settings__badge">Blocked</span>}
              </div>
              <code className="settings__contact-address">{shortAddress(contact.id)}</code>
            </div>

            {contact.blocked ? (
              <button
                className="settings__link"
                onClick={() => {
                  void unblockContact(contact.id).catch((error: unknown) =>
                    console.warn('could not unblock:', error),
                  );
                }}
              >
                Unblock
              </button>
            ) : (
              // A contact whose thread has been closed is still a contact, so
              // the row that reopens it is here rather than nowhere.
              !contact.openDm && (
                <button
                  className="settings__link"
                  onClick={() => {
                    void window.bounce
                      .setOpenDm(contact.id, true)
                      .catch((error: unknown) =>
                        console.warn('could not open the conversation:', error),
                      );
                  }}
                >
                  Open
                </button>
              )
            )}
          </div>
        );
      })}

      {/* Always offered, not only when something is hidden: someone who has
          just blocked a contact comes here to look for them, and a checkbox
          that appears only once you know it exists is no help. */}
      <Toggle label="Show blocked" checked={showBlocked} onChange={setShowBlocked} />
      {blocked > 0 && !showBlocked && (
        <p className="settings__hint">
          {blocked === 1 ? '1 blocked contact is' : `${blocked} blocked contacts are`} hidden.
        </p>
      )}
    </Section>
  );
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

/* ------------------------------------------------------------------ */

/**
 * Show a code that lets another device join this profile.
 *
 * The warning is not decoration. This code and the "add a contact" code look
 * identical and are read the same way, and they grant entirely different
 * things: one lets somebody message you, this one hands over the profile's
 * private keys. The engine keeps their secrets in separate tables so one
 * cannot be redeemed for the other, but a person holding a phone cannot see
 * that — the screen has to say it.
 */
function LinkDeviceDialog({ onClose }: { onClose: () => void }) {
  const [code, setCode] = React.useState('');
  const [error, setError] = React.useState<string | null>(null);
  const [copied, setCopied] = React.useState(false);

  React.useEffect(() => {
    void window.bounce
      .createSyncCode()
      .then(setCode)
      .catch((failure: unknown) => setError(String(failure)));
  }, []);

  React.useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), 2000);
    return () => window.clearTimeout(timer);
  }, [copied]);

  return (
    <div className="modal__backdrop" onClick={onClose}>
      <div className="modal" onClick={(event) => event.stopPropagation()}>
        <div className="modal__title">Link a device</div>

        <p className="settings__hint" style={{ marginTop: 0 }}>
          On the new device, choose <strong>Link to an existing profile</strong> and
          enter this code. It expires in five minutes and works once.
        </p>

        <p className="settings__warning">
          Anyone who uses this code becomes one of your devices and receives your
          private keys. Only show it to a device you own.
        </p>

        {code && (
          <div style={{ display: 'flex', justifyContent: 'center', marginBottom: 12 }}>
            <QrCode text={code} size={196} />
          </div>
        )}

        <div className="settings__address-row">
          <code className="settings__address">{code || 'Generating…'}</code>
          <button
            className="settings__copy"
            disabled={!code}
            onClick={() => {
              void navigator.clipboard.writeText(code).then(() => setCopied(true));
            }}
          >
            {copied ? 'Copied' : 'Copy'}
          </button>
        </div>

        {error && <p className="settings__error">{error}</p>}

        <div className="modal__actions">
          <button className="modal__button" onClick={onClose}>
            Done
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * Confirm before retiring a device.
 *
 * Revocation is global and permanent: every contact is told, and the device
 * can never be readmitted under the same identity, because its address *is*
 * its key. Worth a sentence and a second click.
 */
function RevokeDeviceDialog({ device, onClose }: { device: Device; onClose: () => void }) {
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  const revoke = async () => {
    setBusy(true);
    setError(null);
    try {
      await window.bounce.revokeDevice(device.id);
      onClose();
    } catch (failure) {
      setError(String(failure));
      setBusy(false);
    }
  };

  return (
    <div className="modal__backdrop" onClick={onClose}>
      <div className="modal" onClick={(event) => event.stopPropagation()}>
        <div className="modal__title">Revoke this device?</div>

        <p className="settings__hint" style={{ marginTop: 0 }}>
          <code>{shortAddress(device.address)}</code>
          {device.name ? ` — ${device.name}` : ''}
        </p>

        <p className="settings__warning">
          Every contact is told to stop trusting it, and it cannot be added back:
          a device's address is its key. Anything it signed before now stays
          valid, so your history is not affected.
        </p>

        {error && <p className="settings__error">{error}</p>}

        <div className="modal__actions">
          <button className="modal__button" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button
            className="modal__button modal__button--destructive"
            onClick={() => void revoke()}
            disabled={busy}
          >
            {busy ? 'Revoking…' : 'Revoke'}
          </button>
        </div>
      </div>
    </div>
  );
}

/** Choose a picture and set it as the profile's, reporting any refusal. */
async function pickProfilePicture(onError: (message: string | null) => void) {
  onError(null);
  const file = await choosePicture();
  if (!file) return;

  try {
    await window.bounce.setProfileImage(await prepareImage(file));
  } catch (failure) {
    onError(failure instanceof Error ? failure.message : String(failure));
  }
}
