// A standalone copy of the Go client's transport handshake, over plain TCP.
//
// The handshake in `network/tor.go` is entangled with starting Tor and
// publishing an onion service, neither of which a test can afford. Everything
// below the socket is identical: the same byte order, the same fixed sizes, the
// same Ed25519 over the same XOR'd challenge, and the same v3 onion address
// derivation. Only the socket differs.
//
// Kept in step with upstream by construction — it is a transcription, and the
// commit it was taken from is named here:
//
//	upstream e553f68 "patch important vulnerabilities from claude audit,
//	including breaking change to handshakes"
//
// Modes:
//
//	dial <host:port>    complete the dialer's half, print our address
//	listen <host:port>  complete the listener's half, print the peer's address
//	vectors             emit deterministic test vectors as JSON
package main

import (
	"crypto/ed25519"
	"crypto/rand"
	"crypto/sha3"
	"encoding/base32"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"os"
)

const (
	handshakeChallengeSize = 32
	signatureSize          = 64
	onionAddressLength     = 56
)

// --- transport primitives, transcribed from network/tor.go ---

func read(conn net.Conn, size int) ([]byte, error) {
	payload := make([]byte, 0)
	payloadRead := 0
	for payloadRead < size {
		buf := make([]byte, size-payloadRead)
		n, err := conn.Read(buf)
		payloadRead += n
		if err == io.EOF {
			if payloadRead != size {
				return []byte{}, err
			}
		} else if err != nil {
			return []byte{}, err
		}
		payload = append(payload, buf[:n]...)
	}
	return payload, nil
}

func write(conn net.Conn, payload []byte) error {
	bytesToWrite := len(payload)
	bytesWritten := 0
	for bytesWritten < bytesToWrite {
		n, err := conn.Write(payload[bytesWritten:])
		if err != nil {
			return err
		}
		bytesWritten += n
	}
	return nil
}

func xor(a, b []byte) []byte {
	if len(a) != len(b) {
		panic("cannot XOR byte slices of different length")
	}
	dst := make([]byte, len(a))
	for i := range a {
		dst[i] = a[i] ^ b[i]
	}
	return dst
}

// --- v3 onion addressing ---
//
// torutil.OnionServiceIDFromV3PublicKey, without the dependency:
// base32(pubkey || checksum || version), where the checksum is the first two
// bytes of SHA3-256(".onion checksum" || pubkey || version).

var encoding = base32.StdEncoding.WithPadding(base32.NoPadding)

func onionServiceID(pub ed25519.PublicKey) string {
	checksumInput := make([]byte, 0, len(".onion checksum")+ed25519.PublicKeySize+1)
	checksumInput = append(checksumInput, ".onion checksum"...)
	checksumInput = append(checksumInput, pub...)
	checksumInput = append(checksumInput, 0x03)
	sum := sha3.Sum256(checksumInput)

	address := make([]byte, 0, ed25519.PublicKeySize+3)
	address = append(address, pub...)
	address = append(address, sum[0], sum[1])
	address = append(address, 0x03)

	return toLower(encoding.EncodeToString(address))
}

func publicKeyFromOnionServiceID(id string) (ed25519.PublicKey, error) {
	decoded, err := encoding.DecodeString(toUpper(id))
	if err != nil {
		return nil, err
	}
	if len(decoded) != ed25519.PublicKeySize+3 {
		return nil, fmt.Errorf("onion id decodes to %d bytes", len(decoded))
	}
	return ed25519.PublicKey(decoded[:ed25519.PublicKeySize]), nil
}

func toLower(s string) string {
	out := []byte(s)
	for i, c := range out {
		if c >= 'A' && c <= 'Z' {
			out[i] = c + 32
		}
	}
	return string(out)
}

func toUpper(s string) string {
	out := []byte(s)
	for i, c := range out {
		if c >= 'a' && c <= 'z' {
			out[i] = c - 32
		}
	}
	return string(out)
}

// --- the handshake itself ---

