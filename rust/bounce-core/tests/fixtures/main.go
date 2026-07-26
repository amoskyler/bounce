package main

// Interop harness: encodes/decodes the same structures the Bounce Go engine
// uses, so the Rust port can be checked against real Go output.

import (
	"crypto/ed25519"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"

	"github.com/Basekick-Labs/msgpack/v6"
	"github.com/google/uuid"
	"github.com/zeebo/blake3"
)

type signedContainer struct {
	Signer    string
	Payload   []byte
	Signature []byte
}

type fileAttachment struct {
	ID        uuid.UUID
	FileID    uuid.UUID
	MessageID uuid.UUID
	Name      string
	Size      int64
}

type imageAttachment struct {
	ID        uuid.UUID
	FileID    uuid.UUID
	MessageID uuid.UUID
	Name      string
	Size      int64
	Width     int
	Height    int
	BlurHash  string
}

type directMessage struct {
	ID               uuid.UUID
	WrittenAt        int64
	DeleteAt         int64
	Author           uuid.UUID
	Xor              uuid.UUID
	Text             string
	FileAttachments  []fileAttachment
	ImageAttachments []imageAttachment
}

type introductionSignature struct {
	ID                           uuid.UUID
	DeviceID                     uuid.UUID
	PreexistingDevice            string
	SignatureOfNewDevice         []byte
	SignatureOfPreexistingDevice []byte
}

type device struct {
	ID        uuid.UUID
	UserID    uuid.UUID
	Address   string
	Timestamp int64
	RevokedAt int64
	Signature *introductionSignature
}

type user struct {
	ID               uuid.UUID
	Name             string
	Images           string
	EncryptedDevices string
	PublicECDHKey    []byte
	Devices          []device
}

type group struct {
	ID                     uuid.UUID
	Name                   string
	Images                 string
	CreatedBy              uuid.UUID
	CreatedAt              int64
	Retention              int64
	ClearBefore            int64
	MutedUntil             int64
	Users                  []user
	Admins                 string
	Invites                string
	BlockedUsers           string
	RestrictUserManagement bool
	RestrictGroupEdits     bool
	RestrictPosting        bool
	LastActivity           int64
}

// Mirrors chat/file.go. Key and Nonce are empty for an unencrypted file,
// which is the case that used to be encoded as an empty array rather than an
// empty bin and refused here.
type file struct {
	ID                uuid.UUID
	Name              string
	Type              int
	AttachedTo        uuid.UUID
	Hash              string
	Size              int64
	ChunkSize         int
	HashList          string
	EncryptedHashList string
	Key               []byte
	Nonce             []byte
	Scope             int
	Destination       uuid.UUID
	Author            uuid.UUID
	Timestamp         int64
}

type chunkOffer struct {
	ID          uuid.UUID
	Scope       int
	Destination uuid.UUID
	Author      uuid.UUID
	FileID      uuid.UUID
	Hash        string
	Location    string
	Timestamp   int64
}

type groupCreation struct {
	ID        uuid.UUID
	Timestamp int64
	Data      []byte
}

// Mirrors chat/typing_indicator.go. Thread is xor(sender, recipient) for a
// direct message, and the group ID for a group.
type typingIndicator struct {
	ID          uuid.UUID
	Thread      uuid.UUID
	MessageType uint16
	Author      uuid.UUID
}

type frameReference struct {
	FrameID uuid.UUID
	Type    uint16
}

type ack struct {
	References []frameReference
}

func main() {
	switch os.Args[1] {
	case "emit":
		emit()
	case "verify":
		verify()
	}
}

