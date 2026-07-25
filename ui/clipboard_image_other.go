//go:build ios || android || (!darwin && !linux && !windows)

package ui

// Android keeps images on the clipboard behind a ClipData content URI that only
// the activity can resolve, which is a different piece of plumbing to the one
// the desktop platforms share.  Until that exists, pasting on these platforms
// falls through to the normal text handling.

func clipboardImagesSupported() bool {
	return false
}

func clipboardImage() []byte {
	return nil
}

func clipboardFilePaths() []string {
	return nil
}

func setClipboardImage(_ []byte) bool {
	return false
}
