package ui

import (
	"bytes"
	"image"
	"image/jpeg"
	"image/png"
	"os"
	"path/filepath"
	"testing"

	"fyne.io/fyne/v2/test"
	"github.com/alecthomas/assert/v2"
	"github.com/bounce-chat/bounce/chat"
	"github.com/bounce-chat/bounce/config"
)

// Staging an attachment goes through fyne's storage repositories, which are
// registered by whichever driver is in use, so the tests need one standing up.
func TestMain(m *testing.M) {
	test.NewApp()

	os.Exit(m.Run())
}

func samplePNG(t *testing.T) []byte {
	t.Helper()

	encoded := &bytes.Buffer{}
	err := png.Encode(encoded, sampleImage())
	assert.NoError(t, err)

	return encoded.Bytes()
}

func TestStageClipboardImage(t *testing.T) {
	data := samplePNG(t)

	reader, err := stageClipboardImage(data)
	assert.NoError(t, err)
	defer reader.Close()

	// The name is what the recipient sees on the message, so it has to carry the
	// right extension and nothing a filesystem will object to.
	name := reader.URI().Name()
	assert.Equal(t, ".png", filepath.Ext(name))
	assert.False(t, containsAny(name, `:/\`))

	// The staged file has to be readable through the same path the attachment
	// code takes, which is the URI's own path rather than the reader.
	onDisk, err := os.ReadFile(reader.URI().Path())
	assert.NoError(t, err)
	assert.Equal(t, data, onDisk)

	streamed := &bytes.Buffer{}
	_, err = streamed.ReadFrom(reader)
	assert.NoError(t, err)
	assert.Equal(t, data, streamed.Bytes())
}

// The extension comes from what the data actually is, not from an assumption
// that everything on the clipboard is png, because the name travels with the
// attachment to whoever receives it.
func TestStageClipboardImageNamesByFormat(t *testing.T) {
	encoded := &bytes.Buffer{}
	err := jpeg.Encode(encoded, sampleImage(), nil)
	assert.NoError(t, err)

	reader, err := stageClipboardImage(encoded.Bytes())
	assert.NoError(t, err)
	defer reader.Close()

	assert.Equal(t, ".jpg", filepath.Ext(reader.URI().Name()))

	encodedPNG := &bytes.Buffer{}
	err = png.Encode(encodedPNG, sampleImage())
	assert.NoError(t, err)

	pngReader, err := stageClipboardImage(encodedPNG.Bytes())
	assert.NoError(t, err)
	defer pngReader.Close()

	assert.Equal(t, ".png", filepath.Ext(pngReader.URI().Name()))
}

func TestStageClipboardImageRejectsNonImages(t *testing.T) {
	_, err := stageClipboardImage([]byte("this is not an image"))
	assert.Error(t, err)
}

func TestStagedAttachmentDirectory(t *testing.T) {
	reader, err := stageClipboardImage(samplePNG(t))
	assert.NoError(t, err)
	defer reader.Close()

	staged := stagedAttachmentDirectory(reader.URI().Path())
	assert.NotEqual(t, "", staged)
	assert.Equal(t, filepath.Join(config.GetConfigDirectory(), "pasted"), filepath.Dir(staged))

	// Removing the staged directory must not reach beyond it.
	err = os.RemoveAll(staged)
	assert.NoError(t, err)

	_, err = os.Stat(config.GetConfigDirectory())
	assert.NoError(t, err)
}

func TestStagedAttachmentDirectoryIgnoresOtherFiles(t *testing.T) {
	// A file the user picked with the file dialog is not ours to delete, and
	// neither is anything that merely sits near the staging directory.
	for _, path := range []string{
		"",
		"/Users/someone/holiday.png",
		filepath.Join(config.GetConfigDirectory(), "blobs", "something"),
		filepath.Join(config.GetConfigDirectory(), "pasted", "loose-file.png"),
	} {
		assert.Equal(t, "", stagedAttachmentDirectory(path))
	}
}

func TestClearStagedAttachments(t *testing.T) {
	reader, err := stageClipboardImage(samplePNG(t))
	assert.NoError(t, err)
	reader.Close()

	clearStagedAttachments()

	_, err = os.Stat(stagingDirectory())
	assert.True(t, os.IsNotExist(err))

	// Clearing when there is nothing there must not be an error either.
	clearStagedAttachments()
}

// An attachment over the embedding limit is seeded from wherever it sits, and
// the path is recorded against it, so it must not be swept away on the next
// start.  Anything at or under the limit is copied into our own storage when it
// is sent, so the staged copy is disposable.
func TestStagingDirectoryFollowsTheEmbeddingLimit(t *testing.T) {
	for _, size := range []int64{0, 1, chat.EmbeddedFileLimit - 1, chat.EmbeddedFileLimit} {
		assert.Equal(t, stagingDirectory(), stagingDirectoryFor(size))
	}

	assert.Equal(t, retainedStagingDirectory(), stagingDirectoryFor(chat.EmbeddedFileLimit+1))
	assert.NotEqual(t, stagingDirectory(), retainedStagingDirectory())
}

// stageRetained puts a file in the retained tree the way stageClipboardImage
// would, marker and all.
func stageRetained(t *testing.T, name string) string {
	t.Helper()

	directory := filepath.Join(retainedStagingDirectory(), name)
	assert.NoError(t, os.MkdirAll(directory, 0700))
	assert.NoError(t, os.WriteFile(stagedAttachmentMarker(directory), nil, 0600))

	path := filepath.Join(directory, "b.png")
	assert.NoError(t, os.WriteFile(path, []byte("x"), 0600))

	return path
}

// A retained attachment that was actually sent is depended on by a message and
// must survive; one that was only ever composed must not be left behind.
func TestSweepKeepsSentRetainedAttachmentsOnly(t *testing.T) {
	t.Cleanup(func() { os.RemoveAll(retainedStagingDirectory()) })

	disposable := filepath.Join(stagingDirectory(), "one", "a.png")
	assert.NoError(t, os.MkdirAll(filepath.Dir(disposable), 0700))
	assert.NoError(t, os.WriteFile(disposable, []byte("x"), 0600))

	sent := stageRetained(t, "sent")
	abandoned := stageRetained(t, "abandoned")

	// Both trees are ours to take back when an attachment is cancelled.
	assert.Equal(t, filepath.Dir(disposable), stagedAttachmentDirectory(disposable))
	assert.Equal(t, filepath.Dir(sent), stagedAttachmentDirectory(sent))

	retainStagedAttachment(sent)

	clearStagedAttachments()

	_, err := os.Stat(disposable)
	assert.True(t, os.IsNotExist(err))

	_, err = os.Stat(abandoned)
	assert.True(t, os.IsNotExist(err))

	_, err = os.Stat(sent)
	assert.NoError(t, err)
}

// Sweeping twice, or with nothing staged, must not be an error either.
func TestSweepIsRepeatable(t *testing.T) {
	clearStagedAttachments()
	clearStagedAttachments()

	sent := stageRetained(t, "kept")
	retainStagedAttachment(sent)

	clearStagedAttachments()
	clearStagedAttachments()

	_, err := os.Stat(sent)
	assert.NoError(t, err)

	assert.NoError(t, os.RemoveAll(retainedStagingDirectory()))
}

// Cancelling a retained attachment takes the marker with it, so the sweep is
// not left tripping over an orphan.
func TestRemoveStagedAttachmentTakesTheMarker(t *testing.T) {
	path := stageRetained(t, "cancelled")
	directory := stagedAttachmentDirectory(path)

	removeStagedAttachment(directory)

	_, err := os.Stat(directory)
	assert.True(t, os.IsNotExist(err))

	_, err = os.Stat(stagedAttachmentMarker(directory))
	assert.True(t, os.IsNotExist(err))
}

func TestIsPastableImagePath(t *testing.T) {
	for _, path := range []string{"/tmp/a.png", "/tmp/a.PNG", "/tmp/a.jpg", "/tmp/a.jpeg", "/tmp/a.gif"} {
		assert.True(t, isPastableImagePath(path))
	}

	// Anything that is not an image is left alone so that copying a file and
	// pasting still pastes text.
	for _, path := range []string{"/tmp/a.txt", "/tmp/a.pdf", "/tmp/a", "/tmp/png", ""} {
		assert.False(t, isPastableImagePath(path))
	}
}

func TestURIListPath(t *testing.T) {
	path, err := uriListPath("file:///home/someone/a%20holiday.png")
	assert.NoError(t, err)
	assert.Equal(t, "/home/someone/a holiday.png", path)

	for _, line := range []string{"https://example.com/a.png", "not a uri at all", "file://"} {
		_, err := uriListPath(line)
		assert.Error(t, err)
	}
}

func TestCopyImageToClipboardRejectsNonImages(t *testing.T) {
	assert.False(t, copyImageToClipboard([]byte("this is not an image")))
	assert.False(t, copyImageToClipboard(nil))
}

// TestClipboardRoundTrip exercises the real system clipboard, which means it
// replaces whatever the person running the tests had on it.  It only runs when
// asked for by name.
func TestClipboardRoundTrip(t *testing.T) {
	if os.Getenv("BOUNCE_CLIPBOARD_TEST") == "" {
		t.Skip("set BOUNCE_CLIPBOARD_TEST=1 to run against the real system clipboard")
	}

	want := sampleImage()
	assert.True(t, copyImageToClipboard(samplePNG(t)))

	data := clipboardImage()
	assert.True(t, len(data) > 0)

	got, _, err := image.Decode(bytes.NewReader(data))
	assert.NoError(t, err)

	assertSameImage(t, want, got)
}

func containsAny(s string, chars string) bool {
	for _, c := range chars {
		for _, in := range s {
			if c == in {
				return true
			}
		}
	}

	return false
}
