//go:build darwin && !ios

package ui

/*
#cgo darwin LDFLAGS: -framework Cocoa

#include <stddef.h>
#include <stdlib.h>

void *bounceClipboardImagePNG(size_t *length);
char *bounceClipboardFilePaths(size_t *length, int *count);
int bounceClipboardSetImagePNG(const void *bytes, size_t length);
*/
import "C"

import (
	"bytes"
	"unsafe"
)

// clipboardImagesSupported reports whether this platform can put an image on
// the clipboard at all, so that the interface can leave out an action that
// could only ever fail.
func clipboardImagesSupported() bool {
	return true
}

func clipboardImage() []byte {
	var length C.size_t

	buffer := C.bounceClipboardImagePNG(&length)
	if buffer == nil {
		return nil
	}
	defer C.free(buffer)

	return C.GoBytes(buffer, C.int(length))
}

func clipboardFilePaths() []string {
	var length C.size_t
	var count C.int

	buffer := C.bounceClipboardFilePaths(&length, &count)
	if buffer == nil {
		return nil
	}
	defer C.free(unsafe.Pointer(buffer))

	paths := []string{}
	for _, path := range bytes.Split(C.GoBytes(unsafe.Pointer(buffer), C.int(length)), []byte{0}) {
		if len(path) > 0 {
			paths = append(paths, string(path))
		}
	}

	return paths
}

func setClipboardImage(png []byte) bool {
	if len(png) == 0 {
		return false
	}

	return C.bounceClipboardSetImagePNG(unsafe.Pointer(&png[0]), C.size_t(len(png))) != 0
}