// dialHandshake is network/tor.go's Dial, from the challenge read onwards.
func dialHandshake(conn net.Conn, priv ed25519.PrivateKey, id string) error {
	challenge, err := read(conn, handshakeChallengeSize)
	if err != nil {
		return err
	}

	challengeXor := make([]byte, handshakeChallengeSize)
	n, err := rand.Read(challengeXor)
	if n != handshakeChallengeSize {
		return fmt.Errorf("failed to generate random challenge for handshake XOR")
	}
	if err != nil {
		return err
	}

	response := ed25519.Sign(priv, xor(challenge, challengeXor))

	if err := write(conn, []byte(id)); err != nil {
		return err
	}
	if err := write(conn, challengeXor); err != nil {
		return err
	}
	return write(conn, response)
}

// acceptHandshake is network/tor.go's Accept, from the challenge write onwards.
func acceptHandshake(conn net.Conn) (string, error) {
	challenge := make([]byte, handshakeChallengeSize)
	n, err := rand.Read(challenge)
	if n != handshakeChallengeSize {
		return "", fmt.Errorf("failed to generate random challenge for handshake")
	}
	if err != nil {
		return "", err
	}
	if err := write(conn, challenge); err != nil {
		return "", err
	}

	peerAddress, err := read(conn, onionAddressLength)
	if err != nil {
		return "", err
	}
	challengeXor, err := read(conn, handshakeChallengeSize)
	if err != nil {
		return "", err
	}
	response, err := read(conn, signatureSize)
	if err != nil {
		return "", err
	}

	pub, err := publicKeyFromOnionServiceID(string(peerAddress))
	if err != nil {
		return "", err
	}
	if !ed25519.Verify(pub, xor(challenge, challengeXor), response) {
		return "", fmt.Errorf("signature validation failed during handshake with %s", peerAddress)
	}
	return string(peerAddress), nil
}

func main() {
	if len(os.Args) < 2 {
		fail("usage: handshake dial|listen <addr> | handshake vectors")
	}

	switch os.Args[1] {
	case "dial":
		pub, priv, err := ed25519.GenerateKey(rand.Reader)
		must(err)
		id := onionServiceID(pub)

		conn, err := net.Dial("tcp", os.Args[2])
		must(err)
		defer conn.Close()
		must(dialHandshake(conn, priv, id))
		fmt.Println(id)

	case "listen":
		listener, err := net.Listen("tcp", os.Args[2])
		must(err)
		// The port, so the caller does not have to guess when it asked for :0.
		fmt.Println(listener.Addr().String())
		os.Stdout.Sync()

		conn, err := listener.Accept()
		must(err)
		defer conn.Close()
		peer, err := acceptHandshake(conn)
		must(err)
		fmt.Println(peer)

	case "vectors":
		// Deterministic, so the expected signature can be checked without Go
		// present. A fixed key and both halves of the challenge fixed.
		seed := make([]byte, ed25519.SeedSize)
		for i := range seed {
			seed[i] = byte(i)
		}
		priv := ed25519.NewKeyFromSeed(seed)
		pub := priv.Public().(ed25519.PublicKey)

		challenge := make([]byte, handshakeChallengeSize)
		challengeXor := make([]byte, handshakeChallengeSize)
		for i := range challenge {
			challenge[i] = byte(0xA0 + i)
			challengeXor[i] = byte(0x5C - i)
		}

		signed := xor(challenge, challengeXor)
		out := map[string]string{
			"seed":          hex.EncodeToString(seed),
			"address":       onionServiceID(pub),
			"challenge":     hex.EncodeToString(challenge),
			"challenge_xor": hex.EncodeToString(challengeXor),
			"signed_bytes":  hex.EncodeToString(signed),
			"signature":     hex.EncodeToString(ed25519.Sign(priv, signed)),
		}
		encoded, err := json.MarshalIndent(out, "", "  ")
		must(err)
		fmt.Println(string(encoded))

	default:
		fail("unknown mode " + os.Args[1])
	}
}

func must(err error) {
	if err != nil {
		fail(err.Error())
	}
}

func fail(message string) {
	fmt.Fprintln(os.Stderr, message)
	os.Exit(1)
}
