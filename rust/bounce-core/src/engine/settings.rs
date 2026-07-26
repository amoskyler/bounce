//! Profile-wide settings, and the device group as the interface sees it.
//!
//! These are the preferences with no single conversation to hang off: how long
//! a *new* conversation keeps its messages, whether receipts and typing
//! indicators go out unless a conversation says otherwise, and the
//! restrictions a group created on this device is born with.
//! [`Engine::create_group`] reads them, so a change here shows up in the next
//! group made rather than in the ones already going.
//!
//! Nothing here is broadcast. The Go implementation carries these to a user's
//! other devices in a sync-scoped `UpdateSettings` frame — the frame type
//! number is reserved as [`FrameType::UpdateSettings`] — but the engine does
//! not drive that flow yet, so a change applies to the device it was made on
//! and no other. That is a gap rather than a design decision, and it is worth
//! knowing before trusting a setting to hold across a device group.
//!
//! [`FrameType::UpdateSettings`]: crate::types::FrameType::UpdateSettings

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::frames::identity::{self, ProfileSettings};
use crate::net::Network;

use super::{DeviceView, Engine, Event};

/// The three answers to "should I join a group I have been invited to without
/// being asked?".
///
/// The values are the ones the Go implementation writes into an
/// `UpdateSettings` payload and stores, so they are wire values and cannot be
/// renumbered — note that the permissive-ish middle option, not "never", is
/// zero.
pub mod auto_join {
    /// Join automatically, but only when the group holds nobody this device's
    /// owner has not already accepted as a contact.
    pub const ONLY_WITHOUT_NEW_USERS: i64 = 0;
    /// Never join without being asked.
    pub const NEVER: i64 = 1;
    /// Always join.
    pub const ALWAYS: i64 = 2;
}

/// A snapshot of the profile-wide settings, as the interface needs them.
///
/// [`ProfileSettings`] itself is the storage and wire shape: its serde names
/// are the Go field names, and it carries a row ID the interface has no use
/// for. This is the same information under the camelCase names every other
/// view uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    /// Seconds a new group keeps its messages for; zero keeps them
    /// indefinitely.
    pub default_group_retention: i64,
    /// The same, for a new direct conversation.
    pub default_dm_retention: i64,
    pub default_read_receipts: bool,
    pub default_typing_indicators: bool,
    pub new_group_restrict_posting: bool,
    pub new_group_restrict_group_edits: bool,
    pub new_group_restrict_user_management: bool,
    /// One of the [`auto_join`] values.
    pub auto_join_groups: i64,
    pub blocked_groups: Vec<Uuid>,
}

