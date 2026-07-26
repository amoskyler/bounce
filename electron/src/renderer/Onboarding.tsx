/**
 * First run.
 *
 * There is no account to register and no server to reach: creating a profile
 * generates keys locally and is complete the moment it returns.
 */

import * as React from 'react';

import { BounceLogo } from './icons';

export function Onboarding({ onCreated }: { onCreated: () => void }) {
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

  return (
    <div className="onboarding">
      <BounceLogo size={72} className="placeholder__logo" />

      <h1 className="onboarding__title">Welcome to Bounce</h1>

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
