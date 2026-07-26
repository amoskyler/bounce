//! Status changes, as the timeline shows them.
//!
//! "Ada renamed the group", "Bo left", "Cy made Dee an admin" — the sentences
//! that sit between the messages. They are not a separate kind of frame: every
//! one of them is an [`UpdateGroup`] or [`UpdateDm`] that the engine already
//! stores, read back out in a shape the interface can render.
//!
//! ## Why the sentence is not built here
//!
//! Each view carries a *kind* and the ids involved, never finished prose. Two
//! reasons:
//!
//! - Names change. A row that said "Ada left the group" would keep saying Ada
//!   after Ada renamed herself, because the words were fixed when the update
//!   arrived. Carrying the id lets the client resolve the name every render.
//! - "You" is the client's word. The engine has no notion of whose screen this
//!   is beyond the local profile, and the same event reads differently on the
//!   two devices that saw it.
//!
//! The client owns the wording; see `SystemMessage.tsx`, which keeps it
//! word-for-word identical to the Fyne client's `ui/thread_item.go`.
//!
//! ## What does not get a row
//!
//! Mute state, read-receipt and typing-indicator overrides, and aliases are one
//! side's private view of a conversation rather than something that happened
//! in it, so they change quietly. Group deletion has no row either: the
//! conversation it would appear in is gone.

use uuid::Uuid;

use crate::error::Result;
use crate::frames::group::UpdateGroup;
use crate::frames::identity::User;
use crate::frames::update::UpdateDm;
use crate::frames::UpdateDmType;
use crate::net::Network;
use crate::types::UpdateGroupType;

use super::event::SystemMessageView;
use super::Engine;

/// Seconds in each of the retention presets the Fyne client offers.
///
/// A month is four weeks there, not a calendar month, and the labels are that
/// client's exact strings — a status row is history, and the two builds must
/// not disagree about what it says.
const RETENTION_LABELS: [(i64, &str); 5] = [
    (0, "Off"),
    (60 * 60, "1 Hour"),
    (24 * 60 * 60, "1 Day"),
    (7 * 24 * 60 * 60, "1 Week"),
    (4 * 7 * 24 * 60 * 60, "1 Month"),
];

impl<N: Network + 'static> Engine<N> {
    /// Every status row for every conversation, for the opening snapshot.
    pub(super) fn system_message_views(&self) -> Result<Vec<SystemMessageView>> {
        let mut views = Vec::new();

        for group in self.store.all_groups()? {
            // The group's own creation is the first row in its timeline.
            views.push(SystemMessageView {
                id: group.id,
                thread: group.id,
                actor: group.created_by,
                kind: "groupCreated".into(),
                subject: None,
                value: None,
                timestamp: group.created_at,
            });

            for update in self.store.updates_for_group(group.id)? {
                if let Some(view) = system_message_for_group_update(&update) {
                    views.push(view);
                }
            }
        }

        // Conversation rows are threaded by XORing our own id back out, so
        // without a profile there is nothing to thread them under. A device
        // with no profile has no conversations either, so there is nothing to
        // lose by skipping them.
        if let Some(profile) = self.store.profile()? {
            for update in self.store.all_update_dms()? {
                if let Some(view) = system_message_for_dm_update(&update, profile.id) {
                    views.push(view);
                }
            }
        }

        views.sort_by_key(|view| view.timestamp);
        Ok(views)
    }

    /// Announce a group status change, if this update is one the timeline
    /// shows.
    pub(super) fn emit_group_system_message(&self, update: &UpdateGroup) {
        if let Some(message) = system_message_for_group_update(update) {
            self.emit(super::Event::SystemMessage { message });
        }
    }

    /// Announce a conversation status change.
    pub(super) fn emit_dm_system_message(&self, update: &UpdateDm) {
        let Ok(my_id) = self.store.my_user_id() else {
            return;
        };
        if let Some(message) = system_message_for_dm_update(update, my_id) {
            self.emit(super::Event::SystemMessage { message });
        }
    }
}

