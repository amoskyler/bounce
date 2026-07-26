//! Device group validation.
//!
//! A user is defined by the set of devices they own, and that set is held
//! together by mutual signatures rather than by any authority. Each device
//! beyond the first carries an [`IntroductionSignature`]: the existing device
//! signed the newcomer's address, and the newcomer signed the existing device's
//! address. Both halves are required, so a device cannot be added without its
//! own consent, and cannot add itself without an existing member's consent.
//!
//! A device group is valid when:
//!
//! 1. it has at least one device;
//! 2. exactly one device — the founder — has no introduction signature;
//! 3. every signature in the group verifies against the address that made it;
//! 4. no device was admitted by a device that had already been revoked;
//! 5. the founder signed at least one other device; and
//! 6. the signature graph is connected.
//!
//! Rules 5 and 6 are what stop a peer from grafting a device group of its own
//! onto a real user: any disconnected component, however internally consistent,
//! fails the traversal.
//!
//! ```text
//!        phone (founder)
//!         /        \
//!     laptop      tablet          valid: connected, rooted at the founder
//!        |
//!      watch
//!
//!        phone (founder)          invalid: {desktop, server} is a separate
//!         /                       component that the founder never signed
//!     laptop      desktop — server
//! ```

use std::collections::{HashMap, HashSet};

use crate::crypto;
use crate::frames::identity::{Device, User};

/// Why a device group was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invalid {
    /// A user must own at least one device.
    NoDevices,
    /// More than one device claims to be the founder.
    MultipleFoundersFound,
    /// A device was admitted by one that had already been revoked.
    AddedByRevokedDevice { device: String, signer: String },
    /// A device names an introducer that is not in the group.
    IntroducerNotInGroup { device: String, signer: String },
    /// One half of a mutual signature does not verify.
    BadSignature { signer: String, target: String },
    /// The founding device never signed anybody.
    FounderSignedNobody,
    /// The signature graph has more than one component.
    Disconnected,
    /// Two devices claim the same address, which is a contradiction: an address
    /// is a public key.
    DuplicateAddress(String),
}

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Invalid::NoDevices => write!(f, "device group has no devices"),
            Invalid::MultipleFoundersFound => {
                write!(f, "device group has more than one unsigned device")
            }
            Invalid::AddedByRevokedDevice { device, signer } => write!(
                f,
                "device {device} was added by {signer}, which was already revoked"
            ),
            Invalid::IntroducerNotInGroup { device, signer } => write!(
                f,
                "device {device} names introducer {signer}, which is not in the group"
            ),
            Invalid::BadSignature { signer, target } => {
                write!(f, "invalid signature by {signer} over {target}")
            }
            Invalid::FounderSignedNobody => {
                write!(f, "the founding device did not sign any other device")
            }
            Invalid::Disconnected => write!(f, "devices do not form a connected graph"),
            Invalid::DuplicateAddress(address) => {
                write!(f, "two devices claim the address {address}")
            }
        }
    }
}

/// One edge of the signature graph.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MutualSignature {
    /// The device being introduced.
    new_device: String,
    /// The device already in the group.
    preexisting_device: String,
    /// The new device's signature over the existing device's address.
    new_signs_preexisting: Vec<u8>,
    /// The existing device's signature over the new device's address.
    preexisting_signs_new: Vec<u8>,
}

