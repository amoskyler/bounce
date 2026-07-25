//go:build windows

package ui

import (
	"bytes"
	"errors"
	"image/png"
	"runtime"
	"syscall"
	"unsafe"

	log "github.com/sirupsen/logrus"
	"golang.org/x/sys/windows"
)

var (
	user32   = windows.NewLazySystemDLL("user32.dll")
	kernel32 = windows.NewLazySystemDLL("kernel32.dll")
	shell32  = windows.NewLazySystemDLL("shell32.dll")

	procOpenClipboard             = user32.NewProc("OpenClipboard")
	procCloseClipboard            = user32.NewProc("CloseClipboard")
	procEmptyClipboard            = user32.NewProc("EmptyClipboard")
	procGetClipboardData          = user32.NewProc("GetClipboardData")
	procSetClipboardData          = user32.NewProc("SetClipboardData")
	procIsClipboardFormatAvailabl = user32.NewProc("IsClipboardFormatAvailable")
	procRegisterClipboardFormatW  = user32.NewProc("RegisterClipboardFormatW")

	procGlobalAlloc  = kernel32.NewProc("GlobalAlloc")
	procGlobalFree   = kernel32.NewProc("GlobalFree")
	procGlobalLock   = kernel32.NewProc("GlobalLock")
	procGlobalUnlock = kernel32.NewProc("GlobalUnlock")
	procGlobalSize   = kernel32.NewProc("GlobalSize")

	procDragQueryFileW = shell32.NewProc("DragQueryFileW")
)

const (
	clipboardFormatDIB   = 8
	clipboardFormatHDrop = 15
	clipboardFormatDIBV5 = 17

	globalMemoryMoveable = 0x0002

	// Opening the clipboard fails outright while another application holds it,
	// which happens routinely for the moment after a copy.
	clipboardOpenAttempts = 8
)

// withClipboard opens the clipboard for the duration of fn.  Windows scopes
// clipboard ownership to a thread, so the goroutine has to stay put while it is
// open.
func withClipboard(fn func() error) error {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()

	opened := false
	for attempt := 0; attempt < clipboardOpenAttempts; attempt++ {
		if ret, _, _ := procOpenClipboard.Call(0); ret != 0 {
			opened = true
			break
		}

		windows.SleepEx(10, false)
	}
	if !opened {
		return errors.New("another application is holding the clipboard open")
	}
	defer procCloseClipboard.Call()

	return fn()
}

func clipboardFormatAvailable(format uintptr) bool {
	ret, _, _ := procIsClipboardFormatAvailabl.Call(format)
	return ret != 0
}

func registeredClipboardFormat(name string) uintptr {
	encoded, err := windows.UTF16PtrFromString(name)
	if err != nil {
		return 0
	}

	ret, _, _ := procRegisterClipboardFormatW.Call(uintptr(unsafe.Pointer(encoded)))

	return ret
}

// globalSlice exposes memory locked with GlobalLock as a byte slice.  go vet
// objects to the conversion, and there is no way to avoid it: the lazy
// procedure interface can only hand an address back as a uintptr.  It is sound
// here because the memory belongs to Windows rather than to the go heap, so it
// cannot be moved or collected while the handle is locked.
func globalSlice(pointer uintptr, size int) []byte {
	return unsafe.Slice((*byte)(unsafe.Pointer(pointer)), size)
}

// clipboardBytes copies out the contents of the given clipboard format.  The
// clipboard must already be open, and the returned bytes are a copy because the
// handle stops being ours the moment the clipboard is closed.
func clipboardBytes(format uintptr) []byte {
	handle, _, _ := procGetClipboardData.Call(format)
	if handle == 0 {
		return nil
	}

	pointer, _, _ := procGlobalLock.Call(handle)
	if pointer == 0 {
		return nil
	}
	defer procGlobalUnlock.Call(handle)

	size, _, _ := procGlobalSize.Call(handle)
	if size == 0 {
		return nil
	}

	return bytes.Clone(globalSlice(pointer, int(size)))
}

func clipboardImagesSupported() bool {
	return true
}