// emit produces Go-encoded fixtures for the Rust side to decode.
func emit() {
	seed := make([]byte, 32)
	for i := range seed {
		seed[i] = byte(i)
	}
	priv := ed25519.NewKeyFromSeed(seed)
	pub := priv.Public().(ed25519.PublicKey)

	msgID := uuid.MustParse("aaaaaaaa-0000-4000-8000-000000000001")
	author := uuid.MustParse("bbbbbbbb-0000-4000-8000-000000000002")
	xorID := uuid.MustParse("cccccccc-0000-4000-8000-000000000003")

	dm := directMessage{
		ID:        msgID,
		WrittenAt: 1700000000,
		DeleteAt:  1700086400,
		Author:    author,
		Xor:       xorID,
		Text:      "hello from Go 👋",
		FileAttachments: []fileAttachment{{
			ID:        uuid.MustParse("dddddddd-0000-4000-8000-000000000004"),
			FileID:    uuid.MustParse("eeeeeeee-0000-4000-8000-000000000005"),
			MessageID: msgID,
			Name:      "report.pdf",
			Size:      4096,
		}},
		ImageAttachments: []imageAttachment{},
	}

	body, err := msgpack.Marshal(&dm)
	if err != nil {
		panic(err)
	}
	digest := blake3.Sum256(body)
	sig := ed25519.Sign(priv, digest[:])

	sc := signedContainer{Signer: "PLACEHOLDER", Payload: body, Signature: sig}
	scBytes, err := msgpack.Marshal(sc)
	if err != nil {
		panic(err)
	}

	// Group creation, so the Rust side can check ID derivation.
	creator := uuid.MustParse("11111111-0000-4000-8000-000000000006")
	g := group{
		Name:      "Interop Group",
		CreatedBy: creator,
		CreatedAt: 1700000000,
		Admins:    creator.String(),
		Users: []user{{
			ID:            creator,
			Name:          "Creator",
			PublicECDHKey: []byte{1, 2, 3},
			Devices: []device{{
				ID:        uuid.MustParse("22222222-0000-4000-8000-000000000007"),
				UserID:    creator,
				Address:   "someonionaddress",
				Timestamp: 1700000000,
			}},
		}},
	}
	gData, err := msgpack.Marshal(&g)
	if err != nil {
		panic(err)
	}
	hasher := blake3.New()
	hasher.Write(gData)
	dig := hasher.Digest()
	gHash := make([]byte, 16)
	dig.Read(gHash)
	gID, _ := uuid.FromBytes(gHash)

	gc := groupCreation{ID: gID, Timestamp: 1700000000, Data: gData}
	gcBytes, _ := msgpack.Marshal(&gc)

	// A typing indicator as TypingInDirectMessage builds one:
	// broadcastTypingIndicator(xor(userID, currentUserID), typeDirectMessage).
	sender := uuid.MustParse("bbbbbbbb-0000-4000-8000-000000000002")
	recipient := uuid.MustParse("cccccccc-0000-4000-8000-000000000003")
	var pair uuid.UUID
	for i := range pair {
		pair[i] = sender[i] ^ recipient[i]
	}
	ti := typingIndicator{
		ID:          uuid.MustParse("99999999-0000-4000-8000-000000000009"),
		Thread:      pair,
		MessageType: 0, // typeDirectMessage
		Author:      sender,
	}
	tiBytes, _ := msgpack.Marshal(&ti)

	a := ack{References: []frameReference{
		{FrameID: msgID, Type: 0},
		{FrameID: xorID, Type: 13},
	}}
	ackBytes, _ := msgpack.Marshal(&a)

	out := map[string]string{
		"ed25519_public_key":    hex.EncodeToString(pub),
		"direct_message_body":   hex.EncodeToString(body),
		"signed_container":      hex.EncodeToString(scBytes),
		"body_blake3":           hex.EncodeToString(digest[:]),
		"signature":             hex.EncodeToString(sig),
		"group_data":            hex.EncodeToString(gData),
		"group_id":              gID.String(),
		"group_creation":        hex.EncodeToString(gcBytes),
		"ack":                   hex.EncodeToString(ackBytes),
		"typing_indicator":      hex.EncodeToString(tiBytes),
		"typing_sender":         sender.String(),
		"typing_recipient":      recipient.String(),
	}
	enc, _ := json.MarshalIndent(out, "", "  ")
	fmt.Println(string(enc))
}