/// Turn a stored group update into a row, or `None` if it is not one the
/// timeline shows.
fn system_message_for_group_update(update: &UpdateGroup) -> Option<SystemMessageView> {
    use UpdateGroupType::*;

    let kind = update.kind().ok()?;
    let mut subject = None;
    let mut value = None;

    let name = match kind {
        ChangeName => {
            value = Some(String::from_utf8(update.data.clone()).ok()?);
            "groupRenamed"
        }
        SetImage => "groupImageChanged",
        ChangeRetention => {
            value = Some(retention_label(update.data_as_i64()?));
            "retentionChanged"
        }
        SetClearBefore => "historyCleared",
        Block => "groupBlocked",

        InviteUser => {
            // The invitation carries the whole user so the invitee's devices
            // travel with it; only the id is needed here.
            let invitee: User = crate::msgpack::from_slice(&update.data).ok()?;
            subject = Some(invitee.id.to_string());
            "userInvited"
        }
        RemoveUser => {
            // Leaving is a removal you apply to yourself. The client phrases
            // the two differently and works that out from the ids.
            subject = Some(update.data_as_uuid()?.to_string());
            "userRemoved"
        }
        RevokeInvite => {
            subject = Some(update.data_as_uuid()?.to_string());
            "inviteRevoked"
        }
        PromoteAdmin => {
            subject = Some(update.data_as_uuid()?.to_string());
            "adminPromoted"
        }
        DemoteAdmin => {
            subject = Some(update.data_as_uuid()?.to_string());
            "adminDemoted"
        }

        RespondToInvite => {
            if update.data_as_bool()? {
                "inviteAccepted"
            } else {
                "inviteRejected"
            }
        }
        ChangeUserManagementPermission => {
            if update.data_as_bool()? {
                "userManagementRestricted"
            } else {
                "userManagementUnrestricted"
            }
        }
        ChangeGroupEditsPermission => {
            if update.data_as_bool()? {
                "groupEditsRestricted"
            } else {
                "groupEditsUnrestricted"
            }
        }
        ChangePostingPermission => {
            if update.data_as_bool()? {
                "postingRestricted"
            } else {
                "postingUnrestricted"
            }
        }

        // Private to one side, or about a conversation that no longer exists.
        ChangeMutedUntil | SetReadReceiptSettings | SetTypingIndicatorSettings | Delete => {
            return None
        }
    };

    Some(SystemMessageView {
        id: update.id,
        thread: update.target,
        actor: update.actor,
        kind: name.into(),
        subject,
        value,
        timestamp: update.timestamp,
    })
}

/// Turn a stored direct-message update into a row.
///
/// `my_id` recovers the counterparty from the thread's XOR, because the
/// interface threads a conversation under the other person's id.
fn system_message_for_dm_update(update: &UpdateDm, my_id: Uuid) -> Option<SystemMessageView> {
    let kind = update.kind().ok()?;
    let mut value = None;

    let name = match kind {
        UpdateDmType::ChangeRetention => {
            let seconds = <[u8; 8]>::try_from(update.data.as_slice())
                .ok()
                .map(i64::from_le_bytes)?;
            value = Some(retention_label(seconds));
            "retentionChanged"
        }
        UpdateDmType::SetClearBefore => "historyCleared",
        _ => return None,
    };

    Some(SystemMessageView {
        id: update.id,
        // A note to self has no counterparty to XOR out and threads under our
        // own id, which is what `xor` already yields when the target is nil.
        thread: crate::xor(update.target, my_id),
        actor: update.actor,
        kind: name.into(),
        subject: None,
        value,
        timestamp: update.timestamp,
    })
}