func clipboardImage() []byte {
	var image []byte

	err := withClipboard(func() error {
		// Browsers and the modern screenshot tools all offer real PNG, which
		// saves us unpacking a bitmap and is lossless where a bitmap is not.
		if format := registeredClipboardFormat("PNG"); format != 0 && clipboardFormatAvailable(format) {
			if data := clipboardBytes(format); len(data) > 0 {
				image = data
				return nil
			}
		}

		// Everything else on Windows offers a device independent bitmap.
		for _, format := range []uintptr{clipboardFormatDIBV5, clipboardFormatDIB} {
			if !clipboardFormatAvailable(format) {
				continue
			}

			data := clipboardBytes(format)
			if len(data) == 0 {
				continue
			}

			decoded, err := dibToImage(data)
			if err != nil {
				log.WithFields(log.Fields{
					"error": err.Error(),
				}).Debug("error decoding bitmap from the clipboard")
				continue
			}

			encoded := &bytes.Buffer{}
			if err := png.Encode(encoded, decoded); err != nil {
				return err
			}

			image = encoded.Bytes()
			return nil
		}

		return nil
	})
	if err != nil {
		log.WithFields(log.Fields{
			"error": err.Error(),
		}).Debug("error reading an image from the clipboard")
		return nil
	}

	return image
}

func clipboardFilePaths() []string {
	paths := []string{}

	err := withClipboard(func() error {
		if !clipboardFormatAvailable(clipboardFormatHDrop) {
			return nil
		}

		handle, _, _ := procGetClipboardData.Call(clipboardFormatHDrop)
		if handle == 0 {
			return nil
		}

		pointer, _, _ := procGlobalLock.Call(handle)
		if pointer == 0 {
			return nil
		}
		defer procGlobalUnlock.Call(handle)

		// Asking for index 0xFFFFFFFF returns how many files are on the drop
		// rather than the name of one.
		count, _, _ := procDragQueryFileW.Call(pointer, 0xFFFFFFFF, 0, 0)
		for i := uintptr(0); i < count; i++ {
			length, _, _ := procDragQueryFileW.Call(pointer, i, 0, 0)
			if length == 0 {
				continue
			}

			buffer := make([]uint16, length+1)
			written, _, _ := procDragQueryFileW.Call(
				pointer,
				i,
				uintptr(unsafe.Pointer(&buffer[0])),
				uintptr(len(buffer)),
			)
			if written == 0 {
				continue
			}

			paths = append(paths, windows.UTF16ToString(buffer))
		}

		return nil
	})
	if err != nil {
		log.WithFields(log.Fields{
			"error": err.Error(),
		}).Debug("error reading file paths from the clipboard")
		return nil
	}

	return paths
}

// setClipboardFormat hands a copy of data to the clipboard under the given
// format.  The clipboard takes ownership of the allocation on success, so it is
// only freed here when the handover fails.
func setClipboardFormat(format uintptr, data []byte) error {
	handle, _, _ := procGlobalAlloc.Call(globalMemoryMoveable, uintptr(len(data)))
	if handle == 0 {
		return errors.New("out of memory allocating for the clipboard")
	}

	pointer, _, _ := procGlobalLock.Call(handle)
	if pointer == 0 {
		procGlobalFree.Call(handle)
		return errors.New("error locking memory allocated for the clipboard")
	}
	copy(globalSlice(pointer, len(data)), data)
	procGlobalUnlock.Call(handle)

	if ret, _, err := procSetClipboardData.Call(format, handle); ret == 0 {
		procGlobalFree.Call(handle)
		if err != nil && err != syscall.Errno(0) {
			return err
		}
		return errors.New("error placing data on the clipboard")
	}

	return nil
}

func setClipboardImage(data []byte) bool {
	if len(data) == 0 {
		return false
	}

	decoded, err := png.Decode(bytes.NewReader(data))
	if err != nil {
		log.WithFields(log.Fields{
			"error": err.Error(),
		}).Error("error decoding image to copy to the clipboard")
		return false
	}

	err = withClipboard(func() error {
		if ret, _, _ := procEmptyClipboard.Call(); ret == 0 {
			return errors.New("error emptying the clipboard")
		}

		// A bitmap is what the widest range of Windows applications will accept,
		// so it is the one we insist on; PNG is offered alongside it for the
		// applications that prefer to keep the alpha channel intact.
		if err := setClipboardFormat(clipboardFormatDIB, imageToDIB(decoded)); err != nil {
			return err
		}

		if format := registeredClipboardFormat("PNG"); format != 0 {
			if err := setClipboardFormat(format, data); err != nil {
				log.WithFields(log.Fields{
					"error": err.Error(),
				}).Debug("error offering the clipboard image as png")
			}
		}

		return nil
	})
	if err != nil {
		log.WithFields(log.Fields{
			"error": err.Error(),
		}).Error("error copying image to the clipboard")
		return false
	}

	return true
}