// verify decodes Rust-produced fixtures and re-checks them with Go.
func verify() {
	var in map[string]string
	if err := json.NewDecoder(os.Stdin).Decode(&in); err != nil {
		panic(err)
	}

	results := map[string]interface{}{}

	scBytes, _ := hex.DecodeString(in["signed_container"])
	var sc signedContainer
	if err := msgpack.Unmarshal(scBytes, &sc); err != nil {
		results["signed_container_decoded"] = "error: " + err.Error()
	} else {
		results["signed_container_decoded"] = true
		results["signer"] = sc.Signer

		pub, _ := hex.DecodeString(in["ed25519_public_key"])
		digest := blake3.Sum256(sc.Payload)
		results["signature_valid"] = ed25519.Verify(ed25519.PublicKey(pub), digest[:], sc.Signature)

		var dm directMessage
		if err := msgpack.Unmarshal(sc.Payload, &dm); err != nil {
			results["direct_message_decoded"] = "error: " + err.Error()
		} else {
			results["direct_message_decoded"] = true
			results["text"] = dm.Text
			results["message_id"] = dm.ID.String()
			results["written_at"] = dm.WrittenAt
			results["file_attachment_count"] = len(dm.FileAttachments)
			if len(dm.FileAttachments) > 0 {
				results["file_attachment_name"] = dm.FileAttachments[0].Name
				results["file_attachment_size"] = dm.FileAttachments[0].Size
			}
		}
	}

	gcBytes, _ := hex.DecodeString(in["group_creation"])
	var gc groupCreation
	if err := msgpack.Unmarshal(gcBytes, &gc); err != nil {
		results["group_creation_decoded"] = "error: " + err.Error()
	} else {
		hasher := blake3.New()
		hasher.Write(gc.Data)
		dig := hasher.Digest()
		gHash := make([]byte, 16)
		dig.Read(gHash)
		derived, _ := uuid.FromBytes(gHash)
		results["group_creation_decoded"] = true
		results["group_id_matches_hash"] = derived == gc.ID
		var g group
		if err := msgpack.Unmarshal(gc.Data, &g); err != nil {
			results["group_decoded"] = "error: " + err.Error()
		} else {
			results["group_decoded"] = true
			results["group_name"] = g.Name
			results["group_user_count"] = len(g.Users)
			if len(g.Users) > 0 && len(g.Users[0].Devices) > 0 {
				results["group_device_address"] = g.Users[0].Devices[0].Address
			}
		}
	}

	ackBytes, _ := hex.DecodeString(in["ack"])
	var a ack
	if err := msgpack.Unmarshal(ackBytes, &a); err != nil {
		results["ack_decoded"] = "error: " + err.Error()
	} else {
		results["ack_decoded"] = true
		results["ack_reference_count"] = len(a.References)
		if len(a.References) > 1 {
			results["ack_second_type"] = a.References[1].Type
		}
	}

	fileBytes, _ := hex.DecodeString(in["file"])
	var f file
	if err := msgpack.Unmarshal(fileBytes, &f); err != nil {
		results["file_decoded"] = "error: " + err.Error()
	} else {
		results["file_decoded"] = true
		results["file_name"] = f.Name
		results["file_size"] = f.Size
		results["file_hash_list"] = f.HashList
		// The point of the fixture: an unencrypted file's key and nonce are
		// empty, and Go must read them as empty rather than fail the frame.
		results["file_key_empty"] = len(f.Key) == 0
		results["file_nonce_empty"] = len(f.Nonce) == 0
	}

	offerBytes, _ := hex.DecodeString(in["chunk_offer"])
	var co chunkOffer
	if err := msgpack.Unmarshal(offerBytes, &co); err != nil {
		results["chunk_offer_decoded"] = "error: " + err.Error()
	} else {
		results["chunk_offer_decoded"] = true
		results["chunk_offer_hash_matches"] = co.Hash == in["chunk_hash"]
		results["chunk_offer_location"] = co.Location
	}

	enc, _ := json.MarshalIndent(results, "", "  ")
	fmt.Println(string(enc))
}
