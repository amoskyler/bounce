//go:build darwin && !ios

package ui

import (
	"bytes"
	"image"
	"os"
	"os/exec"
	"path/filepath"
	"testing"

	"fyne.io/fyne/v2"

	"github.com/alecthomas/assert/v2"
)

// These exercise the real pasteboard, and so replace whatever the person
// running the tests had on it.  They only run when asked for by name.
func requireClipboardTests(t *testing.T) {
	t.Helper()

	if os.Getenv("BOUNCE_CLIPBOARD_TEST") == "" {
		t.Skip("set BOUNCE_CLIPBOARD_TEST=1 to run against the real system clipboard")
	}
}

// putOnPasteboard hands a file to the pasteboard through AppleScript, so that
// the data we read back was put there by something other than ourselves.
func putOnPasteboard(t *testing.T, path string, class string) {
	t.Helper()

	script := `set the clipboard to (read (POSIX file "` + path + `") as ` + class + `)`

	output, err := exec.Command("osascript", "-e", script).CombinedOutput()
	assert.NoError(t, err, string(output))
}

func writeSamplePNG(t *testing.T) string {
	t.Helper()

	path := filepath.Join(t.TempDir(), "sample.png")

	err := os.WriteFile(path, samplePNG(t), 0600)
	assert.NoError(t, err)

	return path
}

// A pasteboard carrying only TIFF is what plenty of native macOS applications
// offer, and is the path that has to convert before we ever see the bytes.
func TestClipboardReadsForeignTIFF(t *testing.T) {
	requireClipboardTests(t)

	putOnPasteboard(t, writeSamplePNG(t), `«class TIFF»`)

	data := clipboardImage()
	assert.True(t, len(data) > 0)

	got, format, err := image.Decode(bytes.NewReader(data))
	assert.NoError(t, err)

	// Whatever went in, what comes out has to be png, because that is all the
	// rest of the paste path is prepared to handle.
	assert.Equal(t, "png", format)
	assertSameImage(t, sampleImage(), got)
}

func TestClipboardReadsForeignPNG(t *testing.T) {
	requireClipboardTests(t)

	putOnPasteboard(t, writeSamplePNG(t), `«class PNGf»`)

	data := clipboardImage()
	assert.True(t, len(data) > 0)

	got, _, err := image.Decode(bytes.NewReader(data))
	assert.NoError(t, err)

	assertSameImage(t, sampleImage(), got)
}

// Copying a file in Finder puts a file url on the pasteboard rather than any
// pixels, and we attach the file itself in that case.
func TestClipboardReadsFilePaths(t *testing.T) {
	requireClipboardTests(t)

	path := writeSamplePNG(t)

	script := `set the clipboard to (POSIX file "` + path + `")`
	output, err := exec.Command("osascript", "-e", script).CombinedOutput()
	assert.NoError(t, err, string(output))

	paths := clipboardFilePaths()
	assert.Equal(t, 1, len(paths))

	// macOS resolves /var to /private/var, so compare what the paths point at
	// rather than the strings themselves.
	wanted, err := filepath.EvalSymlinks(path)
	assert.NoError(t, err)
	got, err := filepath.EvalSymlinks(paths[0])
	assert.NoError(t, err)
	assert.Equal(t, wanted, got)

	assert.True(t, isPastableImagePath(paths[0]))
}

// A file copied in a file manager puts only its url on the pasteboard, so a
// copied PDF has to come back as a file we decline to attach.  Nothing may turn
// it into an image on the way past.
func TestClipboardDoesNotAttachCopiedPDFAsImage(t *testing.T) {
	requireClipboardTests(t)

	path := filepath.Join(t.TempDir(), "sample.pdf")
	output, err := exec.Command(
		"sips", "-s", "format", "pdf",
		writeSamplePNG(t),
		"--out", path,
	).CombinedOutput()
	if err != nil {
		t.Skip("sips could not produce a pdf to test with: " + string(output))
	}

	output, err = exec.Command("osascript", "-e", `set the clipboard to (POSIX file "`+path+`")`).CombinedOutput()
	assert.NoError(t, err, string(output))

	// The file is on the clipboard, but it is not an image and must not be
	// offered as one.
	assert.Equal(t, 1, len(clipboardFilePaths()))
	assert.False(t, isPastableImagePath(clipboardFilePaths()[0]))
	assert.Equal(t, 0, len(clipboardImage()))
}

// A screenshot is the case the whole feature exists for, and it puts pixels on
// the pasteboard and nothing else, so the text check ahead of the image must
// not get in its way.
func TestClipboardImagePasteHasNoTextAlongsideIt(t *testing.T) {
	requireClipboardTests(t)

	putOnPasteboard(t, writeSamplePNG(t), `«class PNGf»`)

	assert.True(t, len(clipboardImage()) > 0)
	assert.Equal(t, "", fyne.CurrentApp().Clipboard().Content())
}

// Text on the pasteboard must not be mistaken for an image, or pasting text
// into the message box would stop working.
func TestClipboardIgnoresText(t *testing.T) {
	requireClipboardTests(t)

	output, err := exec.Command("osascript", "-e", `set the clipboard to "just some text"`).CombinedOutput()
	assert.NoError(t, err, string(output))

	assert.Equal(t, 0, len(clipboardImage()))
	assert.Equal(t, 0, len(clipboardFilePaths()))
}
