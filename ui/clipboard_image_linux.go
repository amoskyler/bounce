//go:build linux && !android

package ui

import (
	"bytes"
	"context"
	"os"
	"os/exec"
	"strings"
	"time"

	log "github.com/sirupsen/logrus"
)

// X11 has no way to read a selection without owning a window and pumping an
// event loop for it, and Wayland does not expose the clipboard to clients at
// all outside of a focused surface.  Rather than reach around the toolkit for
// either, we shell out to the standard helper for whichever display server is
// in use, the same way most applications outside of a full desktop environment
// do.  Both helpers are small and widely packaged, and the feature degrades to
// a log line when neither is installed.
type clipboardHelper struct {
	read     []string
	listType []string
	write    []string
}

// clipboardHelpers lists the helpers worth trying, best first.  Both are
// returned when both are installed rather than only the one matching the
// session, because reading the Wayland clipboard needs a data control protocol
// that not every compositor implements; when wl-paste comes back with nothing
// the same selection is usually still readable through XWayland with xclip.
func clipboardHelpers() []clipboardHelper {
	helpers := []clipboardHelper{}

	if os.Getenv("WAYLAND_DISPLAY") != "" {
		if _, err := exec.LookPath("wl-paste"); err == nil {
			helpers = append(helpers, clipboardHelper{
				read:     []string{"wl-paste", "--no-newline", "--type"},
				listType: []string{"wl-paste", "--list-types"},
				write:    []string{"wl-copy", "--type"},
			})
		}
	}

	if _, err := exec.LookPath("xclip"); err == nil {
		helpers = append(helpers, clipboardHelper{
			read:     []string{"xclip", "-selection", "clipboard", "-o", "-t"},
			listType: []string{"xclip", "-selection", "clipboard", "-o", "-t", "TARGETS"},
			write:    []string{"xclip", "-selection", "clipboard", "-i", "-t"},
		})
	}

	if len(helpers) == 0 {
		log.Debug("cannot read images from the clipboard without wl-paste or xclip installed")
	}

	return helpers
}

// A paste is handled on the same goroutine that draws the window, so a helper
// that hangs would hang the whole application.  Reading a selection means
// waiting on whichever program owns it, and that program can be busy, stopped
// or gone, so every call is given a deadline it cannot outlive.
const clipboardHelperTimeout = 2 * time.Second

func (helper clipboardHelper) run(argv []string) []byte {
	ctx, cancel := context.WithTimeout(context.Background(), clipboardHelperTimeout)
	defer cancel()

	out, err := exec.CommandContext(ctx, argv[0], argv[1:]...).Output()
	if err != nil {
		if ctx.Err() != nil {
			log.WithFields(log.Fields{
				"helper": argv[0],
			}).Warn("timed out reading the clipboard, whoever owns it is not answering")
		}

		return nil
	}

	return out
}

// types is the one call every read starts with, so it is asked for once and
// passed around rather than being re-run for each format we are curious about.
func (helper clipboardHelper) types() []string {
	return strings.Fields(string(helper.run(helper.listType)))
}

func (helper clipboardHelper) get(mime string) []byte {
	return helper.run(append(append([]string{}, helper.read...), mime))
}

func offers(types []string, mime string) bool {
	for _, offered := range types {
		if offered == mime {
			return true
		}
	}

	return false
}

// Without one of the helpers installed there is no way to reach the clipboard,
// so the interface should not offer to.
func clipboardImagesSupported() bool {
	return len(clipboardHelpers()) > 0
}

func clipboardImage() []byte {
	for _, helper := range clipboardHelpers() {
		types := helper.types()

		// PNG first because it is what screenshot tools and browsers offer, and
		// because it is the format the rest of the paste path expects.
		for _, mime := range []string{"image/png", "image/jpeg", "image/gif"} {
			if !offers(types, mime) {
				continue
			}

			if data := helper.get(mime); len(data) > 0 {
				return data
			}
		}
	}

	return nil
}

func clipboardFilePaths() []string {
	for _, helper := range clipboardHelpers() {
		if !offers(helper.types(), "text/uri-list") {
			continue
		}

		paths := []string{}
		for _, line := range strings.Split(string(helper.get("text/uri-list")), "\n") {
			line = strings.TrimSpace(line)

			// A uri-list may carry comments, and file managers copy things that
			// are not files at all, neither of which we can attach.
			if line == "" || strings.HasPrefix(line, "#") {
				continue
			}

			path, err := uriListPath(line)
			if err != nil {
				log.WithFields(log.Fields{
					"uri":   line,
					"error": err.Error(),
				}).Debug("skipping unusable file uri on the clipboard")
				continue
			}

			paths = append(paths, path)
		}

		if len(paths) > 0 {
			return paths
		}
	}

	return nil
}

func setClipboardImage(png []byte) bool {
	if len(png) == 0 {
		return false
	}

	for _, helper := range clipboardHelpers() {
		if _, err := exec.LookPath(helper.write[0]); err != nil {
			continue
		}

		command := exec.Command(helper.write[0], append(helper.write[1:], "image/png")...)
		command.Stdin = bytes.NewReader(png)

		// Both helpers have to stay resident to serve the selection to whoever
		// pastes it, so we start them and leave them running rather than
		// waiting on them.
		err := command.Start()
		if err != nil {
			log.WithFields(log.Fields{
				"helper": helper.write[0],
				"error":  err.Error(),
			}).Error("error copying image to the clipboard")
			continue
		}
		go command.Wait()

		return true
	}

	return false
}
