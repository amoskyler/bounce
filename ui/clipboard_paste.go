package ui

import (
	"bytes"
	"errors"
	"image"
	_ "image/gif"
	_ "image/jpeg"
	"image/png"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/dialog"
	"fyne.io/fyne/v2/storage"
	"github.com/bounce-chat/bounce/chat"
	"github.com/bounce-chat/bounce/config"
	"github.com/google/uuid"
	log "github.com/sirupsen/logrus"
)

// An image pasted into a message arrives as bytes with no file behind it, but
// the rest of the attachment path is built around a file on disk, so we stage
// one.  These live in the config directory rather than the system temporary
// directory so that they inherit the same private permissions as everything
// else we write, and each paste gets its own directory so that the file itself
// can keep the name the user will see on the message.
func stagingDirectory() string {
	return filepath.Join(config.GetConfigDirectory(), "pasted")
}

// An attachment small enough to be embedded is copied into our own storage when
// it is sent, which leaves the staged file free to be thrown away.  One too
// large for that is seeded from where it sits instead, and the path is recorded
// against the file, so that one has to stay put for as long as the attachment
// exists.  The two are kept apart so that the sweep can tell them apart.
func retainedStagingDirectory() string {
	return filepath.Join(config.GetConfigDirectory(), "pasted-retained")
}

// stagingDirectoryFor picks between the two by the same limit the send path
// uses to decide whether an attachment is embedded or seeded.
func stagingDirectoryFor(size int64) string {
	if size > chat.EmbeddedFileLimit {
		return retainedStagingDirectory()
	}

	return stagingDirectory()
}

// A retained attachment is marked as pending from the moment it is staged until
// the message carrying it is sent.  The marker is what tells the sweep the
// difference between an image that a message out there depends on and one that
// was pasted into a composer and then abandoned, which would otherwise sit on
// disk for good.
const stagedAttachmentMarkerSuffix = ".pending"

func stagedAttachmentMarker(directory string) string {
	return directory + stagedAttachmentMarkerSuffix
}

// clearStagedAttachments removes anything left behind by a previous run.  This
// is done at startup rather than after a send because sends are asynchronous
// and there is no point at which we can know the file is finished with; at
// startup nothing can be pending, so nothing can be in use.
func clearStagedAttachments() {
	err := os.RemoveAll(stagingDirectory())
	if err != nil {
		log.WithFields(log.Fields{
			"error": err.Error(),
		}).Debug("error clearing staged attachments from a previous run")
	}

	// The retained ones are only swept if they never made it into a message
	entries, err := os.ReadDir(retainedStagingDirectory())
	if err != nil {
		return
	}

	for _, entry := range entries {
		if !strings.HasSuffix(entry.Name(), stagedAttachmentMarkerSuffix) {
			continue
		}

		removeStagedAttachment(filepath.Join(
			retainedStagingDirectory(),
			strings.TrimSuffix(entry.Name(), stagedAttachmentMarkerSuffix),
		))
	}
}

// removeStagedAttachment takes back a staged directory and the marker that goes
// with it.
func removeStagedAttachment(directory string) {
	for _, path := range []string{directory, stagedAttachmentMarker(directory)} {
		err := os.RemoveAll(path)
		if err != nil {
			log.WithFields(log.Fields{
				"path":  path,
				"error": err.Error(),
			}).Debug("error removing a staged attachment")
		}
	}
}

// retainStagedAttachment records that an attachment has made it into a message,
// and so is no longer the sweep's to take back.
func retainStagedAttachment(path string) {
	directory := stagedAttachmentDirectory(path)
	if directory == "" {
		return
	}

	err := os.Remove(stagedAttachmentMarker(directory))
	if err != nil && !os.IsNotExist(err) {
		log.WithFields(log.Fields{
			"path":  directory,
			"error": err.Error(),
		}).Error("error marking a staged attachment as sent, it may be swept away")
	}
}

// stagedAttachmentDirectory reports the directory to remove when an attachment
// staged by a paste is cancelled, or an empty string for an attachment that
// refers to a file we do not own.  Cancelling is the one moment we know a
// retained file was never sent, so it is safe to take that one back too.
func stagedAttachmentDirectory(path string) string {
	if path == "" {
		return ""
	}

	parent := filepath.Dir(path)
	staging := filepath.Dir(parent)

	if staging != stagingDirectory() && staging != retainedStagingDirectory() {
		return ""
	}

	return parent
}

// pastableImageExtensions are the formats we will pick up from a file copied in
// a file manager.  Anything else is left alone so that copying a file and
// pasting into the message box still pastes text, which is what someone who
// copied a path is expecting.
var pastableImageExtensions = map[string]bool{
	".png":  true,
	".jpg":  true,
	".jpeg": true,
	".gif":  true,
}

func isPastableImagePath(path string) bool {
	return pastableImageExtensions[strings.ToLower(filepath.Ext(path))]
}

// uriListPath turns one line of a text/uri-list into a local path.  Anything
// that is not a file on this machine is refused rather than guessed at: what is
// on the clipboard is not necessarily something the user put there, and reading
// through a uri naming another host would turn a paste into a network fetch.
func uriListPath(line string) (string, error) {
	parsed, err := url.Parse(strings.TrimSpace(line))
	if err != nil {
		return "", err
	}

	if parsed.Scheme != "file" {
		return "", errors.New("uri does not refer to a local file")
	}
	if parsed.Host != "" && parsed.Host != "localhost" {
		return "", errors.New("uri refers to a file on another host")
	}
	if parsed.Path == "" {
		return "", errors.New("uri has no path")
	}

	return parsed.Path, nil
}