/// Name a retention period the way both clients do.
///
/// Presets get their exact label. Anything else — a value set by a build with
/// different presets, or by a future one — is spelled out in the largest units
/// that divide it, which is what the Go client's `durafmt` produces.
fn retention_label(seconds: i64) -> String {
    if let Some((_, label)) = RETENTION_LABELS
        .iter()
        .find(|(preset, _)| *preset == seconds)
    {
        return (*label).to_string();
    }
    if seconds < 0 {
        return "Off".into();
    }

    let units = [
        (7 * 24 * 60 * 60, "week"),
        (24 * 60 * 60, "day"),
        (60 * 60, "hour"),
        (60, "minute"),
        (1, "second"),
    ];

    let mut remaining = seconds;
    let mut parts = Vec::new();
    for (size, unit) in units {
        let count = remaining / size;
        if count > 0 {
            parts.push(format!("{count} {unit}{}", if count == 1 { "" } else { "s" }));
            remaining -= count * size;
        }
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::SignedFrame;

    fn update(kind: UpdateGroupType, data: Vec<u8>) -> UpdateGroup {
        UpdateGroup {
            signed: SignedFrame::default(),
            id: Uuid::new_v4(),
            actor: Uuid::new_v4(),
            target: Uuid::new_v4(),
            timestamp: 1_700_000_000,
            saved_at: 0,
            update_type: kind.as_u16(),
            data,
            custom_scope: Uuid::nil(),
            confirmations: Vec::new(),
            applied: false,
            notified: false,
            seen: false,
        }
    }

    #[test]
    fn presets_use_the_labels_the_go_client_shows() {
        assert_eq!(retention_label(0), "Off");
        assert_eq!(retention_label(3600), "1 Hour");
        assert_eq!(retention_label(86_400), "1 Day");
        assert_eq!(retention_label(604_800), "1 Week");
        // Four weeks, which is what that client means by a month.
        assert_eq!(retention_label(2_419_200), "1 Month");
    }

    #[test]
    fn other_periods_are_spelled_out() {
        assert_eq!(retention_label(90 * 60), "1 hour 30 minutes");
        assert_eq!(retention_label(2 * 24 * 60 * 60 + 1), "2 days 1 second");
    }

    #[test]
    fn a_rename_carries_the_new_name() {
        let view = system_message_for_group_update(&update(
            UpdateGroupType::ChangeName,
            b"Book Club".to_vec(),
        ))
        .expect("a rename is shown");

        assert_eq!(view.kind, "groupRenamed");
        assert_eq!(view.value.as_deref(), Some("Book Club"));
        assert!(view.subject.is_none());
    }

    #[test]
    fn a_permission_change_picks_a_kind_from_its_flag() {
        let restricted = system_message_for_group_update(&update(
            UpdateGroupType::ChangePostingPermission,
            UpdateGroup::encode_bool(true),
        ))
        .expect("shown");
        let unrestricted = system_message_for_group_update(&update(
            UpdateGroupType::ChangePostingPermission,
            UpdateGroup::encode_bool(false),
        ))
        .expect("shown");

        assert_eq!(restricted.kind, "postingRestricted");
        assert_eq!(unrestricted.kind, "postingUnrestricted");
    }

    #[test]
    fn a_removal_names_who_was_removed() {
        let target = Uuid::new_v4();
        let view = system_message_for_group_update(&update(
            UpdateGroupType::RemoveUser,
            target.as_bytes().to_vec(),
        ))
        .expect("shown");

        assert_eq!(view.kind, "userRemoved");
        assert_eq!(view.subject.as_deref(), Some(target.to_string().as_str()));
    }

    #[test]
    fn private_settings_get_no_row() {
        // Muting is one device owner's business, and a row announcing it to the
        // group would leak it.
        assert!(system_message_for_group_update(&update(
            UpdateGroupType::ChangeMutedUntil,
            UpdateGroup::encode_i64(0)
        ))
        .is_none());
        assert!(system_message_for_group_update(&update(
            UpdateGroupType::SetReadReceiptSettings,
            UpdateGroup::encode_bool(true)
        ))
        .is_none());
    }

    #[test]
    fn a_malformed_payload_produces_no_row_rather_than_a_wrong_one() {
        // A promotion whose payload is not a user id says nothing useful, and
        // guessing would put somebody else's name in the history.
        assert!(
            system_message_for_group_update(&update(UpdateGroupType::PromoteAdmin, vec![1, 2, 3]))
                .is_none()
        );
    }

    #[test]
    fn a_conversation_row_threads_under_the_counterparty() {
        let me = Uuid::new_v4();
        let them = Uuid::new_v4();

        let dm_update = UpdateDm {
            signed: SignedFrame::default(),
            id: Uuid::new_v4(),
            actor: them,
            target: crate::xor(me, them),
            update_type: UpdateDmType::ChangeRetention.as_u16(),
            data: 3600i64.to_le_bytes().to_vec(),
            timestamp: 1_700_000_000,
            saved_at: 0,
            seen: false,
        };

        let view = system_message_for_dm_update(&dm_update, me).expect("shown");
        assert_eq!(view.thread, them);
        assert_eq!(view.kind, "retentionChanged");
        assert_eq!(view.value.as_deref(), Some("1 Hour"));
    }
}
