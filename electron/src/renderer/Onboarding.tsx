/**
 * First run.
 *
 * Two ways in, and they are not variations on each other. Creating a profile
 * generates keys locally and is finished the moment it returns — there is no
 * account to register and no server to reach. Linking to an existing profile
 * reaches a device that already has one, proves this device is standing next
 * to it, and receives the keys; it is the only route by which a second device
 * is ever useful.
 */

import * as React from 'react';

import { BounceLogo } from './icons';

type Mode = 'choose' | 'create' | 'link';

export function Onboarding({ onCreated }: { onCreated: () => void }) {
  const [mode, setMode] = React.useState<Mode>('choose');
  const [name, setName] = React.useState('');
  const [deviceName, setDeviceName] = React.useState(defaultDeviceName());
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [address, setAddress] = React.useState('');

  React.useEffect(() => {
    void window.bounce
      .address()
      .then(setAddress)
      .catch(() => setAddress(''));
  }, []);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!name.trim()) return;

    setBusy(true);
    setError(null);
    try {
      await window.bounce.createProfile(name.trim(), deviceName.trim() || 'This device');
      onCreated();
    } catch (failure) {
      setError(String(failure));
      setBusy(false);
    }
  };

  if (mode === 'choose') {
    return (
      <div className="onboarding">
        <BounceLogo size={72} className="placeholder__logo" />
        <h1 className="onboarding__title">Welcome to Bounce</h1>

        <p className="onboarding__body">
          Start a new profile, or add this device to one you already have.
        </p>

        <div className="onboarding__form">
          <button className="onboarding__submit" onClick={() => setMode('create')}>
            Create a new profile
          </button>
          <button className="onboarding__secondary" onClick={() => setMode('link')}>
            Link to an existing profile
          </button>
        </div>

        {address && (
          <div className="onboarding__address">
            This device&rsquo;s address
            <br />
            {address}
          </div>
        )}
      </div>
    );
  }

  if (mode === 'link') {
    return <LinkToProfile onDone={onCreated} onBack={() => setMode('choose')} />;
  }

  return (
    <div className="onboarding">
      <BounceLogo size={72} className="placeholder__logo" />

      <h1 className="onboarding__title">Create a profile</h1>

      <p className="onboarding__body">
        Choose the name your contacts will see. Nothing is registered anywhere — your
        profile and its keys are created on this device and stay on it.
      </p>

      <form className="onboarding__form" onSubmit={submit}>
        <input
          className="onboarding__field"
          placeholder="Your name"
          value={name}
          onChange={(event) => setName(event.target.value)}
          maxLength={128}
          autoFocus
        />
        <input
          className="onboarding__field"
          placeholder="Device name"
          value={deviceName}
          onChange={(event) => setDeviceName(event.target.value)}
          maxLength={128}
        />
        <button
          className="onboarding__submit"
          type="submit"
          disabled={busy || name.trim().length === 0}
        >
          {busy ? 'Creating…' : 'Create profile'}
        </button>
        <button className="onboarding__secondary" type="button" onClick={() => setMode('choose')}>
          Back
        </button>
      </form>

      {error && <div style={{ fontSize: 12, color: 'var(--red)' }}>{error}</div>}

      {address && (
        <div className="onboarding__address">
          This device&rsquo;s address
          <br />
          {address}
        </div>
      )}
    </div>
  );
}


/**
 * Join a profile that already exists on another device.
 *
 * The other device shows a code; this one sends a request signed with its own
 * key and, if the code checks out, is handed the profile. Nothing happens
 * synchronously — the acceptance arrives as an event — so this waits for
 * `profileCreated` rather than for the call to return.
 */
function LinkToProfile({ onDone, onBack }: { onDone: () => void; onBack: () => void }) {
  const [code, setCode] = React.useState('');
  const [waiting, setWaiting] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  // The engine answers by emitting, not by resolving, so the outcome is
  // watched for here. An error event during the wait is the rejection.
  React.useEffect(() => {
    if (!waiting) return;

    const unsubscribe = window.bounce.onEvent((event) => {
      if (event.type === 'profileCreated') {
        onDone();
      } else if (event.type === 'error') {
        setError(event.message);
        setWaiting(false);
      }
    });

    // A device that never answers should not leave this spinning forever.
    const timer = window.setTimeout(() => {
      setError('That device did not answer. Check the code and that it is online.');
      setWaiting(false);
    }, 60_000);

    return () => {
      unsubscribe();
      window.clearTimeout(timer);
    };
  }, [waiting, onDone]);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    const trimmed = code.trim();
    if (!trimmed) return;

    setError(null);
    setWaiting(true);
    try {
      await window.bounce.requestToSync(trimmed);
    } catch (failure) {
      setError(String(failure));
      setWaiting(false);
    }
  };

  return (
    <div className="onboarding">
      <BounceLogo size={72} className="placeholder__logo" />

      <h1 className="onboarding__title">Link this device</h1>

      <p className="onboarding__body">
        On a device that already has your profile, open Settings, choose{' '}
        <strong>Link a device</strong>, and enter the code it shows here. Both devices
        need to be running.
      </p>

      <form className="onboarding__form" onSubmit={submit}>
        <input
          className="onboarding__field"
          placeholder="Paste the code"
          value={code}
          onChange={(event) => setCode(event.target.value)}
          disabled={waiting}
          autoFocus
        />
        <button
          className="onboarding__submit"
          type="submit"
          disabled={waiting || code.trim().length === 0}
        >
          {waiting ? 'Waiting for the other device…' : 'Link this device'}
        </button>
        <button
          className="onboarding__secondary"
          type="button"
          onClick={onBack}
          disabled={waiting}
        >
          Back
        </button>
      </form>

      {error && <div style={{ fontSize: 12, color: 'var(--red)' }}>{error}</div>}
    </div>
  );
}

/** A sensible default device label, from the platform. */
function defaultDeviceName(): string {
  switch (window.bounce.platform) {
    case 'darwin':
      return 'Mac';
    case 'win32':
      return 'Windows PC';
    default:
      return 'Linux desktop';
  }
}
