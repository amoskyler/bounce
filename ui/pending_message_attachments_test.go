package ui

import (
	"os"
	"path/filepath"
	"testing"

	"fyne.io/fyne/v2/storage"
	"github.com/alecthomas/assert/v2"
	"github.com/bounce-chat/bounce/chat"
	"github.com/google/uuid"
)

func attachmentForFile(t *testing.T, name string, size int) (*messageAttachment, error) {
	t.Helper()

	path := filepath.Join(t.TempDir(), name)
	err := os.WriteFile(path, make([]byte, size), 0600)
	assert.NoError(t, err)

	reader, err := storage.Reader(storage.NewFileURI(path))
	assert.NoError(t, err)

	return newPendingMessageAttachment(uuid.New(), reader, func() {})
}

// A file at or over the embedded limit takes none of the branches that assign
// an icon, and every attachment is built expecting one.
func TestPendingAttachmentAtAndOverEmbeddedLimit(t *testing.T) {
	for name, size := range map[string]int{
		"under the limit": chat.EmbeddedFileLimit - 1,
		"at the limit":    chat.EmbeddedFileLimit,
		"over the limit":  chat.EmbeddedFileLimit + 1,
	} {
		t.Run(name, func(t *testing.T) {
			ma, err := attachmentForFile(t, "attachment.bin", size)
			assert.NoError(t, err)
			assert.NotZero(t, ma)
			assert.NotZero(t, ma.icon)
			assert.Equal(t, int64(size), ma.fileSize)

			// Building the renderer is what would dereference a missing icon.
			assert.NotZero(t, ma.CreateRenderer())
		})
	}
}