/// Check whether a set of devices constitutes a valid device group.
pub fn validate(devices: &[Device]) -> Result<(), Invalid> {
    if devices.is_empty() {
        return Err(Invalid::NoDevices);
    }

    // An address is a public key, so two devices holding one is impossible.
    // Left unchecked it also breaks the storage layer's uniqueness constraint
    // half-way through writing a contact.
    let mut addresses: HashSet<&str> = HashSet::new();
    for device in devices {
        if !addresses.insert(device.address.as_str()) {
            return Err(Invalid::DuplicateAddress(device.address.clone()));
        }
    }

    let revoked_times: HashMap<&str, i64> = devices
        .iter()
        .map(|d| (d.address.as_str(), d.revoked_at))
        .collect();

    let mut signatures: Vec<MutualSignature> = Vec::new();
    let mut founder: Option<&str> = None;
    let mut signers: HashSet<&str> = HashSet::new();

    for device in devices {
        let Some(introduction) = &device.signature else {
            // A group has exactly one device that nobody introduced.
            if founder.is_some() {
                return Err(Invalid::MultipleFoundersFound);
            }
            founder = Some(&device.address);
            continue;
        };

        signers.insert(&introduction.preexisting_device);

        // The introducer must be a device we can account for, and it must not
        // have been revoked before it did the introducing. Otherwise a stolen,
        // revoked device could keep minting new members.
        match revoked_times.get(introduction.preexisting_device.as_str()) {
            None => {
                return Err(Invalid::IntroducerNotInGroup {
                    device: device.address.clone(),
                    signer: introduction.preexisting_device.clone(),
                })
            }
            Some(&revoked_at) if revoked_at != 0 && revoked_at < device.timestamp => {
                return Err(Invalid::AddedByRevokedDevice {
                    device: device.address.clone(),
                    signer: introduction.preexisting_device.clone(),
                })
            }
            Some(_) => {}
        }

        signatures.push(MutualSignature {
            new_device: device.address.clone(),
            preexisting_device: introduction.preexisting_device.clone(),
            new_signs_preexisting: introduction.signature_of_preexisting_device.clone(),
            preexisting_signs_new: introduction.signature_of_new_device.clone(),
        });
    }

    // A lone founder is a complete, valid group.
    if signatures.is_empty() {
        return Ok(());
    }

    // With signatures present there must be a founder, and it must have
    // introduced somebody — otherwise the group is a chain with no root.
    match founder {
        Some(address) if signers.contains(address) => {}
        _ => return Err(Invalid::FounderSignedNobody),
    }

    let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
    for signature in &signatures {
        // Each half of the pair is checked against the address that made it.
        if !crypto::verify_signature(
            &signature.new_device,
            signature.preexisting_device.as_bytes(),
            &signature.new_signs_preexisting,
        ) {
            return Err(Invalid::BadSignature {
                signer: signature.new_device.clone(),
                target: signature.preexisting_device.clone(),
            });
        }
        if !crypto::verify_signature(
            &signature.preexisting_device,
            signature.new_device.as_bytes(),
            &signature.preexisting_signs_new,
        ) {
            return Err(Invalid::BadSignature {
                signer: signature.preexisting_device.clone(),
                target: signature.new_device.clone(),
            });
        }

        adjacency
            .entry(&signature.new_device)
            .or_default()
            .push(&signature.preexisting_device);
        adjacency
            .entry(&signature.preexisting_device)
            .or_default()
            .push(&signature.new_device);
    }

    // Every device that participates in a signature must be reachable from any
    // one of them.
    let start = signatures[0].new_device.as_str();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut stack = vec![start];
    while let Some(node) = stack.pop() {
        if !seen.insert(node) {
            continue;
        }
        for neighbour in adjacency.get(node).into_iter().flatten() {
            if !seen.contains(*neighbour) {
                stack.push(neighbour);
            }
        }
    }

    if seen.len() != adjacency.len() {
        return Err(Invalid::Disconnected);
    }

    Ok(())
}

/// Whether the devices belonging to `user` form a valid group.
pub fn user_has_valid_device_group(user: &User) -> bool {
    validate(&user.devices).is_ok()
}

/// Whether adding `device` to `user` would leave a valid group.
///
/// This is the check applied to every device frame that arrives: a device is
/// only accepted if it slots into the existing group without breaking it.
pub fn is_valid_addition(user: &User, device: &Device) -> bool {
    let mut devices = user.devices.clone();
    devices.push(device.clone());
    validate(&devices).is_ok()
}