// stageClipboardImage writes image data from the clipboard to a file we can
// attach, returning a reader over it.
func stageClipboardImage(data []byte) (fyne.URIReadCloser, error) {
	// Decoding the header both tells us what to call the file and keeps us from
	// staging something that is not an image at all.
	_, format, err := image.DecodeConfig(bytes.NewReader(data))
	if err != nil {
		return nil, err
	}

	extension := "." + format
	if format == "jpeg" {
		extension = ".jpg"
	}

	// A colon is illegal in a filename on Windows and is shown as a slash on
	// macOS, so the time is punctuated with dots the way screenshots are.
	name := "Pasted Image " + time.Now().Format("2006-01-02 at 15.04.05") + extension

	staging := stagingDirectoryFor(int64(len(data)))
	directory := filepath.Join(staging, uuid.New().String())
	err = os.MkdirAll(directory, 0700)
	if err != nil {
		return nil, err
	}

	// A retained attachment is only kept once it is actually sent
	if staging == retainedStagingDirectory() {
		err = os.WriteFile(stagedAttachmentMarker(directory), nil, 0600)
		if err != nil {
			removeStagedAttachment(directory)
			return nil, err
		}
	}

	path := filepath.Join(directory, name)
	err = os.WriteFile(path, data, 0600)
	if err != nil {
		removeStagedAttachment(directory)
		return nil, err
	}

	reader, err := storage.Reader(storage.NewFileURI(path))
	if err != nil {
		removeStagedAttachment(directory)
		return nil, err
	}

	return reader, nil
}

// copyImageToClipboard puts an attachment on the system clipboard.  Attachments
// arrive in whatever format they were sent in, so anything that is not already
// png is re-encoded first; png is the one format every platform's clipboard
// agrees on, and the only one the paste side knows how to read back.
func copyImageToClipboard(data []byte) bool {
	_, format, err := image.DecodeConfig(bytes.NewReader(data))
	if err != nil {
		log.WithFields(log.Fields{
			"error": err.Error(),
		}).Error("error reading image to copy to the clipboard")
		return false
	}

	if format != "png" {
		decoded, _, err := image.Decode(bytes.NewReader(data))
		if err != nil {
			log.WithFields(log.Fields{
				"error": err.Error(),
			}).Error("error decoding image to copy to the clipboard")
			return false
		}

		encoded := &bytes.Buffer{}
		err = png.Encode(encoded, decoded)
		if err != nil {
			log.WithFields(log.Fields{
				"error": err.Error(),
			}).Error("error encoding image to copy to the clipboard")
			return false
		}

		data = encoded.Bytes()
	}

	return setClipboardImage(data)
}

// pasteAttachments attaches any image the clipboard is holding to the message
// being composed, reporting whether it took the paste.  A paste it does not
// take is left to the text entry to handle as usual.
func (ui *ui) pasteAttachments(pmas *pendingMessageAttachments, refocus func()) bool {
	// A file copied in a file manager is attached as itself, so that it keeps
	// its own name and its original bytes rather than being re-encoded.
	attached := false
	for _, path := range clipboardFilePaths() {
		if !isPastableImagePath(path) {
			continue
		}

		reader, err := storage.Reader(storage.NewFileURI(path))
		if err != nil {
			log.WithFields(log.Fields{
				"path":  path,
				"error": err.Error(),
			}).Debug("error opening an image copied to the clipboard")
			continue
		}

		err = pmas.add(reader, refocus)
		if err != nil {
			// Anything os.Open accepts gets this far, including a directory
			// that happens to be named like an image
			reader.Close()
			ui.showDialog(dialog.NewError(err, ui.window), nil)
			continue
		}

		attached = true
	}
	if attached {
		return true
	}

	// Pixels sitting alongside text almost always means the text is the real
	// content and the picture is a courtesy rendering of it, which is what a
	// spreadsheet or a word processor puts on the clipboard for a copied
	// selection.  Copying cells and pasting them should give the cells, so text
	// wins whenever there is any.  Nothing that exists to be pasted as a
	// picture offers text alongside it: a screenshot carries pixels and nothing
	// else, and copying an image in a browser gives markup rather than a line
	// of text.  A file copied in a file manager does come with its name as
	// text, which is why that case is settled above this point.
	if fyne.CurrentApp().Clipboard().Content() != "" {
		return false
	}

	data := clipboardImage()
	if len(data) == 0 {
		return false
	}

	reader, err := stageClipboardImage(data)
	if err != nil {
		log.WithFields(log.Fields{
			"error": err.Error(),
		}).Error("error staging an image pasted from the clipboard")

		// There is an image on the clipboard, so this paste was ours to handle
		// and the failure has to be said out loud.  Handing it back to the text
		// entry would silently erase whatever the user had selected instead.
		ui.showDialog(dialog.NewError(errors.New("This image could not be attached: "+err.Error()), ui.window), nil)

		return true
	}

	err = pmas.add(reader, refocus)
	if err != nil {
		reader.Close()
		if staged := stagedAttachmentDirectory(reader.URI().Path()); staged != "" {
			removeStagedAttachment(staged)
		}
		ui.showDialog(dialog.NewError(err, ui.window), nil)
		return true
	}

	return true
}