impl<N: Network + 'static> Engine<N> {
    // ---------------------------------------------------------------------
    // Profile settings
    // ---------------------------------------------------------------------

    /// The current profile-wide settings.
    pub fn settings(&self) -> Result<SettingsView> {
        Ok(self.settings_view(&self.profile_settings()?))
    }

    /// Set how long a new conversation keeps its messages, in seconds; zero
    /// keeps them indefinitely.
    ///
    /// Groups and direct conversations have separate stored defaults, because
    /// the Go client offers a control for each. The client here offers one, so
    /// both move together; a device synced from Go may still show them
    /// diverged, which is why [`SettingsView`] reports them separately.
    ///
    /// Conversations already under way are untouched — their retention was
    /// fixed when they were created, and changing it is
    /// [`Engine::set_retention`]'s job.
    pub fn set_default_retention(&self, seconds: i64) -> Result<()> {
        // A negative retention would put every new message's expiry in the
        // past, deleting conversations as fast as they were written.
        if seconds < 0 {
            return Err(Error::InvalidFrame("retention cannot be negative".into()));
        }

        self.update_settings(|settings| {
            settings.default_group_retention = seconds;
            settings.default_dm_retention = seconds;
        })
    }

    /// Whether read receipts are sent for conversations that have not
    /// overridden the choice themselves.
    pub fn set_default_read_receipts(&self, enabled: bool) -> Result<()> {
        self.update_settings(|settings| settings.default_send_read_receipts = enabled)
    }

    /// Whether typing indicators are sent for conversations that have not
    /// overridden the choice themselves.
    pub fn set_default_typing_indicators(&self, enabled: bool) -> Result<()> {
        self.update_settings(|settings| settings.default_send_typing_indicators = enabled)
    }

    /// Whether groups created on this device start with posting restricted to
    /// administrators.
    pub fn set_new_group_restrict_posting(&self, restricted: bool) -> Result<()> {
        self.update_settings(|settings| settings.new_group_restrict_posting = restricted)
    }

    /// Whether groups created on this device start with renaming and images
    /// restricted to administrators.
    pub fn set_new_group_restrict_edits(&self, restricted: bool) -> Result<()> {
        self.update_settings(|settings| settings.new_group_restrict_group_edits = restricted)
    }

    /// Whether groups created on this device start with inviting and removing
    /// restricted to administrators.
    pub fn set_new_group_restrict_user_management(&self, restricted: bool) -> Result<()> {
        self.update_settings(|settings| settings.new_group_restrict_user_management = restricted)
    }

    /// Choose when an invitation is accepted without asking; see [`auto_join`].
    pub fn set_auto_join_groups(&self, setting: i64) -> Result<()> {
        // The stored value is read back as a mode rather than a flag, so an
        // unrecognised one would be silently treated as
        // `ONLY_WITHOUT_NEW_USERS` — the least restrictive of the three, and
        // not what anyone setting an unknown value meant.
        if !matches!(
            setting,
            auto_join::ONLY_WITHOUT_NEW_USERS | auto_join::NEVER | auto_join::ALWAYS
        ) {
            return Err(Error::InvalidFrame(format!(
                "unknown auto-join setting {setting}"
            )));
        }

        self.update_settings(|settings| settings.auto_join_groups = setting)
    }

    /// The settings row for this profile, falling back to the defaults.
    ///
    /// A profile always has a row written when it is created, so the fallback
    /// only covers a database restored without one; matching the fallback
    /// [`Engine::create_group`] already uses keeps the two from disagreeing
    /// about what a default is.
    fn profile_settings(&self) -> Result<ProfileSettings> {
        let my_id = self.store.my_user_id()?;
        Ok(self
            .store
            .profile_settings(my_id)?
            .unwrap_or_else(|| ProfileSettings::defaults(my_id)))
    }

    /// Load, change one thing, store, and announce the result.
    fn update_settings(&self, change: impl FnOnce(&mut ProfileSettings)) -> Result<()> {
        let mut settings = self.profile_settings()?;
        change(&mut settings);
        self.store.save_profile_settings(&settings)?;

        // The whole set goes out rather than the one field, so a client can
        // replace its copy without tracking which setter it called.
        self.emit(Event::SettingsUpdated {
            settings: self.settings_view(&settings),
        });
        Ok(())
    }

    fn settings_view(&self, settings: &ProfileSettings) -> SettingsView {
        SettingsView {
            default_group_retention: settings.default_group_retention,
            default_dm_retention: settings.default_dm_retention,
            default_read_receipts: settings.default_send_read_receipts,
            default_typing_indicators: settings.default_send_typing_indicators,
            new_group_restrict_posting: settings.new_group_restrict_posting,
            new_group_restrict_group_edits: settings.new_group_restrict_group_edits,
            new_group_restrict_user_management: settings.new_group_restrict_user_management,
            auto_join_groups: settings.auto_join_groups,
            blocked_groups: identity::parse_uuid_list(&settings.blocked_groups),
        }
    }

    // ---------------------------------------------------------------------
    // This user's devices
    // ---------------------------------------------------------------------

    /// Every device in this profile's device group, revoked ones included.
    ///
    /// A revoked device is still listed: it is what tells its owner that a lost
    /// laptop was actually cut off, and it stays in the group forever because
    /// its key is needed to check signatures it made while it was trusted.
    pub fn devices(&self) -> Result<Vec<DeviceView>> {
        let my_id = self.store.my_user_id()?;
        let address = self.network.address();

        Ok(self
            .store
            .devices_for_user(my_id)?
            .iter()
            .map(|device| {
                // Whether a peer device is connected lives behind an async
                // lock, and this call is synchronous, so it reports only the
                // one thing it can know without waiting: this device is up,
                // because it is the one asking.
                let local = device.address == address;
                self.device_view(device, local, local)
            })
            .collect())
    }

    /// Give one of this profile's devices a human-readable name.
    ///
    /// The name is local: it is not part of what a device puts on the wire, so
    /// this renames the row and tells the interface, and nothing goes out.
    pub fn rename_device(&self, device_id: Uuid, name: &str) -> Result<()> {
        if !identity::valid_device_name(name) {
            return Err(Error::InvalidFrame("invalid device name".into()));
        }

        // The store renames by ID with no notion of ownership, so a device ID
        // from a contact's device group would rename *their* device in our
        // database. Only our own are ours to name.
        let my_id = self.store.my_user_id()?;
        let mut device = self
            .store
            .devices_for_user(my_id)?
            .into_iter()
            .find(|device| device.id == device_id)
            .ok_or(Error::DeviceNotFound)?;

        self.store.rename_device(device_id, name)?;
        device.name = name.to_string();

        let local = device.address == self.network.address();
        self.emit(Event::DeviceUpdated {
            device: self.device_view(&device, local, local),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::mpsc::UnboundedReceiver;

    use crate::crypto::DeviceKey;
    use crate::frames::identity::{Device, User};
    use crate::net::PeerConnection;
    use crate::store::Store;

    use super::*;

    /// A network that knows its own address and nothing else.
    ///
    /// Everything under test here is a database read or write, and no frame is
    /// ever sent, so a stub keeps these tests synchronous rather than standing
    /// a listener up per case.
    struct StubNetwork {
        address: String,
    }

    impl Network for StubNetwork {
        type Stream = tokio::io::DuplexStream;

        fn address(&self) -> String {
            self.address.clone()
        }

        async fn accept(&self) -> Result<PeerConnection<Self::Stream>> {
            Err(Error::Network("the stub network has no peers".into()))
        }

        async fn dial(&self, _address: &str) -> Result<PeerConnection<Self::Stream>> {
            Err(Error::Network("the stub network has no peers".into()))
        }

        fn sign(&self, _data: &[u8]) -> Vec<u8> {
            Vec::new()
        }

        async fn shutdown(&self) {}
    }

    /// An engine with a profile already created, its store, and its events.
    fn engine() -> (
        Arc<Engine<StubNetwork>>,
        Arc<Store>,
        UnboundedReceiver<Event>,
    ) {
        let key = DeviceKey::generate();
        let network = Arc::new(StubNetwork {
            address: key.address(),
        });
        let store = Arc::new(Store::in_memory().expect("opens a database"));

        let (engine, events) = Engine::new(key, Arc::clone(&store), network);
        engine
            .create_profile("Alice", "Alice's laptop")
            .expect("creates a profile");

        (engine, store, events)
    }

    #[test]
    fn a_changed_default_round_trips_through_the_store() {
        let (engine, store, _events) = engine();

        // Assert the starting value, or a setter that did nothing at all would
        // still pass the checks below by matching the defaults.
        let before = engine.settings().expect("reads settings");
        assert!(before.default_read_receipts);
        assert_ne!(before.default_group_retention, 60);

        engine
            .set_default_read_receipts(false)
            .expect("stores the choice");
        engine.set_default_retention(60).expect("stores the choice");

        let after = engine.settings().expect("reads settings");
        assert!(!after.default_read_receipts);
        assert_eq!(after.default_group_retention, 60);

        // Read the row directly as well: `settings()` served from a cache the
        // setter had mutated would agree with itself while the database still
        // held the old values, and a restart would lose the change.
        let my_id = store.my_user_id().expect("has a profile");
        let stored = store
            .profile_settings(my_id)
            .expect("queries settings")
            .expect("a row exists");
        assert!(!stored.default_send_read_receipts);
        assert_eq!(stored.default_group_retention, 60);
        assert_eq!(stored.default_dm_retention, 60);
    }

    #[test]
    fn setting_something_announces_the_whole_set() {
        let (engine, _store, mut events) = engine();

        // Drain the profile creation event.
        events.try_recv().expect("profile creation was emitted");

        engine
            .set_new_group_restrict_posting(true)
            .expect("stores the choice");

        match events.try_recv().expect("a settings event was emitted") {
            Event::SettingsUpdated { settings } => {
                assert!(settings.new_group_restrict_posting);
                // Fields the setter did not touch still have to arrive, since
                // the client replaces its copy wholesale.
                assert!(settings.default_typing_indicators);
            }
            other => panic!("expected settingsUpdated, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_auto_join_setting_is_refused() {
        let (engine, _store, _events) = engine();

        assert!(matches!(
            engine.set_auto_join_groups(7),
            Err(Error::InvalidFrame(_))
        ));
        assert!(engine.set_auto_join_groups(auto_join::NEVER).is_ok());
        assert_eq!(
            engine.settings().expect("reads settings").auto_join_groups,
            auto_join::NEVER
        );
    }

    #[test]
    fn an_invalid_device_name_is_rejected() {
        let (engine, _store, _events) = engine();
        let device = engine.devices().expect("lists devices").remove(0);

        // A newline would let one device's name forge a second entry anywhere
        // names are shown or logged a line at a time.
        assert!(matches!(
            engine.rename_device(device.id, "laptop\nphone"),
            Err(Error::InvalidFrame(_))
        ));
        assert_eq!(
            engine.devices().expect("lists devices")[0].name,
            device.name,
            "a rejected rename must not have been half applied"
        );

        engine
            .rename_device(device.id, "Studio")
            .expect("accepts an ordinary name");
        assert_eq!(engine.devices().expect("lists devices")[0].name, "Studio");
    }

    #[test]
    fn a_device_belonging_to_someone_else_cannot_be_renamed() {
        let (engine, store, _events) = engine();

        let contact = User::new(Uuid::new_v4(), "Bob".into());
        store.save_user(&contact).expect("saves the contact");

        let theirs = Device::new(Uuid::new_v4(), contact.id, "their-address".into(), crate::now());
        store.save_device(&theirs).expect("saves their device");

        assert!(matches!(
            engine.rename_device(theirs.id, "Mine now"),
            Err(Error::DeviceNotFound)
        ));
        assert!(
            store
                .devices_for_user(contact.id)
                .expect("lists their devices")[0]
                .name
                .is_empty(),
            "their device was renamed in our database"
        );
    }

    #[test]
    fn devices_marks_exactly_one_device_local() {
        let (engine, store, _events) = engine();
        let my_id = store.my_user_id().expect("has a profile");

        let mut phone = Device::new(Uuid::new_v4(), my_id, "phone-address".into(), crate::now());
        phone.name = "Phone".into();
        store.save_device(&phone).expect("saves the second device");

        let devices = engine.devices().expect("lists devices");
        assert_eq!(devices.len(), 2);

        // More than one would mean the interface offering to rename or revoke
        // the device it is running on as if it were somewhere else.
        let local: Vec<&DeviceView> = devices.iter().filter(|device| device.local).collect();
        assert_eq!(local.len(), 1, "got {devices:?}");
        assert_eq!(local[0].address, engine.address());
    }
}