/// Build the signature pair that admits `new_key`'s device to a group, given
/// the existing device's key.
///
/// Returns the two signatures as `(preexisting_signs_new, new_signs_preexisting)`.
pub fn create_introduction_signatures(
    preexisting_key: &crypto::DeviceKey,
    new_key: &crypto::DeviceKey,
) -> (Vec<u8>, Vec<u8>) {
    let preexisting_address = preexisting_key.address();
    let new_address = new_key.address();
    (
        preexisting_key.sign(new_address.as_bytes()).to_vec(),
        new_key.sign(preexisting_address.as_bytes()).to_vec(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::identity::IntroductionSignature;
    use uuid::Uuid;

    /// A device group under construction, so tests can describe topology
    /// instead of key material.
    struct GroupBuilder {
        user_id: Uuid,
        keys: HashMap<String, crypto::DeviceKey>,
        devices: Vec<Device>,
        clock: i64,
    }

    impl GroupBuilder {
        fn new() -> Self {
            GroupBuilder {
                user_id: Uuid::new_v4(),
                keys: HashMap::new(),
                devices: Vec::new(),
                clock: 1_000,
            }
        }

        /// Add the founding device, which nobody introduced.
        fn founder(mut self, label: &str) -> Self {
            let key = crypto::DeviceKey::generate();
            self.clock += 1;
            self.devices.push(Device::new(
                Uuid::new_v4(),
                self.user_id,
                key.address(),
                self.clock,
            ));
            self.keys.insert(label.to_string(), key);
            self
        }

        /// Add a device introduced by an existing one.
        fn introduce(mut self, label: &str, by: &str) -> Self {
            let introducer = &self.keys[by];
            let new_key = crypto::DeviceKey::generate();
            let (preexisting_signs_new, new_signs_preexisting) =
                create_introduction_signatures(introducer, &new_key);

            self.clock += 1;
            let mut device = Device::new(
                Uuid::new_v4(),
                self.user_id,
                new_key.address(),
                self.clock,
            );
            device.signature = Some(IntroductionSignature {
                id: Uuid::new_v4(),
                device_id: device.id,
                preexisting_device: introducer.address(),
                signature_of_new_device: preexisting_signs_new,
                signature_of_preexisting_device: new_signs_preexisting,
            });

            self.devices.push(device);
            self.keys.insert(label.to_string(), new_key);
            self
        }

        fn address_of(&self, label: &str) -> String {
            self.keys[label].address()
        }

        fn device_mut(&mut self, label: &str) -> &mut Device {
            let address = self.address_of(label);
            self.devices
                .iter_mut()
                .find(|d| d.address == address)
                .expect("device exists")
        }

        fn build(self) -> Vec<Device> {
            self.devices
        }
    }

    #[test]
    fn two_devices_sharing_an_address_are_rejected() {
        // An address is a public key; a group claiming one twice is incoherent,
        // and would also break the unique constraint mid-write.
        let mut builder = GroupBuilder::new()
            .founder("phone")
            .introduce("laptop", "phone");

        let duplicate = builder.device_mut("laptop").clone();
        builder.devices.push(duplicate);

        assert!(matches!(
            validate(&builder.build()),
            Err(Invalid::DuplicateAddress(_))
        ));
    }

    #[test]
    fn a_lone_founder_is_a_valid_group() {
        let devices = GroupBuilder::new().founder("phone").build();
        assert_eq!(validate(&devices), Ok(()));
    }

    #[test]
    fn an_empty_group_is_rejected() {
        assert_eq!(validate(&[]), Err(Invalid::NoDevices));
    }

    #[test]
    fn the_documented_chain_validates() {
        // phone introduces laptop, laptop introduces watch, phone introduces
        // tablet: a connected tree rooted at the founder.
        let devices = GroupBuilder::new()
            .founder("phone")
            .introduce("laptop", "phone")
            .introduce("watch", "laptop")
            .introduce("tablet", "phone")
            .build();

        assert_eq!(validate(&devices), Ok(()));
    }

    #[test]
    fn two_unintroduced_devices_are_rejected() {
        let mut builder = GroupBuilder::new()
            .founder("phone")
            .introduce("laptop", "phone");
        // A second founder, spliced in.
        let stray = crypto::DeviceKey::generate();
        builder.devices.push(Device::new(
            Uuid::new_v4(),
            builder.user_id,
            stray.address(),
            2_000,
        ));

        assert_eq!(validate(&builder.build()), Err(Invalid::MultipleFoundersFound));
    }

    #[test]
    fn a_group_with_no_founder_is_rejected() {
        // Two devices that introduced each other: internally consistent, but
        // with no unsigned device the group has no root of trust.
        let user_id = Uuid::new_v4();
        let first = crypto::DeviceKey::generate();
        let second = crypto::DeviceKey::generate();

        let (first_signs_second, second_signs_first) =
            create_introduction_signatures(&first, &second);

        let mut first_device = Device::new(Uuid::new_v4(), user_id, first.address(), 1_000);
        first_device.signature = Some(IntroductionSignature {
            id: Uuid::new_v4(),
            device_id: first_device.id,
            preexisting_device: second.address(),
            signature_of_new_device: second_signs_first.clone(),
            signature_of_preexisting_device: first_signs_second.clone(),
        });

        let mut second_device = Device::new(Uuid::new_v4(), user_id, second.address(), 1_001);
        second_device.signature = Some(IntroductionSignature {
            id: Uuid::new_v4(),
            device_id: second_device.id,
            preexisting_device: first.address(),
            signature_of_new_device: first_signs_second,
            signature_of_preexisting_device: second_signs_first,
        });

        assert_eq!(
            validate(&[first_device, second_device]),
            Err(Invalid::FounderSignedNobody)
        );
    }

    #[test]
    fn a_device_whose_introducer_left_the_group_is_rejected() {
        let mut builder = GroupBuilder::new()
            .founder("phone")
            .introduce("laptop", "phone")
            .introduce("watch", "laptop");

        // Drop the laptop, orphaning the watch that it introduced.
        let laptop_address = builder.address_of("laptop");
        builder.devices.retain(|d| d.address != laptop_address);

        assert!(matches!(
            validate(&builder.build()),
            Err(Invalid::IntroducerNotInGroup { .. })
        ));
    }

    #[test]
    fn a_forged_signature_is_rejected() {
        let mut builder = GroupBuilder::new()
            .founder("phone")
            .introduce("laptop", "phone");

        // Corrupt the introducer's half of the pair.
        let laptop = builder.device_mut("laptop");
        let signature = laptop.signature.as_mut().unwrap();
        signature.signature_of_new_device = vec![0u8; 64];

        assert!(matches!(
            validate(&builder.build()),
            Err(Invalid::BadSignature { .. })
        ));
    }

    #[test]
    fn a_missing_consent_signature_is_rejected() {
        let mut builder = GroupBuilder::new()
            .founder("phone")
            .introduce("laptop", "phone");

        // Corrupt the newcomer's half: the device never consented to be added.
        let laptop = builder.device_mut("laptop");
        let signature = laptop.signature.as_mut().unwrap();
        signature.signature_of_preexisting_device = vec![0u8; 64];

        assert!(matches!(
            validate(&builder.build()),
            Err(Invalid::BadSignature { .. })
        ));
    }

    #[test]
    fn a_device_added_by_a_revoked_device_is_rejected() {
        let mut builder = GroupBuilder::new()
            .founder("phone")
            .introduce("laptop", "phone")
            .introduce("stolen_addition", "laptop");

        let addition_timestamp = {
            let addition = builder.device_mut("stolen_addition");
            addition.timestamp
        };

        // Revoke the laptop before it introduced the new device.
        let laptop = builder.device_mut("laptop");
        laptop.revoked_at = addition_timestamp - 1;

        assert!(matches!(
            validate(&builder.build()),
            Err(Invalid::AddedByRevokedDevice { .. })
        ));
    }

    #[test]
    fn a_device_added_before_its_introducer_was_revoked_is_kept() {
        let mut builder = GroupBuilder::new()
            .founder("phone")
            .introduce("laptop", "phone")
            .introduce("watch", "laptop");

        let addition_timestamp = builder.device_mut("watch").timestamp;

        // Revoked afterwards: the historic introduction still stands.
        let laptop = builder.device_mut("laptop");
        laptop.revoked_at = addition_timestamp + 100;

        assert_eq!(validate(&builder.build()), Ok(()));
    }

    #[test]
    fn a_grafted_component_is_rejected() {
        // Build a legitimate group, then splice in a self-consistent pair of
        // devices that the founder never signed.
        let mut legitimate = GroupBuilder::new()
            .founder("phone")
            .introduce("laptop", "phone");

        let intruder_root = crypto::DeviceKey::generate();
        let intruder_leaf = crypto::DeviceKey::generate();
        let (root_signs_leaf, leaf_signs_root) =
            create_introduction_signatures(&intruder_root, &intruder_leaf);

        // The intruder's root looks like a normal member...
        let mut root_device = Device::new(
            Uuid::new_v4(),
            legitimate.user_id,
            intruder_root.address(),
            5_000,
        );
        // ...introduced by its own leaf, so the pair is internally consistent
        // but attached to nothing in the real group.
        let (leaf_signs_root_2, root_signs_leaf_2) =
            create_introduction_signatures(&intruder_leaf, &intruder_root);
        root_device.signature = Some(IntroductionSignature {
            id: Uuid::new_v4(),
            device_id: root_device.id,
            preexisting_device: intruder_leaf.address(),
            signature_of_new_device: leaf_signs_root_2,
            signature_of_preexisting_device: root_signs_leaf_2,
        });

        let mut leaf_device = Device::new(
            Uuid::new_v4(),
            legitimate.user_id,
            intruder_leaf.address(),
            5_001,
        );
        leaf_device.signature = Some(IntroductionSignature {
            id: Uuid::new_v4(),
            device_id: leaf_device.id,
            preexisting_device: intruder_root.address(),
            signature_of_new_device: root_signs_leaf,
            signature_of_preexisting_device: leaf_signs_root,
        });

        legitimate.devices.push(root_device);
        legitimate.devices.push(leaf_device);

        assert_eq!(validate(&legitimate.build()), Err(Invalid::Disconnected));
    }

    #[test]
    fn a_device_naming_an_unknown_introducer_is_rejected() {
        let mut builder = GroupBuilder::new().founder("phone");

        let outsider = crypto::DeviceKey::generate();
        let newcomer = crypto::DeviceKey::generate();
        let (outsider_signs_new, new_signs_outsider) =
            create_introduction_signatures(&outsider, &newcomer);

        let mut device = Device::new(
            Uuid::new_v4(),
            builder.user_id,
            newcomer.address(),
            2_000,
        );
        device.signature = Some(IntroductionSignature {
            id: Uuid::new_v4(),
            device_id: device.id,
            preexisting_device: outsider.address(),
            signature_of_new_device: outsider_signs_new,
            signature_of_preexisting_device: new_signs_outsider,
        });
        builder.devices.push(device);

        assert!(matches!(
            validate(&builder.build()),
            Err(Invalid::IntroducerNotInGroup { .. })
        ));
    }

    #[test]
    fn is_valid_addition_accepts_a_properly_introduced_device() {
        let builder = GroupBuilder::new().founder("phone");
        let founder_key = builder.keys["phone"].clone();
        let user_id = builder.user_id;

        let mut user = User::new(user_id, "Alice".into());
        user.devices = builder.build();

        let new_key = crypto::DeviceKey::generate();
        let (preexisting_signs_new, new_signs_preexisting) =
            create_introduction_signatures(&founder_key, &new_key);

        let mut newcomer = Device::new(Uuid::new_v4(), user_id, new_key.address(), 2_000);
        newcomer.signature = Some(IntroductionSignature {
            id: Uuid::new_v4(),
            device_id: newcomer.id,
            preexisting_device: founder_key.address(),
            signature_of_new_device: preexisting_signs_new,
            signature_of_preexisting_device: new_signs_preexisting,
        });

        assert!(is_valid_addition(&user, &newcomer));
        assert!(user_has_valid_device_group(&user));
    }

    #[test]
    fn is_valid_addition_rejects_an_uninvited_device() {
        let builder = GroupBuilder::new().founder("phone");
        let user_id = builder.user_id;

        let mut user = User::new(user_id, "Alice".into());
        user.devices = builder.build();

        // A device that simply asserts membership, with no introduction.
        let intruder = crypto::DeviceKey::generate();
        let device = Device::new(Uuid::new_v4(), user_id, intruder.address(), 2_000);

        assert!(!is_valid_addition(&user, &device));
    }
}
